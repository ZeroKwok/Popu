use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::ListenerExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use uuid::Uuid;

use crate::core::RequestRegistry;
use crate::ipc::{self, RpcRequest, RpcResponse, RPC_VERSION};
use crate::history::HistoryStore;
use crate::protocol::{Content, Mode, Request, ResponseStatus};
use crate::{placement, runtime};

#[path = "host/coordinator.rs"] mod coordinator;
#[path = "host/windows.rs"] mod windows;
use coordinator::{Command, Task};
use crate::templates::{self, TemplateAssets};
use crate::tray;

pub struct HostState {
    registry: Mutex<RequestRegistry>,
    changed: Condvar,
    stopping: AtomicBool,
    history: Mutex<Option<HistoryStore>>,
    sender: Mutex<Option<Sender<Task>>>,
    history_window: Mutex<Option<HistoryWindowSession>>,
}

struct HistoryWindowSession { started: Instant, ready: bool, detail_ready: bool, open_requested: bool }

impl Default for HostState {
    fn default() -> Self { Self::with_history(HistoryStore::open_default().ok()) }
}

impl HostState {
    fn with_history(history: Option<HistoryStore>) -> Self {
        Self { registry: Mutex::new(RequestRegistry::default()), changed: Condvar::new(), stopping: AtomicBool::new(false), history: Mutex::new(history), sender: Mutex::new(None), history_window: Mutex::new(None) }
    }
}

#[derive(Clone, Serialize)]
struct Bootstrap { request: Request, revision: u64, template: TemplateAssets, maximized: bool }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateParams { request: Request, #[serde(default)] template: Option<PathBuf> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdParams { request_id: Uuid }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateParams { request_id: Uuid, expected_revision: u64, content: Content }

pub fn run() -> i32 {
    let state = Arc::new(HostState::default());
    let managed = Arc::clone(&state);
    let app = host_builder(managed)
        .setup(move |app| {
            coordinator::start(app.handle().clone(), Arc::clone(&state));
            start_listener(app.handle().clone(), Arc::clone(&state))?;
            build_tray(app.handle())?;
            start_tray_animation(app.handle().clone(), Arc::clone(&state));
            schedule_history(app.handle(), false);
            Ok(())
        })
        .build(tauri::generate_context!());
    match app {
        Ok(app) => {
            app.run(|app, event| {
                if let RunEvent::ExitRequested { api, .. } = event {
                    let state = app.state::<Arc<HostState>>();
                    if !state.stopping.load(Ordering::SeqCst) { api.prevent_exit(); }
                }
            });
            0
        }
        Err(error) => { eprintln!("Popu Host startup error: {error}"); runtime::EXIT_ERROR }
    }
}

fn host_builder(state: Arc<HostState>) -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default().manage(state)
        .invoke_handler(tauri::generate_handler![get_bootstrap, submit, finish, render_failed, ui_ready, history_presented, history_trace, history_list, history_get, history_clear, history_mark_read])
        .on_window_event(handle_window_event)
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "history", "Open history", true, None::<&str>)?;
    let clear_current = MenuItem::with_id(app, "clear_current", "Clear current notifications", true, None::<&str>)?;
    let clear_history_item = MenuItem::with_id(app, "clear_history", "Clear history", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit Popu Host", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &clear_current, &clear_history_item, &exit])?;
    let tray = TrayIconBuilder::with_id("popu").menu(&menu).show_menu_on_left_click(false)
        .tooltip("Popu").icon(tray::frame(tray::IDLE_FRAME));
    tray.on_menu_event(|app, event| match event.id().as_ref() {
        "history" => open_history(app),
        "clear_current" => clear_current_notifications(app),
        "clear_history" => {
            let state = app.state::<Arc<HostState>>();
            if clear_history(state.inner()).is_ok() {
                emit_history_changed(app);
                update_tray_tooltip(app, state.inner());
            }
        },
        "exit" => stop_from_tray(app),
        _ => {}
    }).on_tray_icon_event(|tray, event| {
        if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) { open_history(tray.app_handle()); }
    }).build(app)?;
    let state = app.state::<Arc<HostState>>();
    update_tray_tooltip(app, state.inner());
    Ok(())
}

fn open_history(app: &AppHandle) {
    schedule_history(app, true);
}

fn schedule_history(app: &AppHandle, show: bool) {
    let state = app.state::<Arc<HostState>>();
    // Share the notification worker: never build a WebView inside a tray/event
    // loop callback, or concurrently with another window builder.
    let _ = coordinator::send(&state, Task::History { show });
}

fn show_history(window: &WebviewWindow) -> tauri::Result<()> {
    window.unminimize()?;
    window.show()?;
    window.set_focus()?;
    // The outer HWND and the WebView2 controller have separate focus. Without
    // this, document.hasFocus() can stay false after showing a hidden WebView.
    tauri::Webview::set_focus(window.as_ref())?;
    window.emit("history-opened", ())
}

