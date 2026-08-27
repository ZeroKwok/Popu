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
use crate::placement::WindowPlacement;
use crate::protocol::{Mode, Request, Response, ResponseStatus, Screen, ScreenArea};
use crate::templates::{self, TemplateAssets};

pub const EXIT_OK: i32 = 0;
pub const EXIT_CANCELLED: i32 = 10;
pub const EXIT_TIMED_OUT: i32 = 11;
pub const EXIT_ERROR: i32 = 12;

#[derive(Clone, Serialize)]
pub struct Bootstrap {
    pub request: Request,
    pub template: TemplateAssets,
    pub maximized: bool,
}

pub struct RuntimeState {
    bootstrap: Bootstrap,
    done: AtomicBool,
    exit_code: Arc<AtomicI32>,
    print_ready: bool,
    placement: WindowPlacement,
    maximized: bool,
}

impl RuntimeState {
    fn new(
        request: Request,
        template: TemplateAssets,
        print_ready: bool,
        exit_code: Arc<AtomicI32>,
        placement: WindowPlacement,
        maximized: bool,
    ) -> Self {
        Self {
            bootstrap: Bootstrap {
                request,
                template,
                maximized,
            },
            done: AtomicBool::new(false),
            exit_code,
            print_ready,
            placement,
            maximized,
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
    position_window(&window, state.placement)?;
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
    let placement = match resolve_window_placement(&request) {
        Ok(placement) => placement,
        Err(error) => {
            eprintln!("Popu display error: {error}");
            return EXIT_ERROR;
        }
    };
    let maximized = request.window.is_maximized();
    let state = RuntimeState::new(
        request,
        template,
        print_ready,
        Arc::clone(&exit_code),
        placement,
        maximized,
    );

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
            let (width, height) = state.placement.size;
            let is_notify = state.bootstrap.request.mode == Mode::Notify;
            let window =
                WebviewWindowBuilder::new(app, "popu", WebviewUrl::App("index.html".into()))
                    .title(&state.bootstrap.request.content.title)
                    .inner_size(f64::from(width), f64::from(height))
                    .min_inner_size(1.0, 1.0)
                    .decorations(false)
                    .transparent(true)
                    .resizable(false)
                    .always_on_top(state.bootstrap.request.window.always_on_top)
                    .skip_taskbar(is_notify)
                    .focused(!is_notify)
                    .visible(false)
                    // Windows adds a separate native layer for an undecorated
                    // shadow. In a transparent WebView this is visible as an
                    // oversized rounded frame around the HTML card, so Popu
                    // deliberately has no native shadow.
                    .shadow(false)
                    .build()?;

            #[cfg(debug_assertions)]
            if std::env::var_os("POPU_DEVTOOLS").is_some() {
                window.open_devtools();
            }

            #[cfg(windows)]
            if let Err(error) = configure_frameless_windows_window(&window, state.maximized) {
                // Transparency remains useful even if a system policy or an
                // unexpected platform detail prevents the native refinements.
                eprintln!("Popu warning: cannot configure frameless window: {error}");
            }

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

/// Applies the Windows-only parts of Popu's frameless-window contract.
#[cfg(windows)]
fn configure_frameless_windows_window(
    window: &WebviewWindow,
    maximized: bool,
) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };

    const CARD_RADIUS_LOGICAL_PX: f64 = 14.0;

    let handle = window.window_handle().map_err(|error| error.to_string())?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err("window does not expose a Win32 handle".into());
    };
    let hwnd = handle.hwnd.get() as HWND;
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    let radius = (CARD_RADIUS_LOGICAL_PX * scale).round().max(1.0) as i32;
    let width = i32::try_from(size.width).map_err(|_| "window width is too large")?;
    let height = i32::try_from(size.height).map_err(|_| "window height is too large")?;

    unsafe {
        // Tauri asks for an undecorated window, but Windows may repaint a
        // retained non-client area after activation. Remove caption/system
        // styles at the HWND boundary and force a non-client recalculation.
        let current_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let frameless_style = current_style
            & !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX);
        SetWindowLongPtrW(hwnd, GWL_STYLE, frameless_style as isize);
        let frame_changed =
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
        if SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, frame_changed) == 0 {
            return Err("SetWindowPos failed while removing native caption".into());
        }

        // SetWindowRgn takes ownership on success; on failure we retain it and
        // must release it ourselves.
        let region = if maximized {
            CreateRectRgn(0, 0, width + 1, height + 1)
        } else {
            CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2)
        };
        if region.is_null() {
            return Err("CreateRoundRectRgn failed".into());
        }
        if SetWindowRgn(hwnd, region, 1) == 0 {
            let _ = DeleteObject(region);
            return Err("SetWindowRgn failed".into());
        }
    }
    Ok(())
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
fn resolve_window_placement(request: &Request) -> Result<WindowPlacement, String> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut cursor = POINT { x: 0, y: 0 };
    unsafe {
        if request.window.screen == Screen::Cursor && GetCursorPos(&mut cursor) == 0 {
            return Err("GetCursorPos failed".into());
        }
        let monitor = MonitorFromPoint(
            if request.window.screen == Screen::Primary {
                POINT {
                    x: i32::MIN,
                    y: i32::MIN,
                }
            } else {
                cursor
            },
            MONITOR_DEFAULTTOPRIMARY,
        );
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: Default::default(),
            rcWork: Default::default(),
            dwFlags: 0,
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return Err("GetMonitorInfoW failed".into());
        }
        let rect = match request.window.area {
            ScreenArea::WorkArea => info.rcWork,
            ScreenArea::Screen => info.rcMonitor,
        };
        let area = crate::placement::Rect {
            x: rect.left,
            y: rect.top,
            width: (rect.right - rect.left).max(0) as u32,
            height: (rect.bottom - rect.top).max(0) as u32,
        };
        if area.width == 0 || area.height == 0 {
            return Err("target display area is empty".into());
        }
        Ok(crate::placement::resolve(request, area, 16))
    }
}

#[cfg(not(windows))]
fn resolve_window_placement(request: &Request) -> Result<WindowPlacement, String> {
    Ok(crate::placement::resolve(
        request,
        crate::placement::Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        16,
    ))
}

fn position_window(window: &WebviewWindow, placement: WindowPlacement) -> Result<(), String> {
    window
        .set_position(PhysicalPosition::new(
            placement.position.x,
            placement.position.y,
        ))
        .map_err(|error| error.to_string())?;
    Ok(())
}
