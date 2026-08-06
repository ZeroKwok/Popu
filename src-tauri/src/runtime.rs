use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Map, Value};
use tauri::{
    AppHandle, Manager, PhysicalPosition, Runtime, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::lifetime;
use crate::protocol::{Mode, Request, Response, ResponseStatus};
use crate::templates::{self, TemplateAssets};

pub const EXIT_OK: i32 = 0;
pub const EXIT_CANCELLED: i32 = 10;
pub const EXIT_TIMED_OUT: i32 = 11;
pub const EXIT_ERROR: i32 = 12;

#[derive(Clone, Serialize)]
pub struct Bootstrap {
    pub request: Request,
    pub template: TemplateAssets,
}

pub struct RuntimeState {
    bootstrap: Bootstrap,
    done: AtomicBool,
    exit_code: Arc<AtomicI32>,
    print_ready: bool,
}

impl RuntimeState {
    fn new(
        request: Request,
        template: TemplateAssets,
        print_ready: bool,
        exit_code: Arc<AtomicI32>,
    ) -> Self {
        Self {
            bootstrap: Bootstrap { request, template },
            done: AtomicBool::new(false),
            exit_code,
            print_ready,
        }
    }
}

#[tauri::command]
fn get_bootstrap(state: State<'_, RuntimeState>) -> Bootstrap {
    state.bootstrap.clone()
}

#[tauri::command]
fn submit(
    app: AppHandle,
    state: State<'_, RuntimeState>,
    action: String,
    values: Map<String, Value>,
) -> Result<(), String> {
    let response = state
        .bootstrap
        .request
        .submitted_response(&action, values)
        .map_err(|error| error.to_string())?;
    complete(&app, &state, response, EXIT_OK);
    Ok(())
}

#[tauri::command]
fn finish(
    app: AppHandle,
    state: State<'_, RuntimeState>,
    status: ResponseStatus,
    action: Option<String>,
) -> Result<(), String> {
    if state.bootstrap.request.mode == Mode::Notify {
        if status == ResponseStatus::Closed && action.is_none() {
            abandon(&app, &state, EXIT_OK);
            return Ok(());
        }
        return Err("notifications can only be closed without an action".into());
    }
    if status == ResponseStatus::Submitted || status == ResponseStatus::TimedOut {
        return Err("the requested status cannot be submitted by the renderer".into());
    }
    if let Some(action_id) = action.as_deref() {
        let valid =
            state.bootstrap.request.actions.iter().any(|candidate| {
                candidate.id == action_id && candidate.result_status == Some(status)
            });
        if !valid {
            return Err(format!(
                "action {action_id} cannot produce status {status:?}"
            ));
        }
    }
    let response = state.bootstrap.request.terminal_response(status, action);
    complete(&app, &state, response, exit_code_for(status));
    Ok(())
}

#[tauri::command]
fn render_failed(app: AppHandle, state: State<'_, RuntimeState>, message: String) {
    fail(&app, &state, format!("renderer failed: {message}"));
}

#[tauri::command]
fn ui_ready(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, RuntimeState>,
) -> Result<(), String> {
    position_window(&window, state.bootstrap.request.position)?;
    window.show().map_err(|error| error.to_string())?;
    if state.bootstrap.request.mode == Mode::Dialog {
        window.set_focus().map_err(|error| error.to_string())?;
    }
    if state.print_ready {
        println!("{{\"status\":\"shown\"}}");
        io::stdout().flush().map_err(|error| error.to_string())?;
    }
    if state.bootstrap.request.mode == Mode::Notify {
        let duration = state.bootstrap.request.duration_ms;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(duration));
            if let Some(state) = app.try_state::<RuntimeState>() {
                abandon(&app, &state, EXIT_OK);
            }
        });
    }
    Ok(())
}