fn open_history_on_worker(app: &AppHandle, state: &Arc<HostState>, show: bool) {
    let result = if let Some(window) = app.get_webview_window("history") {
        let ready = {
            let mut session = state.history_window.lock().unwrap();
            session.as_mut().is_some_and(|session| { session.open_requested |= show; session.ready })
        };
        if ready && show { show_history(&window) } else { Ok(()) }
    } else {
        let started = Instant::now();
        *state.history_window.lock().unwrap() = Some(HistoryWindowSession { started, ready: false, detail_ready: false, open_requested: show });
        eprintln!("Popu history: create-start 0ms");
        WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history.html".into()))
            .title("Popu notification history").inner_size(960.0, 620.0).min_inner_size(640.0, 420.0).resizable(true)
            .visible(false).focused(false)
            .on_page_load(move |_, payload| {
                eprintln!("Popu history: navigation-{:?} {}ms", payload.event(), started.elapsed().as_millis());
            })
            .build().map(|_| { eprintln!("Popu history: create-finished {}ms", started.elapsed().as_millis()); })
    };
    if let Err(error) = result { eprintln!("Popu could not open history: {error}"); }
}

fn clear_current_notifications(app: &AppHandle) {
    let state = app.state::<Arc<HostState>>();
    let (reply, _) = mpsc::channel();
    let _ = coordinator::send(&state, Task::Command(Command::Clear, reply));
}

fn stop_from_tray(app: &AppHandle) {
    let state = app.state::<Arc<HostState>>();
    let (reply, _) = mpsc::channel();
    let _ = coordinator::send(&state, Task::Command(Command::Stop, reply));
}

fn start_listener(app: AppHandle, state: Arc<HostState>) -> Result<(), Box<dyn std::error::Error>> {
    let listener = ipc::bind()?;
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            if state.stopping.load(Ordering::SeqCst) { break; }
            match connection {
                Ok(stream) => {
                    let app = app.clone(); let state = Arc::clone(&state);
                    std::thread::spawn(move || handle_connection(stream, app, state));
                }
                Err(error) => eprintln!("Popu IPC accept error: {error}"),
            }
        }
    });
    Ok(())
}

fn handle_connection(mut stream: interprocess::local_socket::Stream, app: AppHandle, state: Arc<HostState>) {
    let response = match ipc::read_json::<_, RpcRequest>(&mut stream) {
        Ok(request) => dispatch(request, &app, &state),
        Err(error) => RpcResponse::failure(json!({"code":"invalid_request","message":error.to_string()})).unwrap(),
    };
    let _ = ipc::write_frame(&mut stream, &response);
}

fn dispatch(rpc: RpcRequest, app: &AppHandle, state: &Arc<HostState>) -> RpcResponse {
    if rpc.version != RPC_VERSION {
        return failure("unsupported_version", format!("unsupported RPC version: {}", rpc.version));
    }
    let result: Result<Value, Value> = (|| match rpc.method.as_str() {
        "request.create" => {
            let mut params: CreateParams = serde_json::from_value(rpc.params).map_err(invalid)?;
            params.request.validate().map_err(invalid)?;
            params.template = params.template.map(|path| path.canonicalize()).transpose().map_err(invalid)?;
            if let Some(path) = params.template.as_deref() { templates::resolve(&params.request, Some(path)).map_err(invalid)?; }
            coordinator::call(state, Command::Create(params))
        }
        "request.update" => {
            let params: UpdateParams = serde_json::from_value(rpc.params).map_err(invalid)?;
            coordinator::call(state, Command::Update(params))
        }
        "request.dismiss" => {
            let params: IdParams = serde_json::from_value(rpc.params).map_err(invalid)?;
            coordinator::call(state, Command::Dismiss(params.request_id))
        }
        "request.get" => {
            let params: IdParams = serde_json::from_value(rpc.params).map_err(invalid)?;
            let snapshot = state.registry.lock().unwrap().snapshot(params.request_id).map_err(|e| serde_json::to_value(e).unwrap())?;
            serde_json::to_value(snapshot).map_err(invalid)
        }
        "request.wait" => {
            let params: IdParams = serde_json::from_value(rpc.params).map_err(invalid)?;
            let mut registry = state.registry.lock().unwrap();
            loop {
                let snapshot = registry.snapshot(params.request_id).map_err(|e| serde_json::to_value(e).unwrap())?;
                if snapshot.state.is_terminal() { break serde_json::to_value(snapshot).map_err(invalid); }
                registry = state.changed.wait(registry).unwrap();
            }
        }
        "host.status" => coordinator::call(state, Command::Status),
        "history.list" => {
            let history = state.history.lock().unwrap();
            let items = history.as_ref().ok_or_else(|| json!({"code":"history_unavailable"}))?.list().map_err(invalid)?;
            serde_json::to_value(items).map_err(invalid)
        }
        "history.get" => {
            let params: IdParams = serde_json::from_value(rpc.params).map_err(invalid)?;
            let history = state.history.lock().unwrap();
            let item = history.as_ref().ok_or_else(|| json!({"code":"history_unavailable"}))?
                .get(&params.request_id.to_string()).map_err(invalid)?;
            serde_json::to_value(item).map_err(invalid)
        }
        "history.clear" => {
            let count = clear_history(state).map_err(|error| json!({"code":"history_unavailable","message":error}))?;
            emit_history_changed(app);
            update_tray_tooltip(app, state);
            Ok(json!({"status":"cleared","count":count}))
        }
        "host.stop" => coordinator::call(state, Command::Stop),
        _ => Err(json!({"code":"method_not_found","message":format!("unknown RPC method: {}", rpc.method)})),
    })();
    match result { Ok(value) => RpcResponse::success(value).unwrap(), Err(error) => RpcResponse { version: RPC_VERSION.into(), ok: false, result: None, error: Some(error) } }
}

