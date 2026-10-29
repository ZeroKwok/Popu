// Exercise production window code in a dedicated executable with its own
// Windows activation manifest. No IPC listener, tray, or user database.
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
    #[cfg(all(test, windows))]
    mod native_tests {
        use super::*;

        fn wait_until(label: &str, mut predicate: impl FnMut() -> bool) -> Result<(), String> {
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline {
                if predicate() {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(format!("timed out waiting for {label}"))
        }

        fn check_reading_policy(
            state: &HostState,
            window: &WebviewWindow,
            unread_before: usize,
        ) -> Result<(), String> {
            wait_until("selected chart fully rendered", || {
                state
                    .history_window
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|session| session.detail_ready)
            })?;
            if runtime::is_window_foreground(window)? {
                wait_until("rendered chart acknowledged without switching rows", || {
                    state
                        .history
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .pending_read_count()
                        .unwrap()
                        == unread_before - 1
                })?;
                eprintln!("Native history test: foreground reading acknowledged");
            } else {
                // Windows may refuse foreground activation for an unattended test
                // process. Verify the guard, without weakening it to satisfy a test.
                std::thread::sleep(Duration::from_millis(250));
                if !runtime::is_window_foreground(window)?
                    && state
                        .history
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .pending_read_count()
                        .unwrap()
                        != unread_before
                {
                    return Err("background history marked selected content read".into());
                }
                eprintln!("Native history test: PASS background unread protection; SKIP foreground acknowledgement (Windows did not activate the test window)");
            }
            Ok(())
        }

        #[test]
        #[ignore = "opens a real WebView2 history window; requires an unlocked Windows desktop"]
        fn history_window_lifecycle() {
            // No IPC listener or tray, and no user history database.
            let mut store =
                HistoryStore::from_connection(rusqlite::Connection::open_in_memory().unwrap())
                    .unwrap();
            let mut registry = RequestRegistry::default();
            let mut request = Request::parse(include_str!(
                "../../protocol/examples/notification-visual-line.json"
            ))
            .unwrap();
            request.requires_read = Some(true);
            let created = registry.create(request.clone(), None);
            registry
                .complete(
                    created.request_id,
                    request.terminal_response(ResponseStatus::TimedOut, None),
                    11,
                )
                .unwrap();
            store
                .record(&registry.snapshot(created.request_id).unwrap())
                .unwrap();
            let state = Arc::new(HostState::with_history(Some(store)));
            let (sender, receiver) = mpsc::channel();
            let app = host_builder(Arc::clone(&state))
                .any_thread()
                .setup(move |app| {
                    let handle = app.handle().clone();
                    coordinator::start(handle.clone(), Arc::clone(&state));
                    std::thread::spawn(move || {
                        let result = (|| -> Result<(), String> {
                            let opened = Instant::now();
                            // Exercise the exact tray entry point, including rapid clicks.
                            for _ in 0..5 {
                                open_history(&handle);
                            }
                            wait_until("history shell", || {
                                state
                                    .history_window
                                    .lock()
                                    .unwrap()
                                    .as_ref()
                                    .is_some_and(|session| session.ready)
                            })?;
                            let window = handle
                                .get_webview_window("history")
                                .ok_or("missing history window")?;
                            let native_handle = window.hwnd().map_err(|e| e.to_string())?;
                            if handle.webview_windows().len() != 1 {
                                return Err("rapid clicks created duplicate windows".into());
                            }
                            eprintln!(
                                "Native history test: cold shell {}ms",
                                opened.elapsed().as_millis()
                            );
                            eprintln!(
                                "Native history test: visible={} outer-focused={} foreground={}",
                                window.is_visible().unwrap(),
                                window.is_focused().unwrap(),
                                runtime::is_window_foreground(&window)?
                            );
                            check_reading_policy(&state, &window, 1)?;
                            eprintln!(
                                "Native history test: chart ready/policy checked {}ms",
                                opened.elapsed().as_millis()
                            );
                            window.close().map_err(|e| e.to_string())?;
                            wait_until("history hidden on close", || {
                                !window.is_visible().unwrap_or(true)
                            })?;
                            if runtime::is_window_foreground(&window)? {
                                return Err("hidden history remained eligible for reading".into());
                            }
                            if handle.get_webview_window("history").is_none() {
                                return Err("close destroyed the cached history window".into());
                            }
                            let reopened = Instant::now();
                            for _ in 0..5 {
                                open_history(&handle);
                            }
                            wait_until("cached history visible", || {
                                window.is_visible().unwrap_or(false)
                            })?;
                            if handle
                                .get_webview_window("history")
                                .unwrap()
                                .hwnd()
                                .map_err(|e| e.to_string())?
                                != native_handle
                            {
                                return Err("reopen replaced the history WebView".into());
                            }
                            eprintln!(
                                "Native history test: cached reopen {}ms",
                                reopened.elapsed().as_millis()
                            );
                            // Recreate a hidden page, as Host startup does. Prewarming
                            // must neither show a window nor acknowledge unread content.
                            window.destroy().map_err(|e| e.to_string())?;
                            wait_until("cached window destroyed for prewarm test", || {
                                handle.get_webview_window("history").is_none()
                            })?;
                            let previous = state
                                .history_window
                                .lock()
                                .unwrap()
                                .as_ref()
                                .unwrap()
                                .started;
                            let unread_before = state
                                .history
                                .lock()
                                .unwrap()
                                .as_ref()
                                .unwrap()
                                .pending_read_count()
                                .unwrap()
                                + 1;
                            let mut registry = RequestRegistry::default();
                            let request = Request::parse(include_str!(
                                "../../protocol/examples/notification-visual-line.json"
                            ))
                            .unwrap();
                            let created = registry.create(request.clone(), None);
                            registry
                                .complete(
                                    created.request_id,
                                    request.terminal_response(ResponseStatus::TimedOut, None),
                                    11,
                                )
                                .unwrap();
                            let mut snapshot = registry.snapshot(created.request_id).unwrap();
                            snapshot.requires_read = true;
                            state
                                .history
                                .lock()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .record(&snapshot)
                                .unwrap();
                            schedule_history(&handle, false);
                            wait_until("hidden prewarm ready", || {
                                state.history_window.lock().unwrap().as_ref().is_some_and(
                                    |session| session.started != previous && session.ready,
                                )
                            })?;
                            let warm = handle.get_webview_window("history").unwrap();
                            if warm.is_visible().unwrap_or(true) {
                                return Err("prewarming showed the history window".into());
                            }
                            std::thread::sleep(Duration::from_millis(200));
                            if state
                                .history_window
                                .lock()
                                .unwrap()
                                .as_ref()
                                .unwrap()
                                .detail_ready
                                || state
                                    .history
                                    .lock()
                                    .unwrap()
                                    .as_ref()
                                    .unwrap()
                                    .pending_read_count()
                                    .unwrap()
                                    != unread_before
                            {
                                return Err("prewarming marked hidden content read".into());
                            }
                            let opened = Instant::now();
                            open_history(&handle);
                            wait_until("prewarmed history visible", || {
                                warm.is_visible().unwrap_or(false)
                            })?;
                            eprintln!(
                                "Native history test: prewarmed open {}ms",
                                opened.elapsed().as_millis()
                            );
                            check_reading_policy(&state, &warm, unread_before)?;
                            Ok(())
                        })();
                        let _ = sender.send(result);
                        state.stopping.store(true, Ordering::SeqCst);
                        handle.exit(0);
                    });
                    Ok(())
                })
                .build(tauri::generate_context!())
                .unwrap();
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
            receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
        }
    }
}