pub fn run_ui(
    request: Request,
    template_dir: Option<&Path>,
    lifetime_handle: Option<u64>,
    print_ready: bool,
) -> i32 {
    let template = match templates::resolve(&request, template_dir) {
        Ok(template) => template,
        Err(error) => {
            eprintln!("Popu template error: {error}");
            return EXIT_ERROR;
        }
    };
    let exit_code = Arc::new(AtomicI32::new(EXIT_OK));
    let state = RuntimeState::new(request, template, print_ready, Arc::clone(&exit_code));

    let builder = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_bootstrap,
            submit,
            finish,
            render_failed,
            ui_ready
        ])
        .on_window_event(handle_window_event)
        .setup(move |app| {
            let state = app.state::<RuntimeState>();
            let (width, height) = state.bootstrap.request.window_size();
            let is_notify = state.bootstrap.request.mode == Mode::Notify;
            WebviewWindowBuilder::new(app, "popu", WebviewUrl::App("index.html".into()))
                .title(&state.bootstrap.request.content.title)
                .inner_size(width, height)
                .min_inner_size(160.0, 160.0)
                .decorations(false)
                .resizable(false)
                .always_on_top(state.bootstrap.request.window.always_on_top)
                .skip_taskbar(is_notify)
                .focused(!is_notify)
                .visible(false)
                .shadow(true)
                .build()?;

            if state.bootstrap.request.mode == Mode::Dialog {
                let timeout = state.bootstrap.request.timeout_seconds;
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(timeout));
                    if let Some(state) = handle.try_state::<RuntimeState>() {
                        let response = state
                            .bootstrap
                            .request
                            .terminal_response(ResponseStatus::TimedOut, None);
                        complete(&handle, &state, response, EXIT_TIMED_OUT);
                    }
                });
            }

            if let Some(raw_handle) = lifetime_handle {
                let handle = app.handle().clone();
                lifetime::monitor(raw_handle, move || {
                    if let Some(state) = handle.try_state::<RuntimeState>() {
                        abandon(&handle, &state, EXIT_ERROR);
                    }
                })?;
            }
            Ok(())
        });

    let result = builder.build(tauri::generate_context!());
    let app = match result {
        Ok(app) => app,
        Err(error) => {
            eprintln!("Popu startup error: {error}");
            return EXIT_ERROR;
        }
    };
    // `run_return` lets the CLI finish in `main`; the shared value is used
    // because some Windows event-loop versions normalize the returned code.
    let _ = app.run_return(|_, _| {});
    exit_code.load(Ordering::SeqCst)
}

pub fn handle_window_event<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    if !matches!(event, WindowEvent::CloseRequested { .. }) {
        return;
    }
    let app = window.app_handle();
    let state = app.state::<RuntimeState>();
    if state.bootstrap.request.mode == Mode::Dialog {
        let response = state
            .bootstrap
            .request
            .terminal_response(ResponseStatus::Closed, None);
        complete(app, &state, response, EXIT_CANCELLED);
    } else {
        abandon(app, &state, EXIT_OK);
    }
}

fn complete<R: Runtime>(app: &AppHandle<R>, state: &RuntimeState, response: Response, code: i32) {
    if state.done.swap(true, Ordering::SeqCst) {
        return;
    }
    match serde_json::to_string(&response) {
        Ok(json) => {
            println!("{json}");
            let _ = io::stdout().flush();
            state.exit_code.store(code, Ordering::SeqCst);
            shutdown(app, code);
        }
        Err(error) => fail(app, state, format!("cannot serialize response: {error}")),
    }
}

fn fail<R: Runtime>(app: &AppHandle<R>, state: &RuntimeState, message: String) {
    if state.done.swap(true, Ordering::SeqCst) {
        return;
    }
    eprintln!("Popu error: {message}");
    state.exit_code.store(EXIT_ERROR, Ordering::SeqCst);
    shutdown(app, EXIT_ERROR);
}

fn abandon<R: Runtime>(app: &AppHandle<R>, state: &RuntimeState, code: i32) {
    if state.done.swap(true, Ordering::SeqCst) {
        return;
    }
    state.exit_code.store(code, Ordering::SeqCst);
    shutdown(app, code);
}

fn shutdown<R: Runtime>(app: &AppHandle<R>, code: i32) {
    if let Some(window) = app.get_webview_window("popu") {
        let _ = window.destroy();
    }
    app.exit(code);
}

fn exit_code_for(status: ResponseStatus) -> i32 {
    match status {
        ResponseStatus::Submitted => EXIT_OK,
        ResponseStatus::Cancelled | ResponseStatus::Closed => EXIT_CANCELLED,
        ResponseStatus::TimedOut => EXIT_TIMED_OUT,
    }
}

#[cfg(windows)]
fn position_window(
    window: &WebviewWindow,
    position: crate::protocol::Position,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut cursor = POINT { x: 0, y: 0 };
    unsafe {
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: Default::default(),
            rcWork: Default::default(),
            dwFlags: 0,
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return Err("GetMonitorInfoW failed".into());
        }
        let size = window.outer_size().map_err(|error| error.to_string())?;
        let work_area = crate::placement::Rect {
            x: info.rcWork.left,
            y: info.rcWork.top,
            width: (info.rcWork.right - info.rcWork.left).max(0) as u32,
            height: (info.rcWork.bottom - info.rcWork.top).max(0) as u32,
        };
        let point = crate::placement::calculate(position, work_area, (size.width, size.height), 16);
        window
            .set_position(PhysicalPosition::new(point.x, point.y))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn position_window(
    window: &WebviewWindow,
    position: crate::protocol::Position,
) -> Result<(), String> {
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no monitor available".to_owned())?;
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let area = crate::placement::Rect {
        x: monitor.position().x,
        y: monitor.position().y,
        width: monitor.size().width,
        height: monitor.size().height,
    };
    let point = crate::placement::calculate(position, area, (size.width, size.height), 16);
    window
        .set_position(PhysicalPosition::new(point.x, point.y))
        .map_err(|error| error.to_string())?;
    Ok(())
}
