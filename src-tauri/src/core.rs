//! Tauri-independent request registry and scheduler.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::protocol::{Content, ContentBlock, Mode, Request, Response, ResponseStatus};

pub const MAX_VISIBLE_NOTIFICATIONS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestState { Queued, Shown, Confirmed, Cancelled, Closed, Expired, Failed }

impl RequestState {
    pub fn is_terminal(self) -> bool { !matches!(self, Self::Queued | Self::Shown) }
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestSnapshot {
    pub request_id: Uuid,
    pub state: RequestState,
    pub revision: u64,
    pub mode: Mode,
    pub requires_read: bool,
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
pub struct CreateResult {
    pub status: &'static str,
    pub request_id: Uuid,
    pub state: RequestState,
    pub revision: u64,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect { Show(Uuid), Update(Uuid), Close(Uuid) }

#[derive(Debug, Error, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum RegistryError {
    #[error("request was not found")]
    NotFound,
    #[error("request is already terminal")]
    Terminal,
    #[error("revision conflict (current revision: {current_revision})")]
    RevisionConflict { current_revision: u64 },
    #[error("a shown dialog's form structure cannot be changed")]
    FormStructureChanged,
    #[error("updated content is invalid: {message}")]
    InvalidContent { message: String },
}

struct Record {
    request: Request,
    template_dir: Option<PathBuf>,
    state: RequestState,
    revision: u64,
    created_at: SystemTime,
    shown_at: Option<SystemTime>,
    shown_instant: Option<Instant>,
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
    visible_notifications: VecDeque<Uuid>,
    coalesced: HashMap<(ModeKey, String), Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ModeKey { Dialog, Notify }
impl From<Mode> for ModeKey {
    fn from(value: Mode) -> Self { match value { Mode::Dialog => Self::Dialog, Mode::Notify => Self::Notify } }
}

impl RequestRegistry {
    pub fn create(&mut self, request: Request, template_dir: Option<PathBuf>) -> (CreateResult, Vec<Effect>) {
        if let Some(key) = request.coalesce_key.as_ref() {
            if let Some(id) = self.coalesced.get(&(request.mode.into(), key.clone())).copied() {
                if let Some(record) = self.records.get(&id).filter(|r| !r.state.is_terminal()) {
                    return (CreateResult { status: "accepted", request_id: id, state: record.state,
                        revision: record.revision, duplicate: true }, vec![]);
                }
            }
        }
        let id = Uuid::new_v4();
        let can_show = match request.mode {
            Mode::Dialog => self.active_dialog.is_none(),
            Mode::Notify => self.visible_notifications.len() < MAX_VISIBLE_NOTIFICATIONS,
        };
        let state = if can_show { RequestState::Shown } else { RequestState::Queued };
        if let Some(key) = request.coalesce_key.clone() { self.coalesced.insert((request.mode.into(), key), id); }
        match (request.mode, state) {
            (Mode::Dialog, RequestState::Shown) => self.active_dialog = Some(id),
            (Mode::Dialog, _) => self.dialog_queue.push_back(id),
            (Mode::Notify, RequestState::Shown) => self.visible_notifications.push_back(id),
            (Mode::Notify, _) => self.notification_queue.push_back(id),
        }
        self.records.insert(id, Record { request, template_dir, state, revision: 1,
            created_at: SystemTime::now(), shown_at: None, shown_instant: None,
            ended_at: None, response: None, exit_code: None });
        (CreateResult { status: "accepted", request_id: id, state, revision: 1, duplicate: false },
            if can_show { vec![Effect::Show(id)] } else { vec![] })
    }

    pub fn snapshot(&self, id: Uuid) -> Result<RequestSnapshot, RegistryError> {
        let r = self.records.get(&id).ok_or(RegistryError::NotFound)?;
        Ok(RequestSnapshot { request_id: id, state: r.state, revision: r.revision,
            mode: r.request.mode, requires_read: r.request.requires_read.unwrap_or(false),
            content: r.request.content.clone(), created_at: r.created_at,
            shown_at: r.shown_at, ended_at: r.ended_at, response: r.response.clone(), exit_code: r.exit_code })
    }

    pub fn request(&self, id: Uuid) -> Option<&Request> { self.records.get(&id).map(|r| &r.request) }
    pub fn template_dir(&self, id: Uuid) -> Option<&Path> { self.records.get(&id)?.template_dir.as_deref() }
    pub fn active_notification_count(&self) -> usize { self.visible_notifications.len() + self.notification_queue.len() }
    pub fn notification_slot(&self, id: Uuid) -> Option<usize> { self.visible_notifications.iter().position(|item| *item == id) }
    pub fn visible_notification_ids(&self) -> Vec<Uuid> { self.visible_notifications.iter().copied().collect() }
    pub fn active_notification_ids(&self) -> Vec<Uuid> {
        self.visible_notifications.iter().chain(self.notification_queue.iter()).copied().collect()
    }

    /// Returns a notification that was provisionally marked as shown to the
    /// FIFO queue when the host cannot fit it in the target work area.
    pub fn defer_notification(&mut self, id: Uuid) -> Result<(), RegistryError> {
        {
            let record = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
            if record.request.mode != Mode::Notify || record.state != RequestState::Shown {
                return Err(RegistryError::Terminal);
            }
            record.state = RequestState::Queued;
        }
        self.visible_notifications.retain(|item| *item != id);
        self.notification_queue.push_back(id);
        Ok(())
    }

    pub fn mark_shown(&mut self, id: Uuid, now: Instant) -> Result<(), RegistryError> {
        let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
        if r.state != RequestState::Shown { return Err(RegistryError::Terminal); }
        r.shown_at.get_or_insert_with(SystemTime::now);
        r.shown_instant.get_or_insert(now);
        Ok(())
    }

    pub fn update(&mut self, id: Uuid, expected_revision: u64, content: Content) -> Result<Vec<Effect>, RegistryError> {
        let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
        if r.state.is_terminal() { return Err(RegistryError::Terminal); }
        if r.revision != expected_revision { return Err(RegistryError::RevisionConflict { current_revision: r.revision }); }
        if r.request.mode == Mode::Dialog && r.state == RequestState::Shown && form_shape(&r.request.content) != form_shape(&content) {
            return Err(RegistryError::FormStructureChanged);
        }
        let old = std::mem::replace(&mut r.request.content, content);
        if let Err(error) = r.request.validate() {
            r.request.content = old;
            return Err(RegistryError::InvalidContent { message: error.to_string() });
        }
        r.revision += 1;
        Ok(if r.state == RequestState::Shown { vec![Effect::Update(id)] } else { vec![] })
    }

    pub fn dismiss(&mut self, id: Uuid) -> Result<Vec<Effect>, RegistryError> {
        self.finish(id, RequestState::Cancelled, None, Some(10))
    }

    pub fn complete(&mut self, id: Uuid, response: Response, exit_code: i32) -> Result<Vec<Effect>, RegistryError> {
        let state = match response.status {
            ResponseStatus::Submitted | ResponseStatus::Confirmed => RequestState::Confirmed,
            ResponseStatus::Cancelled => RequestState::Cancelled,
            ResponseStatus::Closed => RequestState::Closed,
            ResponseStatus::TimedOut => RequestState::Expired,
        };
        self.finish(id, state, Some(response), Some(exit_code))
    }

    pub fn fail(&mut self, id: Uuid) -> Result<Vec<Effect>, RegistryError> {
        self.finish(id, RequestState::Failed, None, Some(12))
    }

    pub fn expire_due(&mut self, now: Instant) -> Vec<Effect> {
        let due: Vec<_> = self.records.iter().filter_map(|(id, r)| {
            let shown = r.shown_instant?;
            let ms = match r.request.mode { Mode::Notify => r.request.duration_ms, Mode::Dialog => r.request.timeout_seconds.saturating_mul(1000) };
            (r.state == RequestState::Shown && now.duration_since(shown) >= Duration::from_millis(ms)).then_some(*id)
        }).collect();
        let mut effects = vec![];
        for id in due {
            let response = self.records.get(&id).and_then(|r| (r.request.mode == Mode::Dialog)
                .then(|| r.request.terminal_response(ResponseStatus::TimedOut, None)));
            if let Ok(mut next) = self.finish(id, RequestState::Expired, response, Some(11)) { effects.append(&mut next); }
        }
        effects
    }

    pub fn shutdown(&mut self) -> Vec<Uuid> {
        let ids: Vec<_> = self.records.iter().filter_map(|(id, r)| (!r.state.is_terminal()).then_some(*id)).collect();
        for id in &ids { let _ = self.finish(*id, RequestState::Failed, None, Some(12)); }
        ids
    }

    fn finish(&mut self, id: Uuid, state: RequestState, response: Option<Response>, exit_code: Option<i32>) -> Result<Vec<Effect>, RegistryError> {
        let (mode, was_shown, key) = {
            let r = self.records.get_mut(&id).ok_or(RegistryError::NotFound)?;
            if r.state.is_terminal() { return Err(RegistryError::Terminal); }
            let was_shown = r.state == RequestState::Shown;
            r.state = state; r.ended_at = Some(SystemTime::now()); r.response = response; r.exit_code = exit_code;
            (r.request.mode, was_shown, r.request.coalesce_key.clone())
        };
        if let Some(key) = key { self.coalesced.remove(&(mode.into(), key)); }
        let mut effects = if was_shown { vec![Effect::Close(id)] } else { vec![] };
        match mode {
            Mode::Dialog => {
                self.dialog_queue.retain(|x| *x != id);
                if self.active_dialog == Some(id) { self.active_dialog = None; }
                if self.active_dialog.is_none() {
                    if let Some(next) = self.dialog_queue.pop_front() { self.active_dialog = Some(next); self.records.get_mut(&next).unwrap().state = RequestState::Shown; effects.push(Effect::Show(next)); }
                }
            }
            Mode::Notify => {
                self.notification_queue.retain(|x| *x != id);
                self.visible_notifications.retain(|x| *x != id);
                while self.visible_notifications.len() < MAX_VISIBLE_NOTIFICATIONS {
                    let Some(next) = self.notification_queue.pop_front() else { break };
                    self.visible_notifications.push_back(next); self.records.get_mut(&next).unwrap().state = RequestState::Shown; effects.push(Effect::Show(next));
                }
            }
        }
        Ok(effects)
    }
}

fn form_shape(content: &Content) -> Value {
    Value::Array(content.blocks.iter().filter_map(|block| match block {
        ContentBlock::Form { title, fields } => Some(serde_json::json!({"title": title, "fields": fields})),
        _ => None,
    }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn req(mode: &str, key: Option<&str>) -> Request {
        let actions = if mode == "dialog" { r#", "actions":[{"id":"ok","label":"OK","submit":true}]"# } else { "" };
        let key = key.map(|k| format!(r#", "coalesce_key":"{k}""#)).unwrap_or_default();
        Request::parse(&format!(r#"{{"version":"1.0","mode":"{mode}"{key},"content":{{"title":"T"}}{actions}}}"#)).unwrap()
    }
    #[test]
    fn dialog_fifo_and_notification_limit_are_independent() {
        let mut r = RequestRegistry::default();
        let (d1, _) = r.create(req("dialog", None), None); let (d2, _) = r.create(req("dialog", None), None);
        assert_eq!((d1.state, d2.state), (RequestState::Shown, RequestState::Queued));
        let notifications: Vec<_> = (0..4).map(|_| r.create(req("notify", None), None).0).collect();
        assert_eq!(notifications[2].state, RequestState::Shown); assert_eq!(notifications[3].state, RequestState::Queued);
        assert!(r.dismiss(d1.request_id).unwrap().contains(&Effect::Show(d2.request_id)));
    }
    #[test]
    fn closing_the_bottom_notification_promotes_fifo_and_reindexes_visible_slots() {
        let mut r = RequestRegistry::default();
        let notifications: Vec<_> = (0..4).map(|_| r.create(req("notify", None), None).0).collect();

        let effects = r.dismiss(notifications[0].request_id).unwrap();

        assert_eq!(effects, vec![Effect::Close(notifications[0].request_id), Effect::Show(notifications[3].request_id)]);
        assert_eq!(r.visible_notification_ids(), notifications[1..].iter().map(|notification| notification.request_id).collect::<Vec<_>>());
        assert_eq!(r.notification_slot(notifications[1].request_id), Some(0));
    }
    #[test]
    fn coalesce_is_mode_scoped_and_released_at_terminal() {
        let mut r = RequestRegistry::default();
        let (a, _) = r.create(req("notify", Some("job")), None); let (b, _) = r.create(req("notify", Some("job")), None);
        let (dialog, _) = r.create(req("dialog", Some("job")), None);
        assert!(b.duplicate); assert_eq!(a.request_id, b.request_id); assert_ne!(a.request_id, dialog.request_id);
        r.dismiss(a.request_id).unwrap(); assert!(!r.create(req("notify", Some("job")), None).0.duplicate);
    }
    #[test]
    fn revision_and_timer_start_are_enforced() {
        let mut r = RequestRegistry::default(); let (a, _) = r.create(req("notify", None), None);
        let mut content = r.request(a.request_id).unwrap().content.clone(); content.title = "New".into();
        r.update(a.request_id, 1, content.clone()).unwrap();
        assert!(matches!(r.update(a.request_id, 1, content), Err(RegistryError::RevisionConflict { current_revision: 2 })));
        let start = Instant::now(); assert!(r.expire_due(start + Duration::from_secs(60)).is_empty());
        r.mark_shown(a.request_id, start).unwrap(); assert!(!r.expire_due(start + Duration::from_secs(9)).is_empty());
    }
    #[test]
    fn deferring_a_notification_releases_its_visible_slot() {
        let mut r = RequestRegistry::default();
        let (notification, effects) = r.create(req("notify", None), None);
        assert_eq!(effects, vec![Effect::Show(notification.request_id)]);

        r.defer_notification(notification.request_id).unwrap();

        assert_eq!(r.snapshot(notification.request_id).unwrap().state, RequestState::Queued);
        assert_eq!(r.notification_slot(notification.request_id), None);
        assert_eq!(r.active_notification_ids(), vec![notification.request_id]);
    }
}
