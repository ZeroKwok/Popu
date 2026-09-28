use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::ListenerExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{window::Color, AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use uuid::Uuid;

use crate::core::{Effect, RequestRegistry};
use crate::ipc::{self, RpcRequest, RpcResponse, RPC_VERSION};
use crate::history::HistoryStore;
use crate::protocol::{Content, Mode, Position, Request, ResponseStatus, Screen};
use crate::{placement, runtime};
use crate::templates::{self, TemplateAssets};
use crate::tray;

pub struct HostState {
    registry: Mutex<RequestRegistry>,
    changed: Condvar,
    stopping: AtomicBool,
    history: Mutex<Option<HistoryStore>>,
    effect_sender: Mutex<Option<Sender<Vec<Effect>>>>,
}

impl Default for HostState {
    fn default() -> Self { Self { registry: Mutex::new(RequestRegistry::default()), changed: Condvar::new(), stopping: AtomicBool::new(false), history: Mutex::new(HistoryStore::open_default().ok()), effect_sender: Mutex::new(None) } }
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
            start_effect_worker(app.handle().clone(), Arc::clone(&state));
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
    let clear_history_item = MenuItem::with_id(app, "clear_history", "Clear history", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit Popu Host", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &clear_current, &clear_history_item, &exit])?;
    let tray = TrayIconBuilder::with_id("popu").menu(&menu).tooltip("Popu — 0 notifications").icon(tray::frame(tray::IDLE_FRAME));
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
    let opened = if let Some(window) = app.get_webview_window("history") {
        if window.show().is_err() { false } else { let _ = window.set_focus(); true }
    } else {
        WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history.html".into()))
            .title("Popu notification history").inner_size(960.0, 620.0).min_inner_size(640.0, 420.0).resizable(true).build().is_ok()
    };
    if opened { acknowledge_history(app); }
}

fn clear_current_notifications(app: &AppHandle) {
    let state = app.state::<Arc<HostState>>(); let ids = state.registry.lock().unwrap().active_notification_ids();
    let mut effects = vec![];
    for id in ids {
        let result = { state.registry.lock().unwrap().dismiss(id) };
        if let Ok(mut next) = result { effects.append(&mut next); persist_terminal(app, state.inner(), id); }
    }
    state.changed.notify_all(); schedule(app, state.inner().clone(), effects);
}

fn stop_from_tray(app: &AppHandle) {
    let state = app.state::<Arc<HostState>>(); state.stopping.store(true, Ordering::SeqCst);
    let ids = state.registry.lock().unwrap().shutdown(); for id in ids { persist_terminal(app, state.inner(), id); }
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
            persist_terminal(app, state, params.request_id);
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
            let count = clear_history(state).map_err(|error| json!({"code":"history_unavailable","message":error}))?;
            emit_history_changed(app);
            update_tray_tooltip(app, state);
            Ok(json!({"status":"cleared","count":count}))
        }
        "host.stop" => {
            state.stopping.store(true, Ordering::SeqCst);
            let ids = state.registry.lock().unwrap().shutdown(); state.changed.notify_all();
            for id in &ids { persist_terminal(app, state, *id); }
            let app = app.clone(); std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(30)); let exit_app = app.clone(); let _ = app.run_on_main_thread(move || exit_app.exit(0)); });
            Ok(json!({"status":"stopping","cancelled":ids.len()}))
        }
        _ => Err(json!({"code":"method_not_found","message":format!("unknown RPC method: {}", rpc.method)})),
    })();
    match result { Ok(value) => RpcResponse::success(value).unwrap(), Err(error) => RpcResponse { version: RPC_VERSION.into(), ok: false, result: None, error: Some(error) } }
}

fn invalid(error: impl std::fmt::Display) -> Value { json!({"code":"invalid_request","message":error.to_string()}) }
fn failure(code: &str, message: String) -> RpcResponse { RpcResponse::failure(json!({"code":code,"message":message})).unwrap() }

fn start_effect_worker(app: AppHandle, state: Arc<HostState>) {
    let (sender, receiver) = mpsc::channel();
    *state.effect_sender.lock().unwrap() = Some(sender);
    std::thread::spawn(move || {
        while let Ok(effects) = receiver.recv() {
            apply_effects(&app, &state, effects);
        }
    });
}

fn schedule(_app: &AppHandle, state: Arc<HostState>, effects: Vec<Effect>) {
    if effects.is_empty() { return; }
    let sender = state.effect_sender.lock().unwrap().clone();
    if let Some(sender) = sender {
        if sender.send(effects).is_err() { eprintln!("Popu effect worker stopped unexpectedly"); }
    } else {
        eprintln!("Popu effect worker is not ready");
    }
}

