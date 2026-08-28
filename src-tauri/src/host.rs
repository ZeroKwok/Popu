use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::ListenerExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{window::Color, AppHandle, Manager, PhysicalPosition, RunEvent, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use uuid::Uuid;

use crate::core::{Effect, RequestRegistry};
use crate::ipc::{self, RpcRequest, RpcResponse, RPC_VERSION};
use crate::history::HistoryStore;
use crate::protocol::{Content, Mode, Position, Request, ResponseStatus, Screen};
use crate::runtime;
use crate::templates::{self, TemplateAssets};
use crate::tray;

pub struct HostState {
    registry: Mutex<RequestRegistry>,
    changed: Condvar,
    stopping: AtomicBool,
    history: Mutex<Option<HistoryStore>>,
}

impl Default for HostState {
    fn default() -> Self { Self { registry: Mutex::new(RequestRegistry::default()), changed: Condvar::new(), stopping: AtomicBool::new(false), history: Mutex::new(HistoryStore::open_default().ok()) } }
}

#[derive(Clone, Serialize)]
struct Bootstrap { request: Request, template: TemplateAssets, maximized: bool }

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
    let app = tauri::Builder::default()
        .manage(managed)
        .invoke_handler(tauri::generate_handler![get_bootstrap, submit, finish, render_failed, ui_presented, history_list, history_clear])
        .on_window_event(handle_window_event)
        .setup(move |app| {
            start_listener(app.handle().clone(), Arc::clone(&state))?;
            build_tray(app.handle())?;
            start_timer(app.handle().clone(), Arc::clone(&state));
            start_tray_animation(app.handle().clone(), Arc::clone(&state));
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

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "history", "Open history", true, None::<&str>)?;
    let clear_current = MenuItem::with_id(app, "clear_current", "Clear current notifications", true, None::<&str>)?;
    let clear_history = MenuItem::with_id(app, "clear_history", "Clear history", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit Popu Host", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &clear_current, &clear_history, &exit])?;
    let tray = TrayIconBuilder::with_id("popu").menu(&menu).tooltip("Popu — 0 notifications").icon(tray::frame(tray::IDLE_FRAME));
    tray.on_menu_event(|app, event| match event.id().as_ref() {
        "history" => open_history(app),
        "clear_current" => clear_current_notifications(app),
        "clear_history" => { let state = app.state::<Arc<HostState>>(); if let Some(history) = state.history.lock().unwrap().as_mut() { let _ = history.clear(); }; },
        "exit" => stop_from_tray(app),
        _ => {}
    }).on_tray_icon_event(|tray, event| {
        if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) { open_history(tray.app_handle()); }
    }).build(app)?;
    Ok(())
}

fn open_history(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("history") { let _ = window.show(); let _ = window.set_focus(); return; }
    let _ = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history.html".into()))
        .title("Popu notification history").inner_size(720.0, 520.0).resizable(true).build();
}

fn clear_current_notifications(app: &AppHandle) {
    let state = app.state::<Arc<HostState>>(); let ids = state.registry.lock().unwrap().active_notification_ids();
    let mut effects = vec![];
    for id in ids {
        let result = { state.registry.lock().unwrap().dismiss(id) };
        if let Ok(mut next) = result { effects.append(&mut next); persist_terminal(state.inner(), id); }
    }
    state.changed.notify_all(); schedule(app, state.inner().clone(), effects);
}

fn stop_from_tray(app: &AppHandle) {
    let state = app.state::<Arc<HostState>>(); state.stopping.store(true, Ordering::SeqCst);
    let ids = state.registry.lock().unwrap().shutdown(); for id in ids { persist_terminal(state.inner(), id); }
    state.changed.notify_all(); app.exit(0);
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
            let (created, effects) = state.registry.lock().unwrap().create(params.request, params.template);
            state.changed.notify_all(); schedule(app, Arc::clone(state), effects);
            serde_json::to_value(created).map_err(invalid)
        }
        "request.update" => {
            let params: UpdateParams = serde_json::from_value(rpc.params).map_err(invalid)?;
            let mut registry = state.registry.lock().unwrap();
            let effects = registry.update(params.request_id, params.expected_revision, params.content).map_err(|e| serde_json::to_value(e).unwrap())?;
            let snapshot = registry.snapshot(params.request_id).unwrap(); drop(registry);
            state.changed.notify_all(); schedule(app, Arc::clone(state), effects);
            serde_json::to_value(snapshot).map_err(invalid)
        }
        "request.dismiss" => {
            let params: IdParams = serde_json::from_value(rpc.params).map_err(invalid)?;
            let mut registry = state.registry.lock().unwrap();
            let effects = registry.dismiss(params.request_id).map_err(|e| serde_json::to_value(e).unwrap())?;
            let snapshot = registry.snapshot(params.request_id).unwrap(); drop(registry);
            persist_terminal(state, params.request_id);
            state.changed.notify_all(); schedule(app, Arc::clone(state), effects);
            serde_json::to_value(snapshot).map_err(invalid)
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
        "host.status" => Ok(json!({"status":"running","notifications":state.registry.lock().unwrap().active_notification_count()})),
        "history.list" => {
            let history = state.history.lock().unwrap();
            let items = history.as_ref().ok_or_else(|| json!({"code":"history_unavailable"}))?.list().map_err(invalid)?;
            serde_json::to_value(items).map_err(invalid)
        }
        "history.clear" => {
            let mut history = state.history.lock().unwrap();
            let count = history.as_mut().ok_or_else(|| json!({"code":"history_unavailable"}))?.clear().map_err(invalid)?;
            Ok(json!({"status":"cleared","count":count}))
        }
        "host.stop" => {
            state.stopping.store(true, Ordering::SeqCst);
            let ids = state.registry.lock().unwrap().shutdown(); state.changed.notify_all();
            for id in &ids { persist_terminal(state, *id); }
            let app = app.clone(); std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(30)); let exit_app = app.clone(); let _ = app.run_on_main_thread(move || exit_app.exit(0)); });
            Ok(json!({"status":"stopping","cancelled":ids.len()}))
        }
        _ => Err(json!({"code":"method_not_found","message":format!("unknown RPC method: {}", rpc.method)})),
    })();
    match result { Ok(value) => RpcResponse::success(value).unwrap(), Err(error) => RpcResponse { version: RPC_VERSION.into(), ok: false, result: None, error: Some(error) } }
}

