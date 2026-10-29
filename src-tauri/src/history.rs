use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::core::{RequestSnapshot, RequestState};
use crate::protocol::Mode;

pub struct HistoryStore { connection: Connection }

#[derive(Serialize)]
pub struct HistoryItem {
    pub request_id: String,
    pub state: String,
    pub revision: u64,
    pub content: serde_json::Value,
    pub created_at_ms: i64,
    pub shown_at_ms: Option<i64>,
    pub ended_at_ms: i64,
    pub response: Option<serde_json::Value>,
    pub requires_read: bool,
    pub read_at_ms: Option<i64>,
}

impl HistoryStore {
    pub fn open_default() -> rusqlite::Result<Self> {
        let path = default_path();
        if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
        Self::from_connection(Connection::open(path)?)
    }

    pub(crate) fn from_connection(mut connection: Connection) -> rusqlite::Result<Self> {
        let transaction = connection.transaction()?;
        transaction.execute_batch("CREATE TABLE IF NOT EXISTS notification_history (
          request_id TEXT PRIMARY KEY, state TEXT NOT NULL, revision INTEGER NOT NULL,
          content_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL, shown_at_ms INTEGER,
          ended_at_ms INTEGER NOT NULL, response_json TEXT,
          requires_read INTEGER NOT NULL DEFAULT 0, read_at_ms INTEGER
        ); CREATE INDEX IF NOT EXISTS notification_history_ended ON notification_history(ended_at_ms DESC);")?;
        // Existing contents acquire the new defaults; no legacy attention is inferred.
        let columns = {
            let mut statement = transaction.prepare("PRAGMA table_info(notification_history)")?;
            let columns = statement.query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            columns
        };
        if !columns.iter().any(|name| name == "requires_read") {
            transaction.execute_batch("ALTER TABLE notification_history ADD COLUMN requires_read INTEGER NOT NULL DEFAULT 0;")?;
        }
        if !columns.iter().any(|name| name == "read_at_ms") {
            transaction.execute_batch("ALTER TABLE notification_history ADD COLUMN read_at_ms INTEGER;")?;
        }
        transaction.commit()?;
        Ok(Self { connection })
    }

    pub fn record(&mut self, snapshot: &RequestSnapshot) -> rusqlite::Result<()> {
        if snapshot.mode != Mode::Notify || !snapshot.state.is_terminal() { return Ok(()); }
        let transaction = self.connection.transaction()?;
        let ended_at_ms = snapshot.ended_at.map(millis).unwrap_or_else(|| millis(SystemTime::now()));
        let read_at_ms = (!matches!(snapshot.state, RequestState::Expired | RequestState::Failed)).then_some(ended_at_ms);
        transaction.execute("INSERT INTO notification_history
          (request_id,state,revision,content_json,created_at_ms,shown_at_ms,ended_at_ms,response_json,requires_read,read_at_ms)
          VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
          ON CONFLICT(request_id) DO NOTHING", params![snapshot.request_id.to_string(),
            serde_json::to_value(snapshot.state).unwrap().as_str().unwrap(), snapshot.revision,
            serde_json::to_string(&snapshot.content).unwrap(), millis(snapshot.created_at),
            snapshot.shown_at.map(millis), ended_at_ms,
            snapshot.response.as_ref().map(|v| serde_json::to_string(v).unwrap()), snapshot.requires_read, read_at_ms])?;
        transaction.execute("DELETE FROM notification_history WHERE request_id IN
          (SELECT request_id FROM notification_history ORDER BY ended_at_ms DESC, rowid DESC LIMIT -1 OFFSET 500)", [])?;
        transaction.commit()
    }