fn apply_effects(app: &AppHandle, state: &Arc<HostState>, effects: Vec<Effect>) {
    // WebView2 can synchronously pump messages while a window is being built.
    // Running builders or destroys in an app.run_on_main_thread callback can
    // therefore re-enter Wry while its WebContextStore mutex is held. Keep a
    // single FIFO worker off the UI thread; Tauri dispatches each native
    // operation to its event loop without blocking that loop re-entrantly.
    // New notifications are positioned as they are created. Reflow only after
    // a notification leaves the stack, so a user's drag is not overwritten by
    // timer ticks, content updates, or unrelated dialog activity.
    let reflow_notifications = {
        let registry = state.registry.lock().unwrap();
        effects.iter().any(|effect| match effect {
            Effect::Close(id) => registry.request(*id).is_some_and(|request| request.mode == Mode::Notify),
            Effect::Show(_) | Effect::Update(_) => false,
        })
    };
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
    if reflow_notifications { reposition_notifications(app, state); }
    update_tray_tooltip(app, state);
}

fn update_tray_tooltip(app: &AppHandle, state: &Arc<HostState>) {
    let (count, attention) = tray_status(state);
    if let Some(tray) = app.tray_by_id("popu") {
        let _ = tray.set_tooltip(Some(tray_tooltip_from_status(count, attention)));
    }
}

fn show_window(app: &AppHandle, state: &Arc<HostState>, id: Uuid) -> Result<(), String> {
    let (mut request, template_dir, preceding_notifications) = {
        let registry = state.registry.lock().unwrap();
        let preceding_notifications: Vec<Request> = registry.notification_slot(id).map(|slot| {
            registry.active_notification_ids().into_iter().take(slot)
                .filter_map(|previous_id| registry.request(previous_id).cloned())
                .collect()
        }).unwrap_or_default();
        (registry.request(id).cloned().ok_or("request not found")?, registry.template_dir(id).map(PathBuf::from), preceding_notifications)
    };
    if request.mode == Mode::Notify { request.window.screen = Screen::Primary; request.position = Position::BottomRight; }
    let _template = templates::resolve(&request, template_dir.as_deref()).map_err(|e| e.to_string())?;
    let mut placement = runtime::resolve_window_placement(&request)?;
    if request.mode == Mode::Notify {
        let preceding_heights = preceding_notifications.into_iter().map(|mut notification| {
            notification.window.screen = Screen::Primary;
            notification.position = Position::BottomRight;
            runtime::resolve_window_placement(&notification).map(|placement| placement.size.1)
        }).collect::<Result<Vec<_>, _>>()?;
        placement.position.y = placement.position.y.saturating_sub(placement::notification_stack_offset(preceding_heights));
    }
    let notify = request.mode == Mode::Notify;
    let window = WebviewWindowBuilder::new(app, id.to_string(), WebviewUrl::App("index.html".into()))
        // Move the hidden window to the target monitor before setting its
        // logical size, so Tauri applies that monitor's DPI scale factor.
        .title(&request.content.title).inner_size(1.0, 1.0)
        .min_inner_size(1.0, 1.0).decorations(false).transparent(true).resizable(false)
        .background_color(Color(0, 0, 0, 0))
        .always_on_top(request.window.always_on_top).skip_taskbar(notify).focused(!notify).visible(false).shadow(false)
        .build().map_err(|e| e.to_string())?;
    runtime::position_window(&window, placement)?;
    runtime::size_window(&window, placement)?;
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
        if let Ok(mut placement) = runtime::resolve_window_placement(&request) { placement.position.y = placement.position.y.saturating_sub(offset); offset = offset.saturating_add(placement.size.1 as i32).saturating_add(placement::NOTIFICATION_STACK_GAP as i32); let _ = window.set_position(PhysicalPosition::new(placement.position.x, placement.position.y)); }
    }
}