fn invalid(error: impl std::fmt::Display) -> Value { json!({"code":"invalid_request","message":error.to_string()}) }
fn failure(code: &str, message: String) -> RpcResponse { RpcResponse::failure(json!({"code":code,"message":message})).unwrap() }

fn schedule(app: &AppHandle, state: Arc<HostState>, effects: Vec<Effect>) {
    let app = app.clone(); let callback_app = app.clone();
    let _ = app.run_on_main_thread(move || apply_effects(&callback_app, &state, effects));
}

fn apply_effects(app: &AppHandle, state: &Arc<HostState>, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::Show(id) => if let Err(error) = show_window(app, state, id) { eprintln!("Popu window error: {error}"); complete_failed(app, state, id); },
            Effect::Update(id) => if let Some(window) = app.get_webview_window(&id.to_string()) {
                if let Some(content) = state.registry.lock().unwrap().request(id).map(|r| r.content.clone()) {
                    if let Ok(value) = serde_json::to_string(&content) { let _ = window.eval(format!("window.popuApplyUpdate?.({value})")); }
                }
            },
            Effect::Close(id) => if let Some(window) = app.get_webview_window(&id.to_string()) { let _ = window.destroy(); },
        }
    }
    reposition_notifications(app, state);
    update_tray_tooltip(app, state);
}

fn update_tray_tooltip(app: &AppHandle, state: &Arc<HostState>) {
    let count = state.registry.lock().unwrap().active_notification_count();
    if let Some(tray) = app.tray_by_id("popu") {
        let label = if count > 99 { "99+".to_owned() } else { count.to_string() };
        let _ = tray.set_tooltip(Some(format!("Popu — {label} notifications")));
    }
}

