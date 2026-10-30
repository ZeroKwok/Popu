//! Tauri-independent request registry and scheduler.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::protocol::{Content, ContentBlock, Mode, Request, Response, ResponseStatus};

pub const MAX_VISIBLE_NOTIFICATIONS: usize = 3;
pub const PRESENTATION_TIMEOUT: Duration = Duration::from_secs(15);
pub const NOTIFICATION_IDLE_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);
pub const MAX_USER_RESHOWS: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestState {
    Queued,
    Preparing,
    Ready,
    Shown,
    Hidden,
    Confirmed,
    Cancelled,
    Closed,
    Expired,
    Failed,
    Stale,
}

impl RequestState {
    pub fn is_terminal(self) -> bool {
        !matches!(
            self,
            Self::Queued | Self::Preparing | Self::Ready | Self::Shown | Self::Hidden
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestSnapshot {
    pub request_id: Uuid,
    pub state: RequestState,
    pub revision: u64,
    pub mode: Mode,
    pub requires_read: bool,
    pub updatable: bool,
    pub occurred_at: SystemTime,
    pub user_closed: bool,
    pub hidden_reason: Option<RequestState>,
    pub reshow_count: u32,
    pub reshow_suppressed: bool,
    pub acknowledged_revision: u64,
    pub content: Content,
    pub created_at: SystemTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown_at: Option<SystemTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<SystemTime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Response>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestSummary {
    pub request_id: Uuid,
    pub state: RequestState,
    pub revision: u64,
    pub title: String,
    pub shown_at: Option<SystemTime>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateResult {
    pub status: &'static str,
    pub request_id: Uuid,
    pub state: RequestState,
    pub revision: u64,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Update(Uuid),
    Hide(Uuid),
    Close(Uuid),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateOptions {
    #[serde(default)]
    pub updatable: Option<bool>,
    #[serde(default)]
    pub reshow: bool,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Error, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum RegistryError {
    #[error("request was not found")]
    NotFound,
    #[error("request is already terminal")]
    Terminal,
    #[error("notification does not accept updates")]
    NotUpdatable,
    #[error("notification expired after 12 hours without an update")]
    NotificationExpired,
    #[error("revision conflict (current revision: {current_revision})")]
    RevisionConflict { current_revision: u64 },
    #[error("a shown dialog's form structure cannot be changed")]
    FormStructureChanged,
    #[error("updated content is invalid: {message}")]
    InvalidContent { message: String },
}

#[derive(Serialize, Deserialize)]
struct Record {
    request: Request,
    template_dir: Option<PathBuf>,
    state: RequestState,
    revision: u64,
    created_at: SystemTime,
    occurred_at: SystemTime,
    user_closed: bool,
    hidden_reason: Option<RequestState>,
    reshow_count: u32,
    reshow_pending: bool,
    reshow_suppressed: bool,
    acknowledged_revision: u64,
    shown_at: Option<SystemTime>,
    #[serde(skip)]
    phase_started: Option<Instant>,
    visible_duration: Duration,
    ended_at: Option<SystemTime>,
    response: Option<Response>,
    exit_code: Option<i32>,
}

#[derive(Default)]
pub struct RequestRegistry {
    records: HashMap<Uuid, Record>,
    dialog_queue: VecDeque<Uuid>,
    active_dialog: Option<Uuid>,
    notification_queue: VecDeque<Uuid>,
    notification_pending: Option<Uuid>,
    notification_visible: VecDeque<Uuid>,
    coalesced: HashMap<(ModeKey, String), Uuid>,
    dirty: HashSet<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ModeKey {
    Dialog,
    Notify,
}
impl From<Mode> for ModeKey {
    fn from(value: Mode) -> Self {
        match value {
            Mode::Dialog => Self::Dialog,
            Mode::Notify => Self::Notify,
        }
    }
}

impl RequestRegistry {
    pub fn create(&mut self, request: Request, template_dir: Option<PathBuf>) -> CreateResult {
        if let Some(key) = request.coalesce_key.as_ref() {
            if let Some(id) = self
                .coalesced
                .get(&(request.mode.into(), key.clone()))
                .copied()
            {
                if let Some(record) = self.records.get(&id).filter(|r| !r.state.is_terminal()) {
                    return CreateResult {
                        status: "accepted",
                        request_id: id,
                        state: record.state,
                        revision: record.revision,
                        duplicate: true,
                    };
                }
            }
        }
        let id = Uuid::new_v4();
        let state = RequestState::Queued;
        if let Some(key) = request.coalesce_key.clone() {
            self.coalesced.insert((request.mode.into(), key), id);
        }
        match request.mode {
            Mode::Dialog => self.dialog_queue.push_back(id),
            Mode::Notify => self.notification_queue.push_back(id),
        }
        self.records.insert(
            id,
            Record {
                request,
                template_dir,
                state,
                revision: 1,
                created_at: SystemTime::now(),
                occurred_at: SystemTime::now(),
                user_closed: false,
                hidden_reason: None,
                reshow_count: 0,
                reshow_pending: false,
                reshow_suppressed: false,
                acknowledged_revision: 0,
                shown_at: None,
                phase_started: None,
                visible_duration: Duration::ZERO,
                ended_at: None,
                response: None,
                exit_code: None,
            },
        );
        self.dirty.insert(id);
        CreateResult {
            status: "accepted",
            request_id: id,
            state,
            revision: 1,
            duplicate: false,
        }
    }

    pub fn snapshot(&self, id: Uuid) -> Result<RequestSnapshot, RegistryError> {
        let r = self.records.get(&id).ok_or(RegistryError::NotFound)?;
        Ok(RequestSnapshot {
            request_id: id,
            state: r.state,
            revision: r.revision,
            mode: r.request.mode,
            requires_read: r.request.requires_read.unwrap_or(false),
            updatable: r.request.updatable,
            occurred_at: r.occurred_at,
            user_closed: r.user_closed,
            hidden_reason: r.hidden_reason,
            reshow_count: r.reshow_count,
            reshow_suppressed: r.reshow_suppressed,
            acknowledged_revision: r.acknowledged_revision,
            content: r.request.content.clone(),
            created_at: r.created_at,
            shown_at: r.shown_at,
            ended_at: r.ended_at,
            response: r.response.clone(),
            exit_code: r.exit_code,
        })
    }

    pub fn summary(&self, id: Uuid) -> Result<RequestSummary, RegistryError> {
        let r = self.records.get(&id).ok_or(RegistryError::NotFound)?;
        Ok(RequestSummary {
            request_id: id,
            state: r.state,
            revision: r.revision,
            title: r.request.content.title.clone(),
            shown_at: r.shown_at,
        })
    }

    pub fn request(&self, id: Uuid) -> Option<&Request> {
        self.records.get(&id).map(|r| &r.request)
    }
    pub fn template_dir(&self, id: Uuid) -> Option<&Path> {
        self.records.get(&id)?.template_dir.as_deref()
    }
    pub fn active_notification_count(&self) -> usize {
        self.notification_visible.len()
            + usize::from(self.notification_pending.is_some())
            + self.notification_queue.len()
    }
    pub fn visible_notification_ids(&self) -> Vec<Uuid> {
        self.notification_visible.iter().copied().collect()
    }
    pub fn pending_notification(&self) -> Option<Uuid> {
        self.notification_pending
    }
    pub fn next_notification(&self) -> Option<Uuid> {
        (self.notification_pending.is_none()
            && self.notification_visible.len() < MAX_VISIBLE_NOTIFICATIONS)
            .then(|| self.notification_queue.front().copied())
            .flatten()
    }
    pub fn next_dialog(&self) -> Option<Uuid> {
        self.active_dialog
            .is_none()
            .then(|| self.dialog_queue.front().copied())
            .flatten()
    }
    pub fn ready_ids(&self) -> Vec<Uuid> {
        self.active_dialog
            .into_iter()
            .chain(self.notification_pending)
            .filter(|id| {
                self.records
                    .get(id)
                    .is_some_and(|r| r.state == RequestState::Ready)
            })
            .collect()
    }
    pub fn active_notification_ids(&self) -> Vec<Uuid> {
        self.notification_visible
            .iter()
            .copied()
            .chain(self.notification_pending)
            .chain(self.notification_queue.iter().copied())
            .collect()
    }

    pub fn is_presented(&self, id: Uuid) -> bool {
        self.records
            .get(&id)
            .is_some_and(|r| r.state == RequestState::Shown)
    }

    pub fn is_preparing(&self, id: Uuid) -> bool {
        self.records
            .get(&id)
            .is_some_and(|r| r.state == RequestState::Preparing)
    }

    /// Start the presentation watchdog when the worker begins creating the
    /// window, never while a request is waiting for capacity or a worker turn.
    pub fn activate(&mut self, id: Uuid, now: Instant) -> bool {
        let Some(record) = self.records.get(&id) else {
            return false;
        };
        if record.state.is_terminal() || record.state == RequestState::Shown {
            return false;
        }
        match record.request.mode {
            Mode::Dialog if self.next_dialog() == Some(id) => {
                self.dialog_queue.pop_front();
                self.active_dialog = Some(id);
            }
            Mode::Notify if self.next_notification() == Some(id) => {
                self.notification_queue.pop_front();
                self.notification_pending = Some(id);
            }
            _ => return false,
        }
        let record = self.records.get_mut(&id).unwrap();
        if record.state == RequestState::Queued {
            record.state = RequestState::Preparing;
            record.phase_started = Some(now);
        }
        self.dirty.insert(id);
        true
    }

    /// Keep an existing HWND/content alive while a smaller work area cannot
    /// contain it. Older suspended windows stay ahead of pending newcomers.
    pub fn park_notification(&mut self, id: Uuid, now: Instant) -> Result<(), RegistryError> {
        let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
        if r.request.mode != Mode::Notify || r.state != RequestState::Shown {
            return Err(RegistryError::Terminal);
        }
        if let Some(started) = r.phase_started.take() {
            r.visible_duration += now.saturating_duration_since(started);
        }
        r.state = RequestState::Ready;
        self.dirty.insert(id);
        self.notification_visible.retain(|item| *item != id);
        if let Some(pending) = self.notification_pending.take() {
            self.notification_queue.push_front(pending);
        }
        self.notification_queue.push_front(id);
        Ok(())
    }

    /// Content readiness is not presentation: a ready window can wait hidden
    /// when a display change temporarily leaves insufficient space.
    pub fn mark_ready(&mut self, id: Uuid) -> Result<(), RegistryError> {
        let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
        match r.state {
            RequestState::Preparing => {
                r.state = RequestState::Ready;
                r.phase_started = None;
            }
            RequestState::Ready | RequestState::Shown => {}
            _ => return Err(RegistryError::Terminal),
        }
        self.dirty.insert(id);
        Ok(())
    }

    pub fn mark_shown(&mut self, id: Uuid, now: Instant) -> Result<(), RegistryError> {
        if !self.ready_ids().contains(&id) && !self.is_presented(id) {
            return Err(RegistryError::Terminal);
        }
        let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
        if r.state == RequestState::Shown {
            return Ok(());
        }
        if r.state != RequestState::Ready {
            return Err(RegistryError::Terminal);
        }
        r.state = RequestState::Shown;
        if r.reshow_pending {
            r.reshow_count += 1;
            r.reshow_pending = false;
        }
        r.hidden_reason = None;
        r.shown_at.get_or_insert_with(SystemTime::now);
        r.phase_started = Some(now);
        if r.request.mode == Mode::Notify {
            self.notification_pending = None;
            self.notification_visible.push_back(id);
        }
        self.dirty.insert(id);
        Ok(())
    }

    pub fn update(
        &mut self,
        id: Uuid,
        expected_revision: u64,
        content: Content,
    ) -> Result<Vec<Effect>, RegistryError> {
        self.update_with_options(id, expected_revision, content, UpdateOptions::default())
    }

    pub fn update_with_options(
        &mut self,
        id: Uuid,
        expected_revision: u64,
        content: Content,
        options: UpdateOptions,
    ) -> Result<Vec<Effect>, RegistryError> {
        let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
        if r.state == RequestState::Stale {
            return Err(RegistryError::NotificationExpired);
        }
        if r.state.is_terminal() {
            return Err(RegistryError::Terminal);
        }
        if r.request.mode == Mode::Notify && !r.request.updatable {
            return Err(RegistryError::NotUpdatable);
        }
        if r.revision != expected_revision {
            return Err(RegistryError::RevisionConflict {
                current_revision: r.revision,
            });
        }
        if r.request.mode == Mode::Dialog
            && r.state == RequestState::Shown
            && form_shape(&r.request.content) != form_shape(&content)
        {
            return Err(RegistryError::FormStructureChanged);
        }
        if r.request.mode == Mode::Dialog
            && (options.updatable.is_some() || options.reshow || options.duration_ms.is_some())
        {
            return Err(RegistryError::InvalidContent {
                message: "notification update options are not valid for dialog".into(),
            });
        }
        let mut next = r.request.clone();
        next.content = content;
        if let Some(updatable) = options.updatable {
            next.updatable = updatable;
        }
        if let Some(duration) = options.duration_ms {
            next.duration_ms = duration;
        }
        if let Err(error) = next.validate() {
            return Err(RegistryError::InvalidContent {
                message: error.to_string(),
            });
        }
        r.request = next;
        r.revision += 1;
        r.occurred_at = SystemTime::now();
        r.reshow_suppressed = options.reshow
            && r.user_closed
            && r.state == RequestState::Hidden
            && r.reshow_count >= MAX_USER_RESHOWS;
        if options.duration_ms.is_some() {
            r.visible_duration = Duration::ZERO;
            if r.state == RequestState::Shown {
                r.phase_started = Some(Instant::now());
            }
        }
        self.dirty.insert(id);
        if r.state == RequestState::Hidden {
            if !r.user_closed || (options.reshow && !r.reshow_suppressed) {
                r.state = RequestState::Queued;
                r.visible_duration = Duration::ZERO;
                r.phase_started = None;
                r.reshow_pending = r.user_closed;
                r.hidden_reason = None;
                self.notification_queue.push_back(id);
            } else if !r.request.updatable {
                // A new final result has not been acknowledged merely because
                // the user closed an earlier revision.
                return self.finish(id, RequestState::Closed, None, Some(0));
            }
        }
        if r.state == RequestState::Ready {
            r.state = RequestState::Preparing;
            r.phase_started = Some(Instant::now());
        }
        Ok(
            if matches!(
                r.state,
                RequestState::Preparing | RequestState::Ready | RequestState::Shown
            ) {
                vec![Effect::Update(id)]
            } else {
                vec![]
            },
        )
    }

    pub fn dismiss(&mut self, id: Uuid) -> Result<Vec<Effect>, RegistryError> {
        let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
        if r.state.is_terminal() {
            return Err(RegistryError::Terminal);
        }
        r.acknowledged_revision = r.revision;
        self.finish(id, RequestState::Cancelled, None, Some(10))
    }

    pub fn complete(
        &mut self,
        id: Uuid,
        response: Response,
        exit_code: i32,
    ) -> Result<Vec<Effect>, RegistryError> {
        let r = self.records.get(&id).ok_or(RegistryError::NotFound)?;
        if r.state.is_terminal() {
            return Err(RegistryError::Terminal);
        }
        if r.state == RequestState::Hidden {
            return Ok(vec![]);
        }
        let state = match response.status {
            ResponseStatus::Submitted | ResponseStatus::Confirmed => RequestState::Confirmed,
            ResponseStatus::Cancelled => RequestState::Cancelled,
            ResponseStatus::Closed => RequestState::Closed,
            ResponseStatus::TimedOut => RequestState::Expired,
        };
        if matches!(state, RequestState::Closed | RequestState::Cancelled) {
            if let Some(r) = self.records.get_mut(&id) {
                r.acknowledged_revision = r.revision;
            }
        }
        if self
            .records
            .get(&id)
            .is_some_and(|r| r.request.mode == Mode::Notify && r.request.updatable)
            && matches!(
                state,
                RequestState::Closed | RequestState::Cancelled | RequestState::Expired
            )
        {
            return self.hide_notification(id, state);
        }
        self.finish(id, state, Some(response), Some(exit_code))
    }

    fn hide_notification(
        &mut self,
        id: Uuid,
        reason: RequestState,
    ) -> Result<Vec<Effect>, RegistryError> {
        let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
        if r.state.is_terminal() {
            return Err(RegistryError::Terminal);
        }
        if r.state == RequestState::Hidden {
            return Ok(vec![]);
        }
        r.state = RequestState::Hidden;
        r.hidden_reason = Some(reason);
        r.user_closed |= matches!(reason, RequestState::Closed | RequestState::Cancelled);
        r.reshow_pending = false;
        r.phase_started = None;
        r.visible_duration = Duration::ZERO;
        self.notification_queue.retain(|x| *x != id);
        self.notification_visible.retain(|x| *x != id);
        if self.notification_pending == Some(id) {
            self.notification_pending = None;
        }
        self.dirty.insert(id);
        Ok(vec![Effect::Hide(id)])
    }

    pub fn fail(&mut self, id: Uuid) -> Result<Vec<Effect>, RegistryError> {
        self.finish(id, RequestState::Failed, None, Some(12))
    }

    pub fn expire_due(&mut self, now: Instant) -> Vec<Effect> {
        self.expire_due_at(now, SystemTime::now())
    }

    pub fn expire_due_at(&mut self, now: Instant, wall: SystemTime) -> Vec<Effect> {
        let due: Vec<_> = self
            .records
            .iter()
            .filter_map(|(id, r)| {
                if !r.state.is_terminal()
                    && r.request.updatable
                    && wall.duration_since(r.occurred_at).unwrap_or_default()
                        >= NOTIFICATION_IDLE_TIMEOUT
                {
                    return Some((*id, RequestState::Stale));
                }
                match (r.state, r.phase_started) {
                    (RequestState::Preparing, Some(started))
                        if now.saturating_duration_since(started) >= PRESENTATION_TIMEOUT =>
                    {
                        Some((*id, RequestState::Failed))
                    }
                    (RequestState::Shown, Some(shown)) => {
                        let ms = match r.request.mode {
                            Mode::Notify => r.request.duration_ms,
                            Mode::Dialog => r.request.timeout_seconds.saturating_mul(1000),
                        };
                        (r.visible_duration + now.saturating_duration_since(shown)
                            >= Duration::from_millis(ms))
                        .then_some((*id, RequestState::Expired))
                    }
                    _ => None,
                }
            })
            .collect();
        let mut effects = vec![];
        for (id, terminal) in due {
            if terminal == RequestState::Expired
                && self
                    .records
                    .get(&id)
                    .is_some_and(|r| r.request.mode == Mode::Notify && r.request.updatable)
            {
                if let Ok(next) = self.hide_notification(id, terminal) {
                    effects.extend(next);
                }
                continue;
            }
            let response = self.records.get(&id).and_then(|r| {
                (terminal == RequestState::Expired && r.request.mode == Mode::Dialog)
                    .then(|| r.request.terminal_response(ResponseStatus::TimedOut, None))
            });
            let exit_code = if terminal == RequestState::Failed {
                12
            } else {
                11
            };
            if let Ok(mut next) = self.finish(id, terminal, response, Some(exit_code)) {
                effects.append(&mut next);
            }
        }
        effects
    }

    pub fn shutdown(&mut self) -> Vec<Uuid> {
        let ids: Vec<_> = self
            .records
            .iter()
            .filter_map(|(id, r)| (!r.state.is_terminal() && !r.request.updatable).then_some(*id))
            .collect();
        for id in &ids {
            let _ = self.finish(*id, RequestState::Failed, None, Some(12));
        }
        ids
    }

    fn finish(
        &mut self,
        id: Uuid,
        state: RequestState,
        response: Option<Response>,
        exit_code: Option<i32>,
    ) -> Result<Vec<Effect>, RegistryError> {
        let (mode, had_window, key) = {
            let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
            if r.state.is_terminal() {
                return Err(RegistryError::Terminal);
            }
            let had_window = matches!(
                r.state,
                RequestState::Preparing
                    | RequestState::Ready
                    | RequestState::Shown
                    | RequestState::Hidden
            );
            r.state = state;
            r.request.updatable = false;
            r.ended_at = Some(SystemTime::now());
            r.response = response;
            r.exit_code = exit_code;
            r.phase_started = None;
            (r.request.mode, had_window, r.request.coalesce_key.clone())
        };
        self.dirty.insert(id);
        if let Some(key) = key {
            self.coalesced.remove(&(mode.into(), key));
        }
        let effects = if had_window {
            vec![Effect::Close(id)]
        } else {
            vec![]
        };
        match mode {
            Mode::Dialog => {
                self.dialog_queue.retain(|x| *x != id);
                if self.active_dialog == Some(id) {
                    self.active_dialog = None;
                }
            }
            Mode::Notify => {
                self.notification_queue.retain(|x| *x != id);
                self.notification_visible.retain(|x| *x != id);
                if self.notification_pending == Some(id) {
                    self.notification_pending = None;
                }
            }
        }
        Ok(effects)
    }

    pub fn hidden_notification_ids(&self) -> Vec<Uuid> {
        self.records
            .iter()
            .filter_map(|(id, r)| (r.state == RequestState::Hidden).then_some(*id))
            .collect()
    }

    pub fn clear_hidden(&mut self) -> Vec<Uuid> {
        let ids = self.hidden_notification_ids();
        for id in &ids {
            let _ = self.finish(*id, RequestState::Cancelled, None, Some(10));
            self.records.remove(id);
            self.dirty.remove(id);
        }
        ids
    }

    pub fn take_dirty(&mut self) -> Vec<Uuid> {
        self.dirty.drain().collect()
    }
    pub fn mark_dirty(&mut self, id: Uuid) {
        self.dirty.insert(id);
    }

    pub fn checkpoint(&self, id: Uuid) -> Option<String> {
        let r = self.records.get(&id)?;
        (r.request.mode == Mode::Notify && !r.state.is_terminal())
            .then(|| serde_json::to_string(r).unwrap())
    }

    /// Restart preserves identity and user intent, without popping up windows.
    pub fn restore(&mut self, id: Uuid, json: &str) -> Result<(), serde_json::Error> {
        let mut record: Record = serde_json::from_str(json)?;
        record.state = RequestState::Hidden;
        record.phase_started = None;
        record.visible_duration = Duration::ZERO;
        record.reshow_pending = false;
        record.hidden_reason.get_or_insert(RequestState::Expired);
        if let Some(key) = &record.request.coalesce_key {
            self.coalesced.insert((ModeKey::Notify, key.clone()), id);
        }
        let sealed = !record.request.updatable;
        self.records.insert(id, record);
        self.dirty.insert(id);
        if sealed {
            let _ = self.finish(id, RequestState::Expired, None, Some(11));
        }
        Ok(())
    }
}

fn form_shape(content: &Content) -> Value {
    Value::Array(
        content
            .blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Form { title, fields } => {
                    Some(serde_json::json!({"title": title, "fields": fields}))
                }
                _ => None,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(mode: &str, key: Option<&str>) -> Request {
        let actions = if mode == "dialog" {
            r#", "actions":[{"id":"ok","label":"OK","submit":true}]"#
        } else {
            ""
        };
        let key = key
            .map(|k| format!(r#", "coalesce_key":"{k}""#))
            .unwrap_or_default();
        Request::parse(&format!(
            r#"{{"version":"1.0","mode":"{mode}"{key},"content":{{"title":"T"}}{actions}}}"#
        ))
        .unwrap()
    }
    fn show(r: &mut RequestRegistry, id: Uuid, now: Instant) {
        assert!(r.activate(id, now));
        r.mark_ready(id).unwrap();
        r.mark_shown(id, now).unwrap();
    }

    #[test]
    fn accepted_requests_are_queued_and_dialogs_are_independent() {
        let mut r = RequestRegistry::default();
        let a = r.create(req("notify", None), None).request_id;
        let b = r.create(req("notify", None), None).request_id;
        let dialog = r.create(req("dialog", None), None).request_id;
        let now = Instant::now();
        assert_eq!(r.snapshot(a).unwrap().state, RequestState::Queued);
        assert!(r.activate(a, now));
        assert!(!r.activate(b, now));
        assert!(r.activate(dialog, now));
        assert!(r.visible_notification_ids().is_empty());
        assert_eq!(r.active_notification_count(), 2);
    }

    #[test]
    fn fifo_head_cannot_be_bypassed_even_when_no_window_is_preparing() {
        let mut r = RequestRegistry::default();
        let a = r.create(req("notify", None), None).request_id;
        let b = r.create(req("notify", None), None).request_id;
        assert_eq!(r.next_notification(), Some(a));
        assert!(!r.activate(b, Instant::now()));
        assert_eq!(r.next_notification(), Some(a));
        r.dismiss(a).unwrap();
        assert_eq!(r.next_notification(), Some(b));
    }

    #[test]
    fn visible_capacity_is_three_and_finishing_does_not_prematurely_promote() {
        let mut r = RequestRegistry::default();
        let ids: Vec<_> = (0..4)
            .map(|_| r.create(req("notify", None), None).request_id)
            .collect();
        let now = Instant::now();
        for id in &ids[..3] {
            show(&mut r, *id, now);
        }
        assert_eq!(r.next_notification(), None);
        assert_eq!(r.dismiss(ids[0]).unwrap(), [Effect::Close(ids[0])]);
        assert_eq!(r.visible_notification_ids(), ids[1..3]);
        assert_eq!(r.next_notification(), Some(ids[3]));
        assert_eq!(r.snapshot(ids[3]).unwrap().state, RequestState::Queued);
        show(&mut r, ids[3], now);
        assert_eq!(r.visible_notification_ids(), ids[1..]);
    }

    #[test]
    fn clocks_are_separate_and_ready_is_neither_visible_nor_preparation_timeout() {
        let mut r = RequestRegistry::default();
        let id = r.create(req("notify", None), None).request_id;
        let now = Instant::now();
        assert!(r.expire_due(now + Duration::from_secs(1000)).is_empty());
        assert!(r.mark_shown(id, now).is_err());
        assert!(r.activate(id, now));
        r.mark_ready(id).unwrap();
        assert!(r.expire_due(now + Duration::from_secs(1000)).is_empty());
        assert!(r.visible_notification_ids().is_empty());
        let shown = now + Duration::from_secs(1000);
        r.mark_shown(id, shown).unwrap();
        r.mark_ready(id).unwrap();
        r.mark_shown(id, shown + Duration::from_secs(7)).unwrap();
        assert!(r.expire_due(shown + Duration::from_secs(7)).is_empty());
        assert_eq!(
            r.expire_due(shown + Duration::from_secs(8)),
            [Effect::Close(id)]
        );
        assert_eq!(r.snapshot(id).unwrap().state, RequestState::Expired);
    }

    #[test]
    fn preparation_timeout_releases_fifo_and_late_callbacks_cannot_revive_it() {
        let mut r = RequestRegistry::default();
        let a = r.create(req("notify", None), None).request_id;
        let b = r.create(req("notify", None), None).request_id;
        let now = Instant::now();
        assert!(r.activate(a, now));
        assert!(r
            .expire_due(now + PRESENTATION_TIMEOUT - Duration::from_millis(1))
            .is_empty());
        assert_eq!(r.expire_due(now + PRESENTATION_TIMEOUT), [Effect::Close(a)]);
        assert_eq!(r.snapshot(a).unwrap().shown_at, None);
        assert_eq!(r.snapshot(a).unwrap().exit_code, Some(12));
        assert!(r.mark_ready(a).is_err());
        assert!(r.mark_shown(a, now).is_err());
        assert!(!r.activate(a, now));
        assert_eq!(r.next_notification(), Some(b));
    }

    #[test]
    fn revision_updates_do_not_reset_presentation_lifetime() {
        let mut r = RequestRegistry::default();
        let mut request = req("notify", None);
        request.updatable = true;
        let id = r.create(request, None).request_id;
        let mut content = r.request(id).unwrap().content.clone();
        content.title = "Updated".into();
        assert!(r.update(id, 1, content.clone()).unwrap().is_empty());
        assert!(matches!(
            r.update(id, 1, content.clone()),
            Err(RegistryError::RevisionConflict {
                current_revision: 2
            })
        ));
        let now = Instant::now();
        show(&mut r, id, now);
        assert_eq!(r.update(id, 2, content).unwrap(), [Effect::Update(id)]);
        assert_eq!(
            r.expire_due(now + Duration::from_secs(8)),
            [Effect::Hide(id)]
        );
    }

    #[test]
    fn coalescing_is_mode_scoped_and_terminal_requests_release_the_key() {
        let mut r = RequestRegistry::default();
        let a = r.create(req("notify", Some("job")), None);
        let b = r.create(req("notify", Some("job")), None);
        let dialog = r.create(req("dialog", Some("job")), None);
        assert!(b.duplicate);
        assert_eq!(a.request_id, b.request_id);
        assert_ne!(a.request_id, dialog.request_id);
        r.dismiss(a.request_id).unwrap();
        assert!(!r.create(req("notify", Some("job")), None).duplicate);
    }

    #[test]
    fn repeated_failures_and_closes_leave_no_membership_or_stale_coalescing() {
        let mut r = RequestRegistry::default();
        let now = Instant::now();
        for _ in 0..100 {
            let a = r.create(req("notify", Some("test")), None).request_id;
            let b = r.create(req("notify", None), None).request_id;
            r.activate(a, now);
            r.expire_due(now + PRESENTATION_TIMEOUT);
            show(&mut r, b, now);
            r.dismiss(b).unwrap();
            assert_eq!(r.active_notification_count(), 0);
            assert!(r.visible_notification_ids().is_empty());
            assert_eq!(r.pending_notification(), None);
            assert_eq!(r.next_notification(), None);
        }
        assert!(!r.create(req("notify", Some("test")), None).duplicate);
    }

    #[test]
    fn shrinking_the_display_parks_in_fifo_order_and_preserves_remaining_time() {
        let mut r = RequestRegistry::default();
        let ids: Vec<_> = (0..4)
            .map(|_| r.create(req("notify", None), None).request_id)
            .collect();
        let now = Instant::now();
        show(&mut r, ids[0], now);
        show(&mut r, ids[1], now);
        assert!(r.activate(ids[2], now));
        r.park_notification(ids[1], now + Duration::from_secs(3))
            .unwrap();
        assert_eq!(r.visible_notification_ids(), [ids[0]]);
        assert_eq!(r.next_notification(), Some(ids[1]));
        assert!(r.mark_shown(ids[1], now).is_err());
        assert!(r.activate(ids[1], now + Duration::from_secs(100)));
        r.mark_shown(ids[1], now + Duration::from_secs(100))
            .unwrap();
        assert_eq!(r.next_notification(), Some(ids[2]));
        assert!(r
            .expire_due(now + Duration::from_secs(104))
            .contains(&Effect::Close(ids[0])));
        assert!(r.is_presented(ids[1]));
        assert!(r
            .expire_due(now + Duration::from_secs(105))
            .contains(&Effect::Close(ids[1])));
    }

    #[test]
    fn updating_ready_content_requires_a_new_acknowledgement() {
        let mut r = RequestRegistry::default();
        let mut request = req("notify", None);
        request.updatable = true;
        let id = r.create(request, None).request_id;
        r.activate(id, Instant::now());
        r.mark_ready(id).unwrap();
        let content = r.request(id).unwrap().content.clone();
        assert_eq!(r.update(id, 1, content).unwrap(), [Effect::Update(id)]);
        assert_eq!(r.snapshot(id).unwrap().state, RequestState::Preparing);
        assert!(r.mark_shown(id, Instant::now()).is_err());
        r.mark_ready(id).unwrap();
        r.mark_shown(id, Instant::now()).unwrap();
    }

    fn live(r: &mut RequestRegistry) -> Uuid {
        let mut request = req("notify", Some("backup"));
        request.updatable = true;
        r.create(request, None).request_id
    }

    fn close_user(r: &mut RequestRegistry, id: Uuid) {
        let response = r
            .request(id)
            .unwrap()
            .terminal_response(ResponseStatus::Closed, None);
        r.complete(id, response, 10).unwrap();
    }

    fn change(r: &mut RequestRegistry, id: Uuid, options: UpdateOptions) {
        let snapshot = r.snapshot(id).unwrap();
        r.update_with_options(id, snapshot.revision, snapshot.content, options)
            .unwrap();
    }

    #[test]
    fn user_close_silences_progress_and_allows_exactly_one_explicit_reshow() {
        let mut r = RequestRegistry::default();
        let id = live(&mut r);
        show(&mut r, id, Instant::now());
        close_user(&mut r, id);
        assert_eq!(r.snapshot(id).unwrap().state, RequestState::Hidden);
        assert!(r.create(r.request(id).unwrap().clone(), None).duplicate);
        change(&mut r, id, UpdateOptions::default());
        assert_eq!(r.snapshot(id).unwrap().state, RequestState::Hidden);
        assert_eq!(r.active_notification_count(), 0);
        change(
            &mut r,
            id,
            UpdateOptions {
                reshow: true,
                ..Default::default()
            },
        );
        change(
            &mut r,
            id,
            UpdateOptions {
                reshow: true,
                ..Default::default()
            },
        );
        assert_eq!(r.snapshot(id).unwrap().reshow_count, 0);
        assert_eq!(r.active_notification_count(), 1);
        show(&mut r, id, Instant::now());
        assert_eq!(r.snapshot(id).unwrap().reshow_count, 1);
        close_user(&mut r, id);
        change(
            &mut r,
            id,
            UpdateOptions {
                reshow: true,
                updatable: Some(false),
                ..Default::default()
            },
        );
        let final_snapshot = r.snapshot(id).unwrap();
        assert!(final_snapshot.reshow_suppressed);
        assert!(final_snapshot.state.is_terminal());
        assert!(final_snapshot.acknowledged_revision < final_snapshot.revision);
        assert!(matches!(
            r.update(id, final_snapshot.revision, final_snapshot.content),
            Err(RegistryError::Terminal)
        ));
    }

    #[test]
    fn automatic_hiding_reappears_on_update_without_using_user_budget() {
        let mut r = RequestRegistry::default();
        let id = live(&mut r);
        for _ in 0..3 {
            let now = Instant::now();
            show(&mut r, id, now);
            assert_eq!(
                r.expire_due(now + Duration::from_secs(8)),
                [Effect::Hide(id)]
            );
            assert_eq!(
                r.snapshot(id).unwrap().hidden_reason,
                Some(RequestState::Expired)
            );
            change(&mut r, id, UpdateOptions::default());
            assert_eq!(r.snapshot(id).unwrap().state, RequestState::Queued);
            assert_eq!(r.snapshot(id).unwrap().reshow_count, 0);
        }
        show(&mut r, id, Instant::now());
        close_user(&mut r, id);
        change(
            &mut r,
            id,
            UpdateOptions {
                reshow: true,
                ..Default::default()
            },
        );
        let now = Instant::now();
        show(&mut r, id, now);
        r.expire_due(now + Duration::from_secs(8));
        change(&mut r, id, UpdateOptions::default());
        assert_eq!(r.snapshot(id).unwrap().state, RequestState::Hidden);
    }

    #[test]
    fn final_update_seals_atomically_and_gets_its_own_display_duration() {
        let mut r = RequestRegistry::default();
        let id = live(&mut r);
        show(&mut r, id, Instant::now());
        close_user(&mut r, id);
        change(
            &mut r,
            id,
            UpdateOptions {
                updatable: Some(false),
                reshow: true,
                duration_ms: Some(10000),
            },
        );
        let now = Instant::now();
        show(&mut r, id, now);
        let final_snapshot = r.snapshot(id).unwrap();
        assert!(!final_snapshot.updatable);
        assert!(matches!(
            r.update(id, final_snapshot.revision, final_snapshot.content),
            Err(RegistryError::NotUpdatable)
        ));
        assert!(r.expire_due(now + Duration::from_secs(9)).is_empty());
        assert_eq!(
            r.expire_due(now + Duration::from_secs(10)),
            [Effect::Close(id)]
        );
    }

    #[test]
    fn duration_override_restarts_a_visible_timer_but_invalid_updates_change_nothing() {
        let mut r = RequestRegistry::default();
        let id = live(&mut r);
        show(&mut r, id, Instant::now() - Duration::from_secs(7));
        change(
            &mut r,
            id,
            UpdateOptions {
                duration_ms: Some(10000),
                ..Default::default()
            },
        );
        let started = r.records[&id].phase_started.unwrap();
        assert!(r.expire_due(started + Duration::from_secs(9)).is_empty());
        let before = r.snapshot(id).unwrap();
        assert!(r
            .update_with_options(
                id,
                before.revision,
                before.content.clone(),
                UpdateOptions {
                    duration_ms: Some(1),
                    updatable: Some(false),
                    reshow: true
                }
            )
            .is_err());
        let after = r.snapshot(id).unwrap();
        assert_eq!(before.revision, after.revision);
        assert_eq!(before.occurred_at, after.occurred_at);
        assert!(after.updatable);
        assert_eq!(
            r.expire_due(started + Duration::from_secs(10)),
            [Effect::Hide(id)]
        );
    }

    #[test]
    fn one_shot_notifications_reject_updates_even_while_visible() {
        let mut r = RequestRegistry::default();
        let id = r.create(req("notify", None), None).request_id;
        let content = r.snapshot(id).unwrap().content;
        assert!(matches!(
            r.update(id, 1, content),
            Err(RegistryError::NotUpdatable)
        ));
    }

    #[test]
    fn idle_timeout_uses_last_successful_report_and_survives_restart() {
        let mut r = RequestRegistry::default();
        let id = live(&mut r);
        r.records.get_mut(&id).unwrap().occurred_at = SystemTime::now() - NOTIFICATION_IDLE_TIMEOUT;
        change(&mut r, id, UpdateOptions::default());
        let occurred = r.snapshot(id).unwrap().occurred_at;
        let checkpoint = r.checkpoint(id).unwrap();
        let mut restored = RequestRegistry::default();
        restored.restore(id, &checkpoint).unwrap();
        assert!(restored
            .expire_due_at(
                Instant::now(),
                occurred + NOTIFICATION_IDLE_TIMEOUT - Duration::from_secs(1)
            )
            .is_empty());
        restored.expire_due_at(Instant::now(), occurred + NOTIFICATION_IDLE_TIMEOUT);
        let snapshot = restored.snapshot(id).unwrap();
        assert_eq!(snapshot.state, RequestState::Stale);
        assert!(matches!(
            restored.update(id, snapshot.revision, snapshot.content),
            Err(RegistryError::NotificationExpired)
        ));
        assert!(restored.checkpoint(id).is_none());
    }

    #[test]
    fn restart_retains_user_close_and_consumed_reshow_budget() {
        let mut r = RequestRegistry::default();
        let id = live(&mut r);
        show(&mut r, id, Instant::now());
        close_user(&mut r, id);
        change(
            &mut r,
            id,
            UpdateOptions {
                reshow: true,
                ..Default::default()
            },
        );
        show(&mut r, id, Instant::now());
        let checkpoint = r.checkpoint(id).unwrap();
        let mut restored = RequestRegistry::default();
        restored.restore(id, &checkpoint).unwrap();
        change(
            &mut restored,
            id,
            UpdateOptions {
                reshow: true,
                ..Default::default()
            },
        );
        let snapshot = restored.snapshot(id).unwrap();
        assert!(snapshot.reshow_suppressed);
        assert!(snapshot.user_closed);
        assert_eq!(snapshot.reshow_count, 1);
        assert_eq!(restored.active_notification_count(), 0);
    }

    #[test]
    fn clearing_hidden_records_rejects_late_updates_and_preserves_queued_work() {
        let mut r = RequestRegistry::default();
        let id = live(&mut r);
        show(&mut r, id, Instant::now());
        close_user(&mut r, id);
        let queued = r.create(req("notify", None), None).request_id;
        assert_eq!(r.clear_hidden(), [id]);
        assert!(matches!(r.snapshot(id), Err(RegistryError::NotFound)));
        assert_eq!(r.next_notification(), Some(queued));
    }

    #[test]
    fn duplicate_close_cannot_acknowledge_silent_new_content() {
        let mut r = RequestRegistry::default();
        let id = live(&mut r);
        show(&mut r, id, Instant::now());
        close_user(&mut r, id);
        change(&mut r, id, UpdateOptions::default());
        close_user(&mut r, id);
        assert_eq!(r.snapshot(id).unwrap().acknowledged_revision, 1);
        change(
            &mut r,
            id,
            UpdateOptions {
                updatable: Some(false),
                ..Default::default()
            },
        );
        let response = r
            .request(id)
            .unwrap()
            .terminal_response(ResponseStatus::Closed, None);
        assert!(r.complete(id, response, 10).is_err());
        assert!(r.dismiss(id).is_err());
        assert_eq!(r.snapshot(id).unwrap().acknowledged_revision, 1);
    }
}
