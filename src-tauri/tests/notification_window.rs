// Real production coordinator and JSON RPC over an isolated named pipe.
// Never uses the running user's Host, tray, endpoint, or history database.
#![cfg(windows)]
#![allow(dead_code)]

pub use popu_lib::{core, ipc, notifications, placement, protocol, templates};
#[path = "../src/history.rs"]
mod history;
#[path = "../src/runtime.rs"]
mod runtime;
#[path = "../src/tray.rs"]
mod tray;

mod host {
    include!("../src/host.rs");

    mod native_tests {
        use super::{
            coordinator, handle_connection, host_builder, id_for, ipc, json, mpsc, runtime,
            windows, AppHandle, Arc, AtomicBool, Duration, HistoryStore, HostState, Instant,
            Manager, Ordering, RunEvent, State, Task, Uuid, WebviewWindow,
        };
        use interprocess::local_socket::traits::ListenerExt;
        use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions, Stream};
        use serde_json::Value;

        #[derive(Default)]
        struct Fault {
            intercepted: AtomicBool,
        }

        #[tauri::command]
        async fn ui_ready(
            window: WebviewWindow,
            state: State<'_, Arc<HostState>>,
            fault: State<'_, Fault>,
            revision: u64,
        ) -> Result<(), String> {
            let request = state
                .registry
                .lock()
                .unwrap()
                .request(id_for(&window)?)
                .cloned()
                .unwrap();
            if request.content.title == "Stalled head" {
                fault.intercepted.store(true, Ordering::SeqCst);
                return Ok(());
            }
            super::ui_ready(window, state, revision).await
        }