fn show_window(app: &AppHandle, state: &Arc<HostState>, id: Uuid) -> Result<(), String> {
    let (mut request, template_dir, slot) = {
        let registry = state.registry.lock().unwrap();
        (registry.request(id).cloned().ok_or("request not found")?, registry.template_dir(id).map(PathBuf::from), registry.notification_slot(id))
    };
    if request.mode == Mode::Notify { request.window.screen = Screen::Primary; request.position = Position::BottomRight; }
    let _template = templates::resolve(&request, template_dir.as_deref()).map_err(|e| e.to_string())?;
    let mut placement = runtime::resolve_window_placement(&request)?;
    if let Some(slot) = slot { placement.position.y -= slot as i32 * (placement.size.1 as i32 + 12); }
    let notify = request.mode == Mode::Notify;
    let window = WebviewWindowBuilder::new(app, id.to_string(), WebviewUrl::App("index.html".into()))
        .title(&request.content.title).inner_size(placement.size.0 as f64, placement.size.1 as f64)
        .min_inner_size(1.0, 1.0).decorations(false).transparent(true).resizable(false)
        .background_color(Color(0, 0, 0, 0))
        .always_on_top(request.window.always_on_top).skip_taskbar(notify).focused(!notify).visible(false).shadow(false)
        .build().map_err(|e| e.to_string())?;
    runtime::position_window(&window, placement)?;
    #[cfg(windows)] runtime::configure_frameless_windows_window(&window, request.window.is_maximized())?;
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

fn reposition_notifications(app: &AppHandle, state: &Arc<HostState>) {
    let mut items: Vec<_> = app.webview_windows().into_iter().filter_map(|(label, window)| {
        let id = Uuid::parse_str(&label).ok()?; let registry = state.registry.lock().unwrap();
        let request = registry.request(id)?.clone(); let slot = registry.notification_slot(id)?; (request.mode == Mode::Notify).then_some((window, request, slot))
    }).collect();
    items.sort_by_key(|(_, _, slot)| *slot);
    let mut offset = 0_i32;
    for (window, mut request, slot) in items {
        request.window.screen = Screen::Primary; request.position = Position::BottomRight;
        let _ = slot;
        if let Ok(mut placement) = runtime::resolve_window_placement(&request) { placement.position.y -= offset; offset += placement.size.1 as i32 + 12; let _ = window.set_position(PhysicalPosition::new(placement.position.x, placement.position.y)); }
    }
}

fn start_timer(app: AppHandle, state: Arc<HostState>) {
    std::thread::spawn(move || { let mut ticks = 0_u8; while !state.stopping.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
        let effects = { state.registry.lock().unwrap().expire_due(Instant::now()) };
        if !effects.is_empty() { for effect in &effects { if let Effect::Close(id) = effect { persist_terminal(&state, *id); } } state.changed.notify_all(); schedule(&app, Arc::clone(&state), effects); }
        ticks = ticks.wrapping_add(1);
        if ticks == 10 { ticks = 0; schedule(&app, Arc::clone(&state), vec![]); }
    }});
}

