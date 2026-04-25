//! 离线事件队列
//!
//! 当 Agent 离线时，发给它的事件被持久化到 SQLite。
//! Agent 重连后通过 resume 命令获取未送达的事件。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::GaggleError;

const MAX_EVENTS_PER_AGENT: usize = 1000;
const DELIVER_BATCH_SIZE: u32 = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedEvent {
    pub id: i64,
    pub agent_id: String,
    pub event_type: String,
    pub payload: String,
    pub event_seq: i64,
    pub created_at: i64,
}

#[derive(Clone)]
pub struct EventQueue {
    db: Arc<Mutex<Connection>>,
}

impl EventQueue {
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;"
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS event_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                event_seq INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                delivered_at INTEGER
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_eq_agent_seq
             ON event_queue (agent_id, event_seq)",
            [],
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// 入队一个事件，返回 event_seq
    pub async fn enqueue(
        &self,
        agent_id: &str,
        event_type: &str,
        payload: &str,
    ) -> Result<i64, GaggleError> {
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();

        // 获取该 agent 下一个 seq
        let next_seq: i64 = db
            .query_row(
                "SELECT COALESCE(MAX(event_seq), 0) + 1 FROM event_queue WHERE agent_id = ?1",
                params![agent_id],
                |row| row.get(0),
            )
            .unwrap_or(1);

        db.execute(
            "INSERT INTO event_queue (agent_id, event_type, payload, event_seq, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![agent_id, event_type, payload, next_seq, now],
        )?;

        // 清理旧事件，保留最新 MAX_EVENTS_PER_AGENT 条
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM event_queue WHERE agent_id = ?1",
                params![agent_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if count > MAX_EVENTS_PER_AGENT as i64 {
            let keep = MAX_EVENTS_PER_AGENT as i64;
            db.execute(
                "DELETE FROM event_queue WHERE agent_id = ?1 AND id NOT IN (
                    SELECT id FROM event_queue WHERE agent_id = ?1 ORDER BY event_seq DESC LIMIT ?2
                )",
                params![agent_id, keep],
            )?;
        }

        Ok(next_seq)
    }

    /// 入队并标记为已送达（在线时用）
    pub async fn enqueue_delivered(
        &self,
        agent_id: &str,
        event_type: &str,
        payload: &str,
    ) -> Result<i64, GaggleError> {
        let seq = self.enqueue(agent_id, event_type, payload).await?;
        // 立即标记为已送达
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();
        db.execute(
            "UPDATE event_queue SET delivered_at = ?1 WHERE agent_id = ?2 AND event_seq = ?3",
            params![now, agent_id, seq],
        )?;
        Ok(seq)
    }

    /// 获取未送达的事件
    pub async fn get_pending(
        &self,
        agent_id: &str,
        after_seq: i64,
    ) -> Result<Vec<QueuedEvent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, agent_id, event_type, payload, event_seq, created_at
             FROM event_queue
             WHERE agent_id = ?1 AND event_seq > ?2 AND delivered_at IS NULL
             ORDER BY event_seq ASC
             LIMIT ?3",
        )?;

        let events = stmt
            .query_map(params![agent_id, after_seq, DELIVER_BATCH_SIZE], |row| {
                Ok(QueuedEvent {
                    id: row.get(0)?,
                    agent_id: row.get(1)?,
                    event_type: row.get(2)?,
                    payload: row.get(3)?,
                    event_seq: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(events)
    }

    /// 标记事件为已送达
    pub async fn mark_delivered(&self, agent_id: &str, seqs: &[i64]) -> Result<(), GaggleError> {
        if seqs.is_empty() {
            return Ok(());
        }
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();
        for seq in seqs {
            db.execute(
                "UPDATE event_queue SET delivered_at = ?1 WHERE agent_id = ?2 AND event_seq = ?3",
                params![now, agent_id, seq],
            )?;
        }
        Ok(())
    }
}