fn start_timer(app: AppHandle, state: Arc<HostState>) {
    std::thread::spawn(move || { while !state.stopping.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
        let effects = { state.registry.lock().unwrap().expire_due(Instant::now()) };
        if !effects.is_empty() { for effect in &effects { if let Effect::Close(id) = effect { persist_terminal(&app, &state, *id); } } state.changed.notify_all(); schedule(&app, Arc::clone(&state), effects); }
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
            let (notifications, attention) = tray_status(&state);
            let active = notifications > 0 || attention;
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
fn history_clear(app: AppHandle, state: State<'_, Arc<HostState>>) -> Result<usize, String> {
    let count = clear_history(state.inner())?;
    emit_history_changed(&app);
    update_tray_tooltip(&app, state.inner());
    Ok(count)
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
    persist_terminal(&app, state.inner(), id);
    state.changed.notify_all(); schedule(&app, state.inner().clone(), effects); Ok(())
}

#[tauri::command]
fn finish(window: WebviewWindow, app: AppHandle, state: State<'_, Arc<HostState>>, status: ResponseStatus, action: Option<String>) -> Result<(), String> {
    let id = id_for(&window)?; let mut registry = state.registry.lock().unwrap(); let request = registry.request(id).cloned().ok_or("request not found")?;
    let response = request.terminal_response(status, action.clone());
    let code = action.as_deref().and_then(|id| request.actions.iter().find(|a| a.id == id)).and_then(|a| a.exit_code).map(i32::from)
        .unwrap_or(match status { ResponseStatus::Submitted | ResponseStatus::Confirmed => 0, ResponseStatus::TimedOut => 11, _ => 10 });
    let effects = registry.complete(id, response, code).map_err(|e| e.to_string())?; drop(registry);
    persist_terminal(&app, state.inner(), id);
    state.changed.notify_all(); schedule(&app, state.inner().clone(), effects); Ok(())
}

#[tauri::command]
fn render_failed(window: WebviewWindow, app: AppHandle, state: State<'_, Arc<HostState>>, message: String) {
    eprintln!("Popu renderer failed: {message}"); if let Ok(id) = id_for(&window) { complete_failed(&app, state.inner(), id); }
}

fn complete_failed(app: &AppHandle, state: &Arc<HostState>, id: Uuid) {
    let result = { state.registry.lock().unwrap().fail(id) };
    if let Ok(effects) = result { persist_terminal(app, state, id); state.changed.notify_all(); schedule(app, Arc::clone(state), effects); }
}

fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    if window.label() == "history" && matches!(event, WindowEvent::Focused(true)) {
        acknowledge_history(window.app_handle());
        return;
    }
    if !matches!(event, WindowEvent::CloseRequested { .. }) { return; }
    let Ok(id) = Uuid::parse_str(window.label()) else { return }; let app = window.app_handle(); let state = app.state::<Arc<HostState>>();
    let result = { let mut registry = state.registry.lock().unwrap();
        let Some(request) = registry.request(id).cloned() else { return };
        if request.mode == Mode::Dialog { registry.complete(id, request.terminal_response(ResponseStatus::Closed, None), 10) } else { registry.dismiss(id) }
    };
    if let Ok(effects) = result { persist_terminal(app, state.inner(), id); state.changed.notify_all(); schedule(app, state.inner().clone(), effects); }
}

fn clear_history(state: &Arc<HostState>) -> Result<usize, String> {
    let mut history = state.history.lock().unwrap();
    history.as_mut().ok_or("history unavailable")?.clear().map_err(|error| error.to_string())
}

fn acknowledge_history(app: &AppHandle) {
    let state = app.state::<Arc<HostState>>();
    let acknowledged = state.history.lock().unwrap().as_mut()
        .map(|history| history.acknowledge_attention())
        .transpose();
    match acknowledged {
        Ok(Some(true)) => update_tray_tooltip(app, state.inner()),
        Ok(_) => {}
        Err(error) => eprintln!("Popu history error: {error}"),
    }
}

fn tray_status(state: &Arc<HostState>) -> (usize, bool) {
    let notifications = state.registry.lock().unwrap().active_notification_count();
    let attention = state.history.lock().unwrap().as_ref()
        .and_then(|history| history.needs_attention().ok())
        .unwrap_or(false);
    (notifications, attention)
}

fn tray_tooltip_from_status(notifications: usize, attention: bool) -> String {
    match (notifications, attention) {
        (0, false) => "Popu — no active notifications".into(),
        (0, true) => "Popu — notification history needs review".into(),
        (count, false) => format!("Popu — {count} active notifications"),
        (count, true) => format!("Popu — {count} active notifications; history needs review"),
    }
}

fn persist_terminal(app: &AppHandle, state: &Arc<HostState>, id: Uuid) {
    let Some(snapshot) = state.registry.lock().unwrap().snapshot(id).ok() else { return };
    if snapshot.mode != Mode::Notify || !snapshot.state.is_terminal() { return; }
    let mut history_guard = state.history.lock().unwrap();
    let Some(history) = history_guard.as_mut() else { return };
    match history.record(&snapshot) {
        Ok(()) => emit_history_changed(app),
        Err(error) => eprintln!("Popu history error: {error}"),
    }
}

fn emit_history_changed(app: &AppHandle) {
    let _ = app.emit_to("history", "history-changed", ());
}