        fn rpc(endpoint: &str, method: &str, params: Value) -> Result<Value, String> {
            let mut stream = Stream::connect(
                endpoint
                    .to_ns_name::<GenericNamespaced>()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            ipc::write_frame(
                &mut stream,
                &ipc::RpcRequest {
                    version: ipc::RPC_VERSION.into(),
                    method: method.into(),
                    params,
                },
            )
            .map_err(|e| e.to_string())?;
            let response: ipc::RpcResponse =
                ipc::read_json(&mut stream).map_err(|e| e.to_string())?;
            if response.ok {
                Ok(response.result.unwrap())
            } else {
                Err(response.error.unwrap().to_string())
            }
        }

        fn request(
            endpoint: &str,
            title: &str,
            height: u32,
            duration: u64,
        ) -> Result<Uuid, String> {
            create(
                endpoint,
                json!({"version":"1.0","mode":"notify","template":"notification","requires_read":true,
                "duration_ms":duration,"window":{"height":height},"content":{"title":title}}),
            )
        }

        fn create(endpoint: &str, request: Value) -> Result<Uuid, String> {
            let result = rpc(endpoint, "request.create", json!({"request":request}))?;
            Uuid::parse_str(result["request_id"].as_str().unwrap()).map_err(|e| e.to_string())
        }

        fn wait(label: &str, mut predicate: impl FnMut() -> bool) -> Result<(), String> {
            let deadline = Instant::now() + Duration::from_secs(25);
            while Instant::now() < deadline {
                if predicate() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(format!("timed out waiting for {label}"))
        }

        fn shown(state: &HostState, id: Uuid) -> bool {
            state.registry.lock().unwrap().is_presented(id)
        }

        fn bottom(app: &AppHandle, id: Uuid) -> bool {
            let Ok(display) = runtime::notification_display() else {
                return false;
            };
            let Some(window) = app.get_webview_window(&id.to_string()) else {
                return false;
            };
            let Ok(native) = windows::observe(&window) else {
                return false;
            };
            let margin = (16.0 * display.scale).round() as u32;
            native.healthy()
                && native.position.0
                    == display.area.x
                        + (display.area.width
                            - native.size.0
                            - margin.min(display.area.width - native.size.0))
                            as i32
                && native.position.1
                    == display.area.y
                        + (display.area.height
                            - native.size.1
                            - margin.min(display.area.height - native.size.1))
                            as i32
        }

        fn assert_stack(app: &AppHandle, state: &HostState) -> Result<(), String> {
            let ids = state.registry.lock().unwrap().visible_notification_ids();
            if ids.is_empty() {
                return Err("expected a visible stack".into());
            }
            if !bottom(app, ids[0]) {
                return Err("bottom notification left a gap".into());
            }
            let display = runtime::notification_display()?;
            let gap = (12.0 * display.scale).round() as i32;
            let mut previous = None;
            for id in ids {
                let window = app
                    .get_webview_window(&id.to_string())
                    .ok_or("missing native window")?;
                let native = windows::observe(&window)?;
                if !native.healthy() {
                    return Err(format!("unhealthy visible window {id}: {native:?}"));
                }
                if let Some(previous_top) = previous {
                    if native.position.1 + native.size.1 as i32 + gap != previous_top {
                        return Err("stack contains a gap or overlap".into());
                    }
                }
                previous = Some(native.position.1);
            }
            Ok(())
        }

        fn clear(endpoint: &str, state: &HostState, app: &AppHandle) -> Result<(), String> {
            // Exercise the same coordinator command as the tray, without invoking
            // the user's actual tray. Each dismiss travels through the named pipe.
            let ids = state.registry.lock().unwrap().active_notification_ids();
            for id in ids {
                let _ = rpc(endpoint, "request.dismiss", json!({"request_id":id}));
            }
            wait("empty native and logical notification sets", || {
                state.registry.lock().unwrap().active_notification_count() == 0
                    && app
                        .webview_windows()
                        .values()
                        .all(|w| w.label() == "history")
            })
        }

        #[test]
        #[ignore = "opens real WebView2 windows; requires an unlocked Windows desktop"]
        fn notification_window_lifecycle() {
            let state = Arc::new(HostState::with_history(Some(
                HistoryStore::from_connection(rusqlite::Connection::open_in_memory().unwrap())
                    .unwrap(),
            )));
            let endpoint = format!("popu-notification-test-{}", Uuid::new_v4());
            let listener = ListenerOptions::new()
                .name(endpoint.as_str().to_ns_name::<GenericNamespaced>().unwrap())
                .create_sync()
                .unwrap();
            let (done, result) = mpsc::channel();
            let app = host_builder(Arc::clone(&state)).any_thread().manage(Fault::default())
                .invoke_handler(tauri::generate_handler![super::get_bootstrap, self::ui_ready, super::finish, super::render_failed])
                .setup(move |app| {
                    let app = app.handle().clone();
                    coordinator::start(app.clone(), Arc::clone(&state));
                    let pipe_app = app.clone(); let pipe_state = Arc::clone(&state);
                    std::thread::spawn(move || {
                        for stream in listener.incoming() {
                            let Ok(stream) = stream else { break };
                            let app = pipe_app.clone(); let state = Arc::clone(&pipe_state);
                            std::thread::spawn(move || handle_connection(stream, app, state));
                        }
                    });
                    std::thread::spawn(move || {
                        let result = (|| -> Result<(), String> {
                            // The second request must wait for the head's actual
                            // preparation timeout, not be inserted below a hidden slot.
                            let stalled = create(&endpoint, json!({"version":"1.0","mode":"notify","template":"notification","updatable":true,"duration_ms":60000,"window":{"height":180},"content":{"title":"Stalled head"}}))?;
                            wait("intercepted content readiness", || app.state::<Fault>().intercepted.load(Ordering::SeqCst))?;
                            rpc(&endpoint, "request.update", json!({"request_id":stalled,"expected_revision":1,"content":{"title":"Stalled head","blocks":[{"type":"paragraph","text":"Updated before presentation"}]}}))?;
                            let (reply, receiver) = mpsc::channel();
                            coordinator::send(&state, Task::Ready { id: stalled, revision: 1, reply })?;
                            receiver.recv_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())??;
                            if shown(&state, stalled) { return Err("old revision revealed replacement content".into()); }
                            let successor = request(&endpoint, "After stalled head", 180, 60000)?;
                            if shown(&state, successor) { return Err("successor bypassed the preparing FIFO head".into()); }
                            wait("stalled preparation reclaimed and successor at bottom", || shown(&state, successor) && bottom(&app, successor))?;
                            let terminal = rpc(&endpoint, "request.get", json!({"request_id":stalled}))?;
                            if terminal["state"] != "failed" { return Err("preparation watchdog did not fail the stalled request".into()); }
                            let (reply, receiver) = mpsc::channel();
                            coordinator::send(&state, Task::Ready { id: stalled, revision: 2, reply })?;
                            if receiver.recv_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())?.is_ok() { return Err("late callback revived a terminal request".into()); }
                            clear(&endpoint, &state, &app)?;
                            eprintln!("PASS real IPC: FIFO preparation timeout and rejected late callback");

                            // A blocked large head cannot be overtaken by a small newcomer.
                            let display = runtime::notification_display()?;
                            let tall = (f64::from(display.area.height) / display.scale * 0.6) as u32;
                            let first = request(&endpoint, "First tall", tall, 60000)?;
                            wait("first tall notification", || shown(&state, first))?;
                            let blocked = request(&endpoint, "Blocked tall head", tall, 60000)?;
                            let small = request(&endpoint, "Small newcomer", 100, 60000)?;
                            std::thread::sleep(Duration::from_millis(300));
                            if shown(&state, blocked) || shown(&state, small) { return Err("space admission broke FIFO".into()); }
                            app.get_webview_window(&first.to_string()).unwrap().close().map_err(|e| e.to_string())?;
                            wait("blocked head promoted before newcomer", || shown(&state, blocked) && shown(&state, small))?;
                            assert_stack(&app, &state)?;
                            clear(&endpoint, &state, &app)?;
                            eprintln!("PASS real IPC: strict FIFO and size-aware admission");

                            let lower = request(&endpoint, "Recovery lower", 180, 60000)?;
                            let upper = request(&endpoint, "Recovery upper", 180, 60000)?;
                            wait("pair presented", || shown(&state, lower) && shown(&state, upper))?;
                            let window = app.get_webview_window(&lower.to_string()).unwrap();
                            window.hide().map_err(|e| e.to_string())?;
                            wait("presented but hidden HWND restored", || windows::observe(&window).is_ok_and(|n| n.healthy()))?;
                            runtime::configure_frameless_windows_window(&window, false)?;
                            wait("presented but DWM-cloaked HWND restored", || windows::observe(&window).is_ok_and(|n| n.healthy()))?;
                            let (reply, receiver) = mpsc::channel();
                            window.with_webview(move |webview| { let _ = reply.send(unsafe { webview.controller().SetIsVisible(false) }.map_err(|e| e.to_string())); }).map_err(|e| e.to_string())?;
                            receiver.recv_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())??;
                            wait("hidden WebView controller restored", || windows::observe(&window).is_ok_and(|n| n.healthy()))?;
                            let (reply, receiver) = mpsc::channel();
                            window.with_webview(move |webview| {
                                let controller = webview.controller();
                                let result = (|| -> Result<(), String> {
                                    let mut bounds = Default::default();
                                    unsafe {
                                        controller.Bounds(&mut bounds).map_err(|e| e.to_string())?;
                                        bounds.right = 1; bounds.bottom = 1;
                                        controller.SetBounds(bounds).map_err(|e| e.to_string())
                                    }
                                })();
                                let _ = reply.send(result);
                            }).map_err(|e| e.to_string())?;
                            receiver.recv_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())??;
                            wait("stale WebView bounds restored", || windows::observe(&window).is_ok_and(|n| n.healthy()))?;
                            assert_stack(&app, &state)?;
                            window.destroy().map_err(|e| e.to_string())?;
                            wait("unexpected destruction reclaimed and upper compacted", || bottom(&app, upper) && state.registry.lock().unwrap().snapshot(lower).unwrap().state.is_terminal())?;
                            clear(&endpoint, &state, &app)?;
                            eprintln!("PASS native recovery: hidden HWND, DWM cloak, hidden WebView, unexpected destroy");

                            // Content updates and duplicate readiness must not extend duration.
                            let timed = create(&endpoint, json!({"version":"1.0","mode":"notify","template":"notification","updatable":true,"duration_ms":1500,"window":{"height":180},"content":{"title":"Timed notification"}}))?;
                            wait("timed notification", || shown(&state, timed))?;
                            rpc(&endpoint, "request.update", json!({"request_id":timed,"expected_revision":1,"updatable":false,"content":{"title":"Updated notification"}}))?;
                            wait("automatic expiry", || state.registry.lock().unwrap().snapshot(timed).unwrap().state.is_terminal())?;
                            wait("expired native window removed", || app.get_webview_window(&timed.to_string()).is_none())?;
                            let item = state.history.lock().unwrap().as_ref().unwrap().get(&timed.to_string()).map_err(|e| e.to_string())?;
                            if item.state != "expired" || item.read_at_ms.is_some() { return Err("expiry did not preserve unread history".into()); }

                            // A real retained HWND receives silent progress, one
                            // explicit reshow, and a final sealed report.
                            let live = create(&endpoint, json!({"version":"1.0","mode":"notify","template":"notification","updatable":true,"requires_read":true,"duration_ms":60000,"content":{"title":"Backup progress"}}))?;
                            wait("live notification", || shown(&state, live))?;
                            let live_window = app.get_webview_window(&live.to_string()).unwrap();
                            let original_hwnd = live_window.hwnd().map_err(|e| e.to_string())?;
                            live_window.close().map_err(|e| e.to_string())?;
                            wait("user close hides rather than ends", || state.registry.lock().unwrap().snapshot(live).is_ok_and(|s| s.state == crate::core::RequestState::Hidden))?;
                            let progress = rpc(&endpoint, "request.update", json!({"request_id":live,"expected_revision":1,"content":{"title":"Backup 50%","blocks":[{"type":"progress","value":50}]}}))?;
                            if progress["state"] != "hidden" || live_window.is_visible().map_err(|e| e.to_string())? { return Err("ordinary progress reopened a user-closed window".into()); }
                            let final_report = rpc(&endpoint, "request.update", json!({"request_id":live,"expected_revision":2,"content":{"title":"Backup complete","blocks":[{"type":"progress","value":100}]},"updatable":false,"reshow":true,"duration_ms":1500}))?;
                            if final_report["updatable"] != false { return Err("final update did not seal updates".into()); }
                            wait("final report reshow", || shown(&state, live))?;
                            if app.get_webview_window(&live.to_string()).unwrap().hwnd().map_err(|e| e.to_string())? != original_hwnd { return Err("reshow recreated the native window".into()); }
                            if state.registry.lock().unwrap().snapshot(live).unwrap().reshow_count != 1 { return Err("reshow did not consume one opportunity".into()); }
                            wait("final report archived", || state.registry.lock().unwrap().snapshot(live).unwrap().state.is_terminal())?;
                            let final_item = state.history.lock().unwrap().as_ref().unwrap().get(&live.to_string()).map_err(|e| e.to_string())?;
                            if final_item.revision != 3 || final_item.content["title"] != "Backup complete" || final_item.read_at_ms.is_some() { return Err("final summary was lost or inherited the progress acknowledgement".into()); }

                            let automatic = create(&endpoint, json!({"version":"1.0","mode":"notify","template":"notification","updatable":true,"duration_ms":600,"content":{"title":"Auto-hidden progress"}}))?;
                            wait("automatic progress first shown", || shown(&state, automatic))?;
                            wait("automatic progress hidden", || state.registry.lock().unwrap().snapshot(automatic).unwrap().state == crate::core::RequestState::Hidden)?;
                            rpc(&endpoint, "request.update", json!({"request_id":automatic,"expected_revision":1,"content":{"title":"Auto-hidden next report"},"duration_ms":60000}))?;
                            wait("automatic progress shown on ordinary update", || shown(&state, automatic))?;
                            app.get_webview_window(&automatic.to_string()).unwrap().close().map_err(|e| e.to_string())?;
                            wait("automatic progress user closed", || state.registry.lock().unwrap().snapshot(automatic).unwrap().state == crate::core::RequestState::Hidden)?;
                            rpc(&endpoint, "history.clear", json!({}))?;
                            if rpc(&endpoint, "request.get", json!({"request_id":automatic})).is_ok() { return Err("history clear retained a hidden update endpoint".into()); }
                            wait("cleared retained HWND removed", || app.get_webview_window(&automatic.to_string()).is_none())?;
                            eprintln!("PASS live notifications: retained HWND, silent progress, final reshow, automatic timeout, and clear");

                            let dialog = json!({"version":"1.0","mode":"dialog","timeout_seconds":2,"content":{"title":"FIFO dialog"},"actions":[{"id":"ok","label":"OK","submit":true}]});
                            let first_dialog = create(&endpoint, dialog.clone())?;
                            let next_dialog = create(&endpoint, dialog)?;
                            let independent = request(&endpoint, "Independent notification", 180, 60000)?;
                            wait("dialog and notification independently presented", || shown(&state, first_dialog) && shown(&state, independent))?;
                            let dragged = app.get_webview_window(&independent.to_string()).unwrap();
                            let original = dragged.outer_position().map_err(|e| e.to_string())?;
                            let point = crate::placement::Point { x: original.x - 40, y: original.y - 40 };
                            windows::position(&dragged, point)?;
                            wait("second FIFO dialog presented", || shown(&state, next_dialog))?;
                            let first_result = rpc(&endpoint, "request.get", json!({"request_id":first_dialog}))?;
                            if first_result["state"] != "expired" || first_result["exit_code"] != 11 { return Err("dialog timeout/FIFO behavior changed".into()); }
                            wait("second dialog expired", || state.registry.lock().unwrap().summary(next_dialog).unwrap().state.is_terminal())?;
                            let position = dragged.outer_position().map_err(|e| e.to_string())?;
                            if position.x != point.x || position.y != point.y { return Err("unrelated dialog activity reset a dragged notification".into()); }
                            clear(&endpoint, &state, &app)?;
                            eprintln!("PASS dialog FIFO, real timeout, and independent notification scheduling");

                            let coalesced = json!({"version":"1.0","mode":"notify","template":"notification","coalesce_key":"parallel-submissions","duration_ms":60000,"content":{"title":"Parallel coalescing"}});
                            let submissions: Vec<_> = (0..8).map(|_| {
                                let endpoint = endpoint.clone(); let value = coalesced.clone();
                                std::thread::spawn(move || create(&endpoint, value))
                            }).collect();
                            let mut ids = Vec::new();
                            for submission in submissions { ids.push(submission.join().map_err(|_| "submission thread failed".to_string())??); }
                            if ids.iter().any(|id| *id != ids[0]) { return Err("concurrent coalescing created duplicate requests".into()); }
                            clear(&endpoint, &state, &app)?;
                            eprintln!("PASS concurrent real IPC coalescing and cancellation during preparation");

                            // Queue the exact mixed-size fixtures through production validation.
                            let fixtures = [include_str!("../../protocol/examples/notification-data-sync.json"),
                                include_str!("../../protocol/examples/notification-security-incident.json"),
                                include_str!("../../protocol/examples/notification-backup-audit.json"),
                                include_str!("../../protocol/examples/notification-success.json"),
                                include_str!("../../protocol/examples/notification-deployment-rollout.json")];
                            for round in 0..8 {
                                for fixture in fixtures {
                                    let mut value: Value = serde_json::from_str(fixture).unwrap();
                                    value["duration_ms"] = json!(60000);
                                    create(&endpoint, value)?;
                                }
                                while state.registry.lock().unwrap().active_notification_count() > 0 {
                                    wait("mixed-size stack settled", || {
                                        let registry = state.registry.lock().unwrap();
                                        let settled = !registry.visible_notification_ids().is_empty() && registry.pending_notification().is_none();
                                        drop(registry);
                                        settled && assert_stack(&app, &state).is_ok()
                                    })?;
                                    assert_stack(&app, &state)?;
                                    let lower = state.registry.lock().unwrap().visible_notification_ids()[0];
                                    app.get_webview_window(&lower.to_string()).unwrap().close().map_err(|e| e.to_string())?;
                                    wait("old bottom removed", || app.get_webview_window(&lower.to_string()).is_none())?;
                                }
                                wait("no residual native windows", || app.webview_windows().is_empty())?;
                                eprintln!("PASS mixed-size real IPC round {}: compact layout, no residual HWND or slot", round + 1);
                            }
                            // Diagnostic response exposes logical and native states separately.
                            let status = rpc(&endpoint, "host.status", json!({}))?;
                            if status["notifications"] != 0 || status["windows"].as_array().unwrap().len() != 0 { return Err("diagnostics retained ghost notifications".into()); }
                            Ok(())
                        })();
                        let _ = done.send(result);
                        state.stopping.store(true, Ordering::SeqCst);
                        app.exit(0);
                    });
                    Ok(())
                }).build(tauri::generate_context!()).unwrap();
            app.run_return(|app, event| {
                if let RunEvent::ExitRequested { api, .. } = event {
                    if !app
                        .state::<Arc<HostState>>()
                        .stopping
                        .load(Ordering::SeqCst)
                    {
                        api.prevent_exit();
                    }
                }
            });
            result
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
        }
    }
}
