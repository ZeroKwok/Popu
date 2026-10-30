// The sole writer of the request registry. UI callbacks and IPC threads submit
// events; they never wait on native work while blocking the UI event loop.
use std::collections::HashMap;

use super::*;
use crate::core::Effect;
use crate::notifications::Layout;
use crate::placement::Display;

pub(super) enum Command {
    Create(CreateParams),
    Update(UpdateParams),
    Dismiss(Uuid),
    Submit {
        id: Uuid,
        action: String,
        values: Map<String, Value>,
    },
    Finish {
        id: Uuid,
        status: ResponseStatus,
        action: Option<String>,
    },
    Status,
    Clear,
    ClearHistory,
    Stop,
}

pub(super) enum Task {
    Command(Command, Sender<Result<Value, Value>>),
    Ready {
        id: Uuid,
        revision: u64,
        reply: Sender<Result<(), String>>,
    },
    RenderFailed {
        id: Uuid,
        message: String,
    },
    Closed(Uuid),
    Destroyed(Uuid),
    History {
        show: bool,
    },
}

pub(super) fn send(state: &HostState, task: Task) -> Result<(), String> {
    state
        .sender
        .lock()
        .unwrap()
        .as_ref()
        .ok_or("window coordinator unavailable")?
        .send(task)
        .map_err(|e| e.to_string())
}

pub(super) fn call(state: &HostState, command: Command) -> Result<Value, Value> {
    let (reply, receiver) = mpsc::channel();
    send(state, Task::Command(command, reply)).map_err(invalid)?;
    receiver.recv().map_err(invalid)?
}

pub(super) async fn call_async(state: Arc<HostState>, command: Command) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || call(&state, command))
        .await
        .map_err(|e| e.to_string())?
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub(super) fn start(app: AppHandle, state: Arc<HostState>) {
    let (sender, receiver) = mpsc::channel();
    *state.sender.lock().unwrap() = Some(sender);
    std::thread::spawn(move || {
        let mut worker = Coordinator {
            app,
            state,
            display: None,
            sizes: HashMap::new(),
            closing: HashMap::new(),
            unhealthy: HashMap::new(),
            dirty: false,
            layout_version: 0,
            next_check: Instant::now(),
        };
        while !worker.state.stopping.load(Ordering::SeqCst) {
            let task = match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(task) => Some(task),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let effects = worker
                .state
                .registry
                .lock()
                .unwrap()
                .expire_due(Instant::now());
            worker.apply(effects);
            if let Some(task) = task { worker.handle(task); }
            if Instant::now() >= worker.next_check {
                worker.check_windows();
                worker.next_check = Instant::now() + Duration::from_secs(1);
            }
            if !worker.state.stopping.load(Ordering::SeqCst) {
                worker.drive();
            }
            if let Err(error) = worker.flush() { eprintln!("Popu persistence error: {error}"); }
            worker.state.changed.notify_all();
        }
    });
}

struct Coordinator {
    app: AppHandle,
    state: Arc<HostState>,
    display: Option<Display>,
    sizes: HashMap<Uuid, (u32, u32)>,
    closing: HashMap<Uuid, u32>,
    unhealthy: HashMap<Uuid, u8>,
    dirty: bool,
    layout_version: u64,
    next_check: Instant,
}

impl Coordinator {
    fn handle(&mut self, task: Task) {
        match task {
            Task::Command(command, reply) => {
                let mut result = self.command(command);
                if let Err(error) = self.flush() { result = Err(error); }
                let _ = reply.send(result);
            }
            Task::Ready {
                id,
                revision,
                reply,
            } => {
                let result = self.ready(id, revision);
                if let Err(error) = &result {
                    self.fail(id, error);
                }
                let _ = reply.send(result);
            }
            Task::RenderFailed { id, message } => self.fail(id, &message),
            Task::Closed(id) => {
                let request = self.state.registry.lock().unwrap().request(id).cloned();
                if let Some(request) = request {
                    let result = {
                        self.state.registry.lock().unwrap().complete(
                            id,
                            request.terminal_response(ResponseStatus::Closed, None),
                            10,
                        )
                    };
                    if let Ok(effects) = result {
                        self.apply(effects);
                    }
                }
            }
            Task::Destroyed(id) => {
                let expected = self.closing.remove(&id).is_some();
                if !expected { self.fail(id, "native window destroyed before request completion"); }
            }
            Task::History { show } => open_history_on_worker(&self.app, &self.state, show),
        }
    }