fn invalid(error: impl std::fmt::Display) -> Value { json!({"code":"invalid_request","message":error.to_string()}) }
fn failure(code: &str, message: String) -> RpcResponse { RpcResponse::failure(json!({"code":code,"message":message})).unwrap() }

fn update_tray_tooltip(app: &AppHandle, state: &Arc<HostState>) {
    let pending = pending_history_read_count(state);
    if let Some(tray) = app.tray_by_id("popu") {
        let _ = tray.set_tooltip(Some(tray_tooltip(pending)));
    }
}

fn start_tray_animation(app: AppHandle, state: Arc<HostState>) {
    std::thread::spawn(move || {
        let renderer = tray::FrameRenderer::new(app);
        let mut animator = tray::Animator::default();
        let interval = tray::FRAME_INTERVAL;
        let mut deadline = Instant::now() + interval;
        while !state.stopping.load(Ordering::SeqCst) {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            let active = pending_history_read_count(&state) > 0;
            if let Some(frame) = animator.advance(active) { renderer.render(frame); }
            deadline += interval;
            if deadline < Instant::now() { deadline = Instant::now() + interval; }
        }
    });
}

fn id_for(window: &WebviewWindow) -> Result<Uuid, String> { Uuid::parse_str(window.label()).map_err(|_| "invalid request window".into()) }

#[tauri::command]
fn get_bootstrap(window: WebviewWindow, state: State<'_, Arc<HostState>>) -> Result<Bootstrap, String> {
    let id = id_for(&window)?; let registry = state.registry.lock().unwrap();
    let request = registry.request(id).cloned().ok_or("request not found")?;
    let revision = registry.summary(id).map_err(|e| e.to_string())?.revision;
    let template_dir = registry.template_dir(id).map(PathBuf::from);
    drop(registry);
    let template = templates::resolve(&request, template_dir.as_deref()).map_err(|e| e.to_string())?;
    Ok(Bootstrap { maximized: request.window.is_maximized(), request, revision, template })
}