fn start_tray_animation(app: AppHandle, state: Arc<HostState>) {
    std::thread::spawn(move || {
        let renderer = tray::FrameRenderer::new(app);
        let mut animator = tray::Animator::default();
        let interval = tray::FRAME_INTERVAL;
        let mut deadline = Instant::now() + interval;
        while !state.stopping.load(Ordering::SeqCst) {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            let active = state.registry.lock().unwrap().active_notification_count() > 0;
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
    let template = templates::resolve(&request, registry.template_dir(id)).map_err(|e| e.to_string())?;
    Ok(Bootstrap { maximized: request.window.is_maximized(), request, template })
}

#[tauri::command]
fn history_list(state: State<'_, Arc<HostState>>) -> Result<Value, String> {
    let history = state.history.lock().unwrap(); let store = history.as_ref().ok_or("history unavailable")?;
    serde_json::to_value(store.list().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[tauri::command]
fn history_clear(state: State<'_, Arc<HostState>>) -> Result<usize, String> {
    let mut history = state.history.lock().unwrap(); history.as_mut().ok_or("history unavailable")?.clear().map_err(|e| e.to_string())
}

#[tauri::command]
fn ui_presented(window: WebviewWindow, state: State<'_, Arc<HostState>>) -> Result<(), String> {
    let id = id_for(&window)?;
    runtime::reveal_windows_window(&window)?;
    let mode = {
        let mut registry = state.registry.lock().unwrap();
        registry.mark_shown(id, Instant::now()).map_err(|e| e.to_string())?;
        registry.request(id).map(|request| request.mode).ok_or("request not found")?
    };
    if mode == Mode::Dialog { window.set_focus().map_err(|e| e.to_string())?; }
    state.changed.notify_all(); Ok(())
}

#[tauri::command]
fn submit(window: WebviewWindow, app: AppHandle, state: State<'_, Arc<HostState>>, action: String, values: Map<String, Value>) -> Result<(), String> {
    let id = id_for(&window)?; let mut registry = state.registry.lock().unwrap();
    let request = registry.request(id).cloned().ok_or("request not found")?;
    let response = request.submitted_response(&action, values).map_err(|e| e.to_string())?;
    let code = request.actions.iter().find(|a| a.id == action).and_then(|a| a.exit_code).map(i32::from).unwrap_or(0);
    let effects = registry.complete(id, response, code).map_err(|e| e.to_string())?; drop(registry);
    persist_terminal(state.inner(), id);
    state.changed.notify_all(); schedule(&app, state.inner().clone(), effects); Ok(())
}

#[tauri::command]
fn finish(window: WebviewWindow, app: AppHandle, state: State<'_, Arc<HostState>>, status: ResponseStatus, action: Option<String>) -> Result<(), String> {
    let id = id_for(&window)?; let mut registry = state.registry.lock().unwrap(); let request = registry.request(id).cloned().ok_or("request not found")?;
    let response = request.terminal_response(status, action.clone());
    let code = action.as_deref().and_then(|id| request.actions.iter().find(|a| a.id == id)).and_then(|a| a.exit_code).map(i32::from)
        .unwrap_or(match status { ResponseStatus::Submitted | ResponseStatus::Confirmed => 0, ResponseStatus::TimedOut => 11, _ => 10 });
    let effects = registry.complete(id, response, code).map_err(|e| e.to_string())?; drop(registry);
    persist_terminal(state.inner(), id);
    state.changed.notify_all(); schedule(&app, state.inner().clone(), effects); Ok(())
}

#[tauri::command]
fn render_failed(window: WebviewWindow, app: AppHandle, state: State<'_, Arc<HostState>>, message: String) {
    eprintln!("Popu renderer failed: {message}"); if let Ok(id) = id_for(&window) { complete_failed(&app, state.inner(), id); }
}

fn complete_failed(app: &AppHandle, state: &Arc<HostState>, id: Uuid) {
    let result = { state.registry.lock().unwrap().fail(id) };
    if let Ok(effects) = result { persist_terminal(state, id); state.changed.notify_all(); schedule(app, Arc::clone(state), effects); }
}

fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if !matches!(event, WindowEvent::CloseRequested { .. }) { return; }
    let Ok(id) = Uuid::parse_str(window.label()) else { return }; let app = window.app_handle(); let state = app.state::<Arc<HostState>>();
    let result = { let mut registry = state.registry.lock().unwrap();
        let Some(request) = registry.request(id).cloned() else { return };
        if request.mode == Mode::Dialog { registry.complete(id, request.terminal_response(ResponseStatus::Closed, None), 10) } else { registry.dismiss(id) }
    };
    if let Ok(effects) = result { persist_terminal(state.inner(), id); state.changed.notify_all(); schedule(app, state.inner().clone(), effects); }
}

fn persist_terminal(state: &Arc<HostState>, id: Uuid) {
    let snapshot = state.registry.lock().unwrap().snapshot(id).ok();
    if let (Some(snapshot), Some(history)) = (snapshot, state.history.lock().unwrap().as_mut()) {
        if let Err(error) = history.record(&snapshot) { eprintln!("Popu history error: {error}"); }
    }
}