    fn command(&mut self, command: Command) -> Result<Value, Value> {
        match command {
            Command::Create(params) => {
                let created = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .create(params.request, params.template);
                serde_json::to_value(created).map_err(invalid)
            }
            Command::Update(params) => {
                let effects = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .update_with_options(params.request_id, params.expected_revision, params.content, UpdateOptions {
                        updatable: params.updatable, reshow: params.reshow, duration_ms: params.duration_ms,
                    })
                    .map_err(invalid_registry)?;
                self.apply(effects);
                self.snapshot(params.request_id)
            }
            Command::Dismiss(id) => {
                let effects = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .dismiss(id)
                    .map_err(invalid_registry)?;
                self.apply(effects);
                self.snapshot(id)
            }
            Command::Submit { id, action, values } => {
                let request = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .request(id)
                    .cloned()
                    .ok_or_else(|| invalid("request not found"))?;
                let response = request
                    .submitted_response(&action, values)
                    .map_err(invalid)?;
                let code = request
                    .actions
                    .iter()
                    .find(|a| a.id == action)
                    .and_then(|a| a.exit_code)
                    .map(i32::from)
                    .unwrap_or(0);
                let effects = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .complete(id, response, code)
                    .map_err(invalid_registry)?;
                self.apply(effects);
                Ok(Value::Null)
            }
            Command::Finish { id, status, action } => {
                let request = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .request(id)
                    .cloned()
                    .ok_or_else(|| invalid("request not found"))?;
                let response = request.terminal_response(status, action.clone());
                let code = action
                    .as_deref()
                    .and_then(|id| request.actions.iter().find(|a| a.id == id))
                    .and_then(|a| a.exit_code)
                    .map(i32::from)
                    .unwrap_or(match status {
                        ResponseStatus::Submitted | ResponseStatus::Confirmed => 0,
                        ResponseStatus::TimedOut => 11,
                        _ => 10,
                    });
                let effects = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .complete(id, response, code)
                    .map_err(invalid_registry)?;
                self.apply(effects);
                Ok(Value::Null)
            }
            Command::Clear => {
                let ids = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .active_notification_ids();
                for id in ids {
                    let result = self.state.registry.lock().unwrap().dismiss(id);
                    if let Ok(effects) = result {
                        self.apply(effects);
                    }
                }
                Ok(Value::Null)
            }
            Command::ClearHistory => {
                let ids = self.state.registry.lock().unwrap().hidden_notification_ids();
                let count = clear_history(&self.state).map_err(invalid)?;
                // Destroy retained hidden HWNDs as well as their durable records.
                for id in ids { self.apply(vec![Effect::Close(id)]); }
                emit_history_changed(&self.app);
                update_tray_tooltip(&self.app, &self.state);
                self.state.changed.notify_all();
                Ok(json!({"status":"cleared","count":count}))
            }
            Command::Status => {
                let registry = self.state.registry.lock().unwrap();
                let ids = registry.active_notification_ids();
                let requests: Vec<_> = ids
                    .iter()
                    .map(|id| registry.summary(*id).unwrap())
                    .collect();
                let mut counts = serde_json::Map::new();
                for name in ["queued", "preparing", "ready", "shown"] {
                    counts.insert(name.into(), json!(0));
                }
                for request in &requests {
                    let key = serde_json::to_value(request.state)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();
                    let count = counts.get_mut(&key).unwrap();
                    *count = json!(count.as_u64().unwrap() + 1);
                }
                let total = registry.active_notification_count();
                let hidden = registry.hidden_notification_ids().len();
                drop(registry);
                let windows: Vec<_> = ids
                    .iter()
                    .filter_map(|id| {
                        self.app.get_webview_window(&id.to_string()).map(|window| {
                            match windows::observe(&window) {
                                Ok(observation) => json!({"request_id":id,"native":observation}),
                                Err(error) => json!({"request_id":id,"error":error}),
                            }
                        })
                    })
                    .collect();
                Ok(
                    json!({"status":"running","notifications":total,"hidden_notifications":hidden,"notification_states":counts,"requests":requests.into_iter().take(128).collect::<Vec<_>>(),"requests_truncated":total > 128,"windows":windows,"layout_version":self.layout_version,"closing":self.closing.keys().collect::<Vec<_>>()}),
                )
            }
            Command::Stop => {
                self.state.stopping.store(true, Ordering::SeqCst);
                let ids = self.state.registry.lock().unwrap().shutdown();
                if let Err(error) = self.flush() { eprintln!("Popu shutdown persistence error: {error}"); }
                self.state.changed.notify_all();
                let app = self.app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(30));
                    app.exit(0);
                });
                Ok(json!({"status":"stopping","cancelled":ids.len()}))
            }
        }
    }

    fn snapshot(&self, id: Uuid) -> Result<Value, Value> {
        serde_json::to_value(
            self.state
                .registry
                .lock()
                .unwrap()
                .snapshot(id)
                .map_err(invalid_registry)?,
        )
        .map_err(invalid)
    }

    fn flush(&mut self) -> Result<(), Value> {
        let ids = self.state.registry.lock().unwrap().take_dirty();
        let mut failure = None;
        for id in ids {
            if let Err(error) = persist_notification(&self.app, &self.state, id) {
                self.state.registry.lock().unwrap().mark_dirty(id);
                failure = Some(json!({"code":"persistence_failed","message":error}));
            }
        }
        failure.map_or(Ok(()), Err)
    }

    fn fail(&mut self, id: Uuid, reason: &str) {
        let result = self.state.registry.lock().unwrap().fail(id);
        if let Ok(effects) = result {
            eprintln!("Popu request {id} failed: {reason}");
            self.apply(effects);
        }
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Hide(id) => {
                    let result = self.app.get_webview_window(&id.to_string())
                        .map(|window| windows::conceal(&window)).unwrap_or(Ok(()));
                    self.sizes.remove(&id);
                    self.unhealthy.remove(&id);
                    self.dirty = true;
                    if let Err(error) = result { self.fail(id, &error); }
                }
                Effect::Close(id) => {
                    let notification = self
                        .state
                        .registry
                        .lock()
                        .unwrap()
                        .request(id)
                        .is_some_and(|request| request.mode == Mode::Notify);
                    self.sizes.remove(&id);
                    self.unhealthy.remove(&id);
                    self.closing.insert(id, 0);
                    self.close(id);
                    self.dirty |= notification;
                }
                Effect::Update(id) => {
                    let content = self
                        .state
                        .registry
                        .lock()
                        .unwrap()
                        .request(id)
                        .map(|r| r.content.clone());
                    if let (Some(window), Some(content)) =
                        (self.app.get_webview_window(&id.to_string()), content)
                    {
                        let revision = self
                            .state
                            .registry
                            .lock()
                            .unwrap()
                            .summary(id)
                            .unwrap()
                            .revision;
                        let occurred_at_ms = timestamp_ms(self.state.registry.lock().unwrap().snapshot(id).unwrap().occurred_at);
                        let result = serde_json::to_string(&content)
                            .map_err(|e| e.to_string())
                            .and_then(|value| {
                                window
                                    .eval(format!("window.popuApplyUpdate?.({value}, {revision}, {occurred_at_ms})"))
                                    .map_err(|e| e.to_string())
                            });
                        if let Err(error) = result {
                            self.fail(id, &error);
                        }
                    }
                }
            }
        }
    }

    fn close(&mut self, id: Uuid) {
        match windows::close(&self.app, id) {
            Ok(()) => {
                // Native dispatch can return before destruction. Keep cleanup
                // tracked until the manager/event callback confirms removal.
                if self.app.get_webview_window(&id.to_string()).is_none() {
                    self.closing.remove(&id);
                }
            }
            Err(error) => {
                let attempts = self.closing.entry(id).or_default();
                *attempts += 1;
                eprintln!("Popu cleanup {id}, attempt {attempts}: {error}");
            }
        }
    }

    fn display(&mut self) -> Result<Display, String> {
        if let Some(display) = self.display {
            return Ok(display);
        }
        let display = runtime::notification_display()?;
        self.display = Some(display);
        Ok(display)
    }

    fn plan(
        &mut self,
        extra: Option<(Uuid, (u32, u32))>,
    ) -> Result<Option<Vec<(Uuid, placement::Point)>>, String> {
        let display = self.display()?;
        let mut ids = self
            .state
            .registry
            .lock()
            .unwrap()
            .visible_notification_ids();
        let mut sizes = ids
            .iter()
            .map(|id| {
                self.sizes
                    .get(id)
                    .copied()
                    .ok_or_else(|| format!("missing measured size for {id}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some((id, size)) = extra {
            ids.push(id);
            sizes.push(size);
        }
        Ok(Layout::new(display)
            .stack(&sizes)
            .map(|points| ids.into_iter().zip(points).collect()))
    }

    fn reflow(&mut self) -> Result<(), String> {
        // A reduced work area parks upper windows without losing content or
        // resetting their remaining visible lifetime. Their HWNDs are reused.
        loop {
            if !self.closing.is_empty() {
                return Err("waiting for native window cleanup".into());
            }
            if let Some(plan) = self.plan(None)? {
                self.layout_version += 1;
                for (id, point) in plan {
                    let result = self
                        .app
                        .get_webview_window(&id.to_string())
                        .ok_or("native window missing".to_string())
                        .and_then(|window| windows::position(&window, point));
                    if let Err(error) = result {
                        self.fail(id, &error);
                        return Ok(());
                    }
                }
                self.dirty = false;
                return Ok(());
            }
            let id = self
                .state
                .registry
                .lock()
                .unwrap()
                .visible_notification_ids()
                .last()
                .copied();
            let Some(id) = id else {
                return Err("empty stack did not fit".into());
            };
            let result = self
                .app
                .get_webview_window(&id.to_string())
                .ok_or("native window missing".to_string())
                .and_then(|window| windows::conceal(&window));
            match result {
                Ok(()) => {
                    self.state
                        .registry
                        .lock()
                        .unwrap()
                        .park_notification(id, Instant::now())
                        .map_err(|e| e.to_string())?;
                    self.sizes.remove(&id);
                }
                Err(error) => self.fail(id, &error),
            }
        }
    }

    fn ready(&mut self, id: Uuid, revision: u64) -> Result<(), String> {
        // A newer content update has its own readiness acknowledgement. An old
        // renderer callback must not reveal half-rendered replacement content.
        let snapshot = self
            .state
            .registry
            .lock()
            .unwrap()
            .summary(id)
            .map_err(|e| e.to_string())?;
        if snapshot.state.is_terminal() {
            return Err("request already ended".into());
        }
        if matches!(snapshot.state, crate::core::RequestState::Hidden | crate::core::RequestState::Queued) { return Ok(()); }
        if snapshot.revision != revision {
            self.apply(vec![Effect::Update(id)]);
            return Ok(());
        }
        self.state
            .registry
            .lock()
            .unwrap()
            .mark_ready(id)
            .map_err(|e| e.to_string())?;
        if !self.closing.is_empty() || self.dirty {
            return Ok(());
        }
        self.present(id)
    }

    fn present(&mut self, id: Uuid) -> Result<(), String> {
        if self.state.registry.lock().unwrap().is_presented(id) {
            return Ok(());
        }
        if !self
            .state
            .registry
            .lock()
            .unwrap()
            .ready_ids()
            .contains(&id)
        {
            return Ok(());
        }
        let request = self
            .state
            .registry
            .lock()
            .unwrap()
            .request(id)
            .cloned()
            .ok_or("request not found")?;
        let window = self
            .app
            .get_webview_window(&id.to_string())
            .ok_or("native window missing")?;
        if request.mode == Mode::Notify {
            let size = window.inner_size().map_err(|e| e.to_string())?;
            let Some(plan) = self.plan(Some((id, (size.width, size.height))))? else {
                return Ok(());
            };
            let (_, point) = plan.last().unwrap();
            windows::position(&window, *point)?;
            // Normal appends preserve existing user drags. Membership removal
            // compacts the whole stack before another builder is started.
        }
        let observation = windows::reveal(&window)?;
        self.state
            .registry
            .lock()
            .unwrap()
            .mark_shown(id, Instant::now())
            .map_err(|e| e.to_string())?;
        if request.mode == Mode::Notify {
            self.sizes.insert(id, observation.size);
            self.layout_version += 1;
        } else {
            window.set_focus().map_err(|e| e.to_string())?;
        }
        eprintln!(
            "Popu presented {id}, layout {}: {observation:?}",
            self.layout_version
        );
        Ok(())
    }

    fn drive(&mut self) {
        // Closing HWNDs must be hidden before either compaction or promotion.
        if !self.closing.is_empty() {
            return;
        }
        if self.dirty {
            if let Err(error) = self.reflow() {
                eprintln!("Popu layout failed: {error}");
                return;
            }
            if self.dirty {
                return;
            }
        }
        let ready = self.state.registry.lock().unwrap().ready_ids();
        for id in ready {
            if let Err(error) = self.present(id) {
                self.fail(id, &error);
            }
        }
        let dialog = self.state.registry.lock().unwrap().next_dialog();
        if let Some(id) = dialog {
            self.prepare(id);
        }
        let next = self.state.registry.lock().unwrap().next_notification();
        if let Some(id) = next {
            let request = self
                .state
                .registry
                .lock()
                .unwrap()
                .request(id)
                .cloned()
                .unwrap();
            let fits = self.display().and_then(|display| {
                let size = if let Some(window) = self.app.get_webview_window(&id.to_string()) {
                    let size = window.inner_size().map_err(|e| e.to_string())?;
                    (size.width, size.height)
                } else {
                    Layout::new(display).notification_size(&request).size
                };
                self.plan(Some((id, size)))
            });
            match fits {
                Ok(Some(_)) => self.prepare(id),
                Ok(None) => {} // Strict FIFO: a smaller newcomer cannot pass the head.
                Err(error) => eprintln!("Popu notification scheduling paused: {error}"),
            }
        }
    }

    fn prepare(&mut self, id: Uuid) {
        let has_window = self.app.get_webview_window(&id.to_string()).is_some();
        let (request, template_dir) = {
            let mut registry = self.state.registry.lock().unwrap();
            if !registry.activate(id, Instant::now()) {
                return;
            }
            (
                registry.request(id).unwrap().clone(),
                registry.template_dir(id).map(PathBuf::from),
            )
        };
        if has_window {
            if self.state.registry.lock().unwrap().is_preparing(id) {
                self.apply(vec![Effect::Update(id)]);
                return;
            }
            if let Err(error) = self.present(id) {
                self.fail(id, &error);
            }
            return;
        }
        let result = (|| {
            templates::resolve(&request, template_dir.as_deref()).map_err(|e| e.to_string())?;
            let placement = if request.mode == Mode::Notify {
                Layout::new(self.display()?).notification_size(&request)
            } else {
                runtime::resolve_window_placement(&request)?
            };
            windows::create(&self.app, id, &request, placement)
        })();
        if let Err(error) = result {
            self.fail(id, &error);
        }
    }

    fn check_windows(&mut self) {
        let closing: Vec<_> = self.closing.keys().copied().collect();
        for id in closing {
            self.close(id);
        }
        match runtime::notification_display() {
            Ok(display) if self.display != Some(display) => {
                self.display = Some(display);
                let ids = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .visible_notification_ids();
                for id in ids {
                    let request = self
                        .state
                        .registry
                        .lock()
                        .unwrap()
                        .request(id)
                        .cloned()
                        .unwrap();
                    let result = self
                        .app
                        .get_webview_window(&id.to_string())
                        .ok_or("native window missing".to_string())
                        .and_then(|window| {
                            windows::conceal(&window)?;
                            windows::resize(
                                &window,
                                Layout::new(display).notification_size(&request),
                            )?;
                            #[cfg(windows)]
                            runtime::configure_frameless_windows_window(&window, false)?;
                            let size = window.inner_size().map_err(|e| e.to_string())?;
                            Ok((size.width, size.height))
                        });
                    match result {
                        Ok(size) => {
                            self.sizes.insert(id, size);
                        }
                        Err(error) => self.fail(id, &error),
                    }
                }
                // Ready windows are also resized, but remain cloaked until they fit.
                let pending = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .active_notification_ids();
                for id in pending {
                    if self.state.registry.lock().unwrap().is_presented(id) {
                        continue;
                    }
                    let request = self
                        .state
                        .registry
                        .lock()
                        .unwrap()
                        .request(id)
                        .cloned()
                        .unwrap();
                    if let Some(window) = self.app.get_webview_window(&id.to_string()) {
                        let result = windows::resize(
                            &window,
                            Layout::new(display).notification_size(&request),
                        )
                        .and_then(|_| {
                            #[cfg(windows)]
                            runtime::configure_frameless_windows_window(&window, false)?;
                            Ok(())
                        });
                        if let Err(error) = result {
                            self.fail(id, &error);
                        }
                    }
                }
                self.dirty = true;
                // Place all survivors while hidden; only then reveal them. This
                // avoids briefly piling every resized HWND at the bottom anchor.
                if let Err(error) = self.reflow() {
                    eprintln!("Popu display reflow failed: {error}");
                }
                let survivors = self
                    .state
                    .registry
                    .lock()
                    .unwrap()
                    .visible_notification_ids();
                for id in survivors {
                    if let Some(window) = self.app.get_webview_window(&id.to_string()) {
                        if let Err(error) = windows::restore(&window) {
                            self.fail(id, &error);
                        }
                    }
                }
            }
            Err(error) => eprintln!("Popu display inspection failed: {error}"),
            _ => {}
        }
        let ids = self
            .state
            .registry
            .lock()
            .unwrap()
            .visible_notification_ids();
        for id in ids {
            let Some(window) = self.app.get_webview_window(&id.to_string()) else {
                self.fail(id, "presented window missing");
                continue;
            };
            let result = windows::observe(&window).and_then(|observation| {
                if observation.system_hidden() || observation.healthy() {
                    Ok(observation)
                } else {
                    eprintln!("Popu recovering {id}: {observation:?}");
                    windows::restore(&window)
                }
            });
            match result {
                Ok(observation) => {
                    self.unhealthy.remove(&id);
                    if self.sizes.insert(id, observation.size) != Some(observation.size) {
                        self.dirty = true;
                    }
                }
                Err(error) => {
                    let count = self.unhealthy.entry(id).or_default();
                    *count += 1;
                    if *count >= 3 {
                        self.fail(id, &format!("window remained unhealthy: {error}"));
                    } else {
                        eprintln!("Popu inspection {id}: {error}");
                    }
                }
            }
        }
    }
}

fn invalid_registry(error: crate::core::RegistryError) -> Value {
    serde_json::to_value(error).unwrap()
}