#[tauri::command]
fn history_list(state: State<'_, Arc<HostState>>) -> Result<Value, String> {
    let history = state.history.lock().unwrap(); let store = history.as_ref().ok_or("history unavailable")?;
    serde_json::to_value(store.list().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum HistoryPhase { ShellReady, ListReady, DetailReady, ReadCheck }

fn trace_history(state: &HostState, phase: HistoryPhase, page_ms: f64) {
    if let Some(session) = state.history_window.lock().unwrap().as_mut() {
        if phase == HistoryPhase::DetailReady { session.detail_ready = true; }
        eprintln!("Popu history: {phase:?} {}ms (page {page_ms:.0}ms)", session.started.elapsed().as_millis());
    }
}

#[tauri::command]
async fn history_presented(window: WebviewWindow, state: State<'_, Arc<HostState>>, page_ms: f64) -> Result<bool, String> {
    if window.label() != "history" { return Err("invalid history window".into()); }
    trace_history(&state, HistoryPhase::ShellReady, page_ms);
    let show = {
        let mut session = state.history_window.lock().unwrap();
        let session = session.as_mut().ok_or("history window session unavailable")?;
        session.ready = true;
        session.open_requested
    };
    if show { show_history(&window).map_err(|e| e.to_string())?; }
    Ok(show)
}

#[tauri::command]
async fn history_trace(window: WebviewWindow, state: State<'_, Arc<HostState>>, phase: HistoryPhase, page_ms: f64, visible: Option<bool>, focused: Option<bool>) -> Result<(), String> {
    if window.label() != "history" { return Err("invalid history window".into()); }
    trace_history(&state, phase, page_ms);
    if let (Some(visible), Some(focused)) = (visible, focused) {
        eprintln!("Popu history: read-check DOM visible={visible} focused={focused}; native foreground={}", runtime::is_window_foreground(&window)?);
    }
    Ok(())
}

#[tauri::command]
fn history_mark_read(window: WebviewWindow, app: AppHandle, state: State<'_, Arc<HostState>>, request_id: Uuid) -> Result<Option<i64>, String> {
    if window.label() != "history" { return Err("only the history window may mark notifications read".into()); }
    if !runtime::is_window_foreground(&window)? { return Ok(None); }
    let read_at_ms = {
        let mut history = state.history.lock().unwrap();
        history.as_mut().ok_or("history unavailable")?.mark_read(&request_id.to_string()).map_err(|e| e.to_string())?
    };
    emit_history_changed(&app);
    update_tray_tooltip(&app, state.inner());
    Ok(Some(read_at_ms))
}

#[tauri::command]
fn history_get(state: State<'_, Arc<HostState>>, request_id: Uuid) -> Result<Value, String> {
    let history = state.history.lock().unwrap();
    let item = history.as_ref().ok_or("history unavailable")?.get(&request_id.to_string()).map_err(|e| e.to_string())?;
    serde_json::to_value(item).map_err(|e| e.to_string())
}

#[tauri::command]
fn history_clear(app: AppHandle, state: State<'_, Arc<HostState>>) -> Result<usize, String> {
    let count = clear_history(state.inner())?;
    emit_history_changed(&app);
    update_tray_tooltip(&app, state.inner());
    Ok(count)
}

#[tauri::command]
async fn ui_ready(window: WebviewWindow, state: State<'_, Arc<HostState>>, revision: u64) -> Result<(), String> {
    let id = id_for(&window)?;
    let (reply, receiver) = mpsc::channel();
    coordinator::send(&state, Task::Ready { id, revision, reply })?;
    tauri::async_runtime::spawn_blocking(move || receiver.recv().map_err(|e| e.to_string()))
        .await.map_err(|e| e.to_string())??
}

#[tauri::command]
async fn submit(window: WebviewWindow, state: State<'_, Arc<HostState>>, action: String, values: Map<String, Value>) -> Result<(), String> {
    coordinator::call_async(Arc::clone(state.inner()), Command::Submit { id: id_for(&window)?, action, values }).await
}

#[tauri::command]
async fn finish(window: WebviewWindow, state: State<'_, Arc<HostState>>, status: ResponseStatus, action: Option<String>) -> Result<(), String> {
    coordinator::call_async(Arc::clone(state.inner()), Command::Finish { id: id_for(&window)?, status, action }).await
}

#[tauri::command]
fn render_failed(window: WebviewWindow, state: State<'_, Arc<HostState>>, message: String) {
    if let Ok(id) = id_for(&window) { let _ = coordinator::send(&state, Task::RenderFailed { id, message }); }
}

fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() == "history" {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Err(error) = window.hide() { eprintln!("Popu could not hide history: {error}"); }
        }
        return;
    }
    let Ok(id) = Uuid::parse_str(window.label()) else { return };
    let task = match event {
        WindowEvent::CloseRequested { api, .. } => { api.prevent_close(); Task::Closed(id) }
        WindowEvent::Destroyed => Task::Destroyed(id),
        _ => return,
    };
    let state = window.app_handle().state::<Arc<HostState>>();
    let _ = coordinator::send(&state, task);
}

fn clear_history(state: &Arc<HostState>) -> Result<usize, String> {
    let mut history = state.history.lock().unwrap();
    history.as_mut().ok_or("history unavailable")?.clear().map_err(|error| error.to_string())
}

fn pending_history_read_count(state: &Arc<HostState>) -> usize {
    state.history.lock().unwrap().as_ref()
        .and_then(|history| history.pending_read_count().ok())
        .unwrap_or(0)
}

fn tray_tooltip(pending: usize) -> String {
    match pending {
        0 => "Popu".into(),
        count => format!("Popu — {count} awaiting reading"),
    }
}

fn persist_terminal(app: &AppHandle, state: &Arc<HostState>, id: Uuid) {
    let Some(snapshot) = state.registry.lock().unwrap().snapshot(id).ok() else { return };
    if snapshot.mode != Mode::Notify || !snapshot.state.is_terminal() { return; }
    let mut history_guard = state.history.lock().unwrap();
    let Some(history) = history_guard.as_mut() else { return };
    let result = history.record(&snapshot);
    drop(history_guard);
    match result {
        Ok(()) => { emit_history_changed(app); update_tray_tooltip(app, state); }
        Err(error) => eprintln!("Popu history error: {error}"),
    }
}

fn emit_history_changed(app: &AppHandle) {
    let _ = app.emit_to("history", "history-changed", ());
}
