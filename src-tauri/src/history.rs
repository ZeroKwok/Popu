use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::core::RequestSnapshot;
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
}

impl HistoryStore {
    pub fn open_default() -> rusqlite::Result<Self> {
        let path = default_path();
        if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
        Self::from_connection(Connection::open(path)?)
    }

    fn from_connection(connection: Connection) -> rusqlite::Result<Self> {
        connection.execute_batch("CREATE TABLE IF NOT EXISTS notification_history (
          request_id TEXT PRIMARY KEY, state TEXT NOT NULL, revision INTEGER NOT NULL,
          content_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL, shown_at_ms INTEGER,
          ended_at_ms INTEGER NOT NULL, response_json TEXT
        ); CREATE INDEX IF NOT EXISTS notification_history_ended ON notification_history(ended_at_ms DESC);")?;
        Ok(Self { connection })
    }

    pub fn record(&mut self, snapshot: &RequestSnapshot) -> rusqlite::Result<()> {
        if snapshot.mode != Mode::Notify || !snapshot.state.is_terminal() { return Ok(()); }
        self.connection.execute("INSERT OR REPLACE INTO notification_history
          (request_id,state,revision,content_json,created_at_ms,shown_at_ms,ended_at_ms,response_json)
          VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![snapshot.request_id.to_string(),
            serde_json::to_value(snapshot.state).unwrap().as_str().unwrap(), snapshot.revision,
            serde_json::to_string(&snapshot.content).unwrap(), millis(snapshot.created_at),
            snapshot.shown_at.map(millis), snapshot.ended_at.map(millis).unwrap_or_else(|| millis(SystemTime::now())),
            snapshot.response.as_ref().map(|v| serde_json::to_string(v).unwrap())])?;
        self.connection.execute("DELETE FROM notification_history WHERE request_id IN
          (SELECT request_id FROM notification_history ORDER BY ended_at_ms DESC, rowid DESC LIMIT -1 OFFSET 500)", [])?;
        Ok(())
    }

    pub fn list(&self) -> rusqlite::Result<Vec<HistoryItem>> {
        let mut statement = self.connection.prepare("SELECT request_id,state,revision,content_json,created_at_ms,shown_at_ms,ended_at_ms,response_json FROM notification_history ORDER BY ended_at_ms DESC, rowid DESC LIMIT 500")?;
        let items = statement.query_map([], |row| {
            let content: String = row.get(3)?; let response: Option<String> = row.get(7)?;
            Ok(HistoryItem { request_id: row.get(0)?, state: row.get(1)?, revision: row.get(2)?,
                content: serde_json::from_str(&content).unwrap_or_default(), created_at_ms: row.get(4)?,
                shown_at_ms: row.get(5)?, ended_at_ms: row.get(6)?, response: response.and_then(|v| serde_json::from_str(&v).ok()) })
        })?.collect();
        items
    }

    pub fn clear(&mut self) -> rusqlite::Result<usize> { self.connection.execute("DELETE FROM notification_history", []) }
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
            let (created, _) = registry.create(request, None);
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
        let (created, _) = registry.create(request, None);
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
}