    pub fn list(&self) -> rusqlite::Result<Vec<HistoryItem>> {
        // List only the fields used in the sidebar, never every inline image/spec.
        let mut statement = self.connection.prepare("SELECT request_id,state,revision,
          json_object('title',json_extract(content_json,'$.title'),'severity',COALESCE(json_extract(content_json,'$.severity'),'info')),
          created_at_ms,shown_at_ms,ended_at_ms,response_json,requires_read,read_at_ms
          FROM notification_history ORDER BY ended_at_ms DESC, rowid DESC LIMIT 500")?;
        let items = statement.query_map([], history_item)?.collect();
        items
    }

    pub fn get(&self, request_id: &str) -> rusqlite::Result<HistoryItem> {
        self.connection.query_row("SELECT request_id,state,revision,content_json,created_at_ms,shown_at_ms,ended_at_ms,response_json,requires_read,read_at_ms
          FROM notification_history WHERE request_id = ?1", [request_id], history_item)
    }

    pub fn pending_read_count(&self) -> rusqlite::Result<usize> {
        self.connection.query_row("SELECT COUNT(*) FROM notification_history WHERE requires_read = 1 AND read_at_ms IS NULL", [], |row| row.get(0))
    }

    pub fn mark_read(&mut self, request_id: &str) -> rusqlite::Result<i64> {
        let read_at_ms = millis(SystemTime::now());
        self.connection.query_row("UPDATE notification_history SET read_at_ms = COALESCE(read_at_ms, ?2)
          WHERE request_id = ?1 RETURNING read_at_ms", params![request_id, read_at_ms], |row| row.get(0))
    }

    pub fn clear(&mut self) -> rusqlite::Result<usize> {
        self.connection.execute("DELETE FROM notification_history", [])
    }
}

fn history_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryItem> {
    let content: String = row.get(3)?;
    let response: Option<String> = row.get(7)?;
    Ok(HistoryItem { request_id: row.get(0)?, state: row.get(1)?, revision: row.get(2)?,
        content: serde_json::from_str(&content).unwrap_or_default(), created_at_ms: row.get(4)?,
        shown_at_ms: row.get(5)?, ended_at_ms: row.get(6)?, response: response.and_then(|v| serde_json::from_str(&v).ok()),
        requires_read: row.get(8)?, read_at_ms: row.get(9)? })
}

fn millis(time: SystemTime) -> i64 { time.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(i64::MAX as u128) as i64 }

fn default_path() -> PathBuf {
    #[cfg(windows)] { return PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into())).join("Popu").join("popu.db"); }
    #[cfg(not(windows))] {
        if let Some(path) = std::env::var_os("XDG_DATA_HOME") { PathBuf::from(path).join("popu").join("popu.db") }
        else { PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(".local/share/popu/popu.db") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RequestRegistry;
    use crate::protocol::Request;

    #[test]
    fn keeps_only_the_latest_500_notifications() {
        let mut store = HistoryStore::from_connection(Connection::open_in_memory().unwrap()).unwrap();
        let mut registry = RequestRegistry::default();
        for index in 0..501 {
            let request = Request::parse(&format!(r#"{{"version":"1.0","mode":"notify","content":{{"title":"Item {index}"}}}}"#)).unwrap();
            let created = registry.create(request, None);
            registry.dismiss(created.request_id).unwrap();
            store.record(&registry.snapshot(created.request_id).unwrap()).unwrap();
        }
        let items = store.list().unwrap();
        assert_eq!(items.len(), 500);
        assert_eq!(items[0].content["title"], "Item 500");
    }

    #[test]
    fn preserves_the_notification_terminal_state_for_history_detail() {
        let mut store = HistoryStore::from_connection(Connection::open_in_memory().unwrap()).unwrap();
        let mut registry = RequestRegistry::default();
        let request = Request::parse(
            r#"{"version":"1.0","mode":"notify","content":{"title":"Completed"}}"#,
        )
        .unwrap();
        let created = registry.create(request, None);
        let request = registry.request(created.request_id).unwrap().clone();
        registry
            .complete(
                created.request_id,
                request.terminal_response(crate::protocol::ResponseStatus::Closed, None),
                10,
            )
            .unwrap();
        store.record(&registry.snapshot(created.request_id).unwrap()).unwrap();

        assert_eq!(store.list().unwrap()[0].state, "closed");
    }

    fn terminal(requires_read: bool, state: RequestState) -> RequestSnapshot {
        let mut registry = RequestRegistry::default();
        let request = Request::parse(&format!(r#"{{"version":"1.0","mode":"notify","requires_read":{requires_read},"content":{{"title":"History item"}}}}"#)).unwrap();
        let created = registry.create(request.clone(), None);
        match state {
            RequestState::Failed => { registry.fail(created.request_id).unwrap(); }
            RequestState::Cancelled => { registry.dismiss(created.request_id).unwrap(); }
            RequestState::Expired | RequestState::Closed => {
                let status = if state == RequestState::Expired { crate::protocol::ResponseStatus::TimedOut } else { crate::protocol::ResponseStatus::Closed };
                registry.complete(created.request_id, request.terminal_response(status, None), 11).unwrap();
            }
            _ => panic!("unexpected test state"),
        }
        registry.snapshot(created.request_id).unwrap()
    }

    #[test]
    fn reading_requirement_and_unread_are_independent() {
        for requires_read in [false, true] {
            for state in [RequestState::Expired, RequestState::Failed, RequestState::Closed, RequestState::Cancelled] {
                let mut store = HistoryStore::from_connection(Connection::open_in_memory().unwrap()).unwrap();
                store.record(&terminal(requires_read, state)).unwrap();
                let items = store.list().unwrap();
                let unread = matches!(state, RequestState::Expired | RequestState::Failed);
                assert_eq!(items[0].requires_read, requires_read);
                assert_eq!(items[0].read_at_ms.is_none(), unread, "{state:?}");
                assert_eq!(store.pending_read_count().unwrap(), usize::from(requires_read && unread));
            }
        }
    }

    #[test]
    fn reading_one_item_preserves_other_pending_items_and_terminal_state() {
        let mut store = HistoryStore::from_connection(Connection::open_in_memory().unwrap()).unwrap();
        let first = terminal(true, RequestState::Expired);
        let second = terminal(true, RequestState::Failed);
        let ordinary = terminal(false, RequestState::Expired);
        for item in [&first, &second, &ordinary] { store.record(item).unwrap(); }
        assert_eq!(store.pending_read_count().unwrap(), 2);
        store.mark_read(&ordinary.request_id.to_string()).unwrap();
        assert_eq!(store.pending_read_count().unwrap(), 2);
        let timestamp = store.mark_read(&first.request_id.to_string()).unwrap();
        assert_eq!(store.pending_read_count().unwrap(), 1);
        assert_eq!(store.mark_read(&first.request_id.to_string()).unwrap(), timestamp);
        store.record(&first).unwrap();
        let items = store.list().unwrap();
        let first = items.iter().find(|item| item.request_id == first.request_id.to_string()).unwrap();
        assert_eq!(first.state, "expired");
        assert_eq!(first.read_at_ms, Some(timestamp));
        assert_eq!(store.pending_read_count().unwrap(), 1);
        assert_eq!(store.clear().unwrap(), 3);
        assert_eq!(store.pending_read_count().unwrap(), 0);
        assert!(store.mark_read(&second.request_id.to_string()).is_err());
    }

    #[test]
    fn unread_requirements_and_read_timestamps_survive_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.db");
        let first = terminal(true, RequestState::Expired);
        let second = terminal(true, RequestState::Expired);
        let timestamp = {
            let mut store = HistoryStore::from_connection(Connection::open(&path).unwrap()).unwrap();
            store.record(&first).unwrap();
            store.record(&second).unwrap();
            store.mark_read(&first.request_id.to_string()).unwrap()
        };
        let store = HistoryStore::from_connection(Connection::open(&path).unwrap()).unwrap();
        assert_eq!(store.pending_read_count().unwrap(), 1);
        assert_eq!(store.list().unwrap().iter().find(|item| item.request_id == first.request_id.to_string()).unwrap().read_at_ms, Some(timestamp));
    }

    #[test]
    fn existing_contents_get_defaults_without_inferring_legacy_attention() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE notification_history (
          request_id TEXT PRIMARY KEY, state TEXT NOT NULL, revision INTEGER NOT NULL,
          content_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL, shown_at_ms INTEGER,
          ended_at_ms INTEGER NOT NULL, response_json TEXT);
          INSERT INTO notification_history VALUES ('old','expired',1,'{\"title\":\"Old item\"}',1,1,2,NULL);
          CREATE TABLE history_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
          INSERT INTO history_meta VALUES ('needs_attention', '1');").unwrap();
        let store = HistoryStore::from_connection(connection).unwrap();
        let items = store.list().unwrap();
        assert_eq!(items[0].content["title"], "Old item");
        assert!(!items[0].requires_read);
        assert!(items[0].read_at_ms.is_none());
        assert_eq!(store.pending_read_count().unwrap(), 0);
    }

    #[test]
    fn retention_drops_reminders_along_with_their_records() {
        let mut store = HistoryStore::from_connection(Connection::open_in_memory().unwrap()).unwrap();
        store.record(&terminal(true, RequestState::Expired)).unwrap();
        for _ in 0..500 { store.record(&terminal(false, RequestState::Cancelled)).unwrap(); }
        assert_eq!(store.list().unwrap().len(), 500);
        assert_eq!(store.pending_read_count().unwrap(), 0);
    }

    #[test]
    fn visual_history_retains_the_image_bytes_and_chart_data_snapshot() {
        let mut store = HistoryStore::from_connection(Connection::open_in_memory().unwrap()).unwrap();
        let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../protocol/examples");
        for name in ["notification-visual-image.json", "notification-visual-line.json"] {
            let request = Request::parse(&std::fs::read_to_string(examples.join(name)).unwrap()).unwrap();
            let expected = serde_json::to_value(&request.content).unwrap();
            let mut registry = RequestRegistry::default();
            let created = registry.create(request, None);
            registry.fail(created.request_id).unwrap();
            store.record(&registry.snapshot(created.request_id).unwrap()).unwrap();
            let items = store.list().unwrap();
            assert!(items.iter().all(|item| item.content.get("blocks").is_none() && item.content.get("icon").is_none()));
            let item = store.get(&created.request_id.to_string()).unwrap();
            assert_eq!(item.content, expected);
        }
    }
}
