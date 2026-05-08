//! 持久化事件队列 — 可靠投递引擎
//!
//! 当 Agent 离线时，发给它的事件被持久化到 SQLite。
//! Agent 重连后通过 resume 命令获取未送达的事件。
//!
//! 重试机制：
//! - 事件入队后立即尝试在线投递
//! - 如果未收到 EventAck，按指数退避重试：5s → 15s → 45s → 135s → 405s
//! - 超过 MAX_RETRIES 后标记为 dead_letter
//! - Governor 后台任务每 10s 扫描需要重试的事件

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::GaggleError;

const MAX_EVENTS_PER_AGENT: usize = 1000;
const DELIVER_BATCH_SIZE: u32 = 500;
const MAX_RETRIES: u32 = 5;

/// 指数退避间隔（毫秒）：5s, 15s, 45s, 135s, 405s
const RETRY_BACKOFF_MS: &[i64] = &[5_000, 15_000, 45_000, 135_000, 405_000];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedEvent {
    pub id: i64,
    pub agent_id: String,
    pub event_type: String,
    pub payload: String,
    pub event_seq: i64,
    pub created_at: i64,
    pub retry_count: u32,
    pub next_retry_at: Option<i64>,
    pub is_dead_letter: bool,
}

/// 队列统计信息，供前端 observability 使用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    pub pending_count: i64,
    pub retry_pending_count: i64,
    pub dead_letter_count: i64,
    pub delivered_count: i64,
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
                delivered_at INTEGER,
                retry_count INTEGER NOT NULL DEFAULT 0,
                next_retry_at INTEGER,
                last_attempt_at INTEGER,
                is_dead_letter INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_eq_agent_seq
             ON event_queue (agent_id, event_seq)",
            [],
        )?;

        // Migration: add retry columns to existing table
        let cols: Vec<String> = conn
            .prepare("SELECT * FROM event_queue LIMIT 0")?
            .column_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        if !cols.contains(&"retry_count".to_string()) {
            conn.execute_batch(
                "ALTER TABLE event_queue ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE event_queue ADD COLUMN next_retry_at INTEGER;
                 ALTER TABLE event_queue ADD COLUMN last_attempt_at INTEGER;
                 ALTER TABLE event_queue ADD COLUMN is_dead_letter INTEGER NOT NULL DEFAULT 0;"
            )?;
        }

        // Partial index for retry scheduler (only if column exists after migration)
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_eq_retry
             ON event_queue (next_retry_at)
             WHERE delivered_at IS NULL AND is_dead_letter = 0 AND next_retry_at IS NOT NULL",
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

        let next_seq: i64 = db
            .query_row(
                "SELECT COALESCE(MAX(event_seq), 0) + 1 FROM event_queue WHERE agent_id = ?1",
                params![agent_id],
                |row| row.get(0),
            )
            .unwrap_or(1);

        // 首次投递窗口：5 秒后如果没收到 ACK 就开始重试
        let first_retry = now + RETRY_BACKOFF_MS[0];

        db.execute(
            "INSERT INTO event_queue (agent_id, event_type, payload, event_seq, created_at, next_retry_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![agent_id, event_type, payload, next_seq, now, first_retry],
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

    /// 获取未送达的事件（供 Resume 使用）
    pub async fn get_pending(
        &self,
        agent_id: &str,
        after_seq: i64,
    ) -> Result<Vec<QueuedEvent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, agent_id, event_type, payload, event_seq, created_at,
                    retry_count, next_retry_at, is_dead_letter
             FROM event_queue
             WHERE agent_id = ?1 AND event_seq > ?2 AND delivered_at IS NULL AND is_dead_letter = 0
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
                    retry_count: row.get(6)?,
                    next_retry_at: row.get(7)?,
                    is_dead_letter: row.get::<_, i32>(8)? != 0,
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
                "UPDATE event_queue SET delivered_at = ?1, next_retry_at = NULL
                 WHERE agent_id = ?2 AND event_seq = ?3",
                params![now, agent_id, seq],
            )?;
        }
        Ok(())
    }

    /// 累积 ACK：标记该 agent 所有 event_seq <= up_to_seq 的未送达事件为已送达。
    /// 返回标记的事件数量。
    pub async fn mark_delivered_up_to(
        &self,
        agent_id: &str,
        up_to_seq: i64,
    ) -> Result<usize, GaggleError> {
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();
        let rows = db.execute(
            "UPDATE event_queue SET delivered_at = ?1, next_retry_at = NULL
             WHERE agent_id = ?2 AND event_seq <= ?3 AND delivered_at IS NULL",
            params![now, agent_id, up_to_seq],
        )?;
        Ok(rows as usize)
    }

    /// 获取所有到期需要重试的事件（供 Governor 后台任务调用）。
    /// 返回 (agent_id, events) 列表，按 agent 分组。
    pub async fn get_retry_pending(&self) -> Result<Vec<(String, Vec<QueuedEvent>)>, GaggleError> {
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();

        let mut stmt = db.prepare(
            "SELECT id, agent_id, event_type, payload, event_seq, created_at,
                    retry_count, next_retry_at, is_dead_letter
             FROM event_queue
             WHERE delivered_at IS NULL
               AND is_dead_letter = 0
               AND next_retry_at IS NOT NULL
               AND next_retry_at <= ?1
             ORDER BY agent_id, event_seq ASC
             LIMIT 500",
        )?;

        let events = stmt
            .query_map(params![now], |row| {
                Ok(QueuedEvent {
                    id: row.get(0)?,
                    agent_id: row.get(1)?,
                    event_type: row.get(2)?,
                    payload: row.get(3)?,
                    event_seq: row.get(4)?,
                    created_at: row.get(5)?,
                    retry_count: row.get(6)?,
                    next_retry_at: row.get(7)?,
                    is_dead_letter: row.get::<_, i32>(8)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // Group by agent_id
        let mut grouped: Vec<(String, Vec<QueuedEvent>)> = Vec::new();
        let mut current_agent = String::new();
        let mut current_events = Vec::new();

        for evt in events {
            if evt.agent_id != current_agent {
                if !current_events.is_empty() {
                    grouped.push((current_agent.clone(), std::mem::take(&mut current_events)));
                }
                current_agent = evt.agent_id.clone();
            }
            current_events.push(evt);
        }
        if !current_events.is_empty() {
            grouped.push((current_agent, current_events));
        }

        Ok(grouped)
    }

    /// 标记事件为已重试一次。如果超过 MAX_RETRIES，标记为 dead_letter。
    /// 返回 true 如果事件仍有重试机会，false 如果已变成 dead_letter。
    pub async fn mark_retry_attempt(&self, event_id: i64) -> Result<bool, GaggleError> {
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();

        let retry_count: u32 = db
            .query_row(
                "SELECT retry_count FROM event_queue WHERE id = ?1",
                params![event_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let new_count = retry_count + 1;

        if new_count >= MAX_RETRIES {
            // Dead letter
            db.execute(
                "UPDATE event_queue SET retry_count = ?1, last_attempt_at = ?2,
                 is_dead_letter = 1, next_retry_at = NULL
                 WHERE id = ?3",
                params![new_count, now, event_id],
            )?;
            Ok(false)
        } else {
            let backoff_idx = std::cmp::min(new_count as usize, RETRY_BACKOFF_MS.len()) - 1;
            let next_retry = now + RETRY_BACKOFF_MS[backoff_idx];
            db.execute(
                "UPDATE event_queue SET retry_count = ?1, last_attempt_at = ?2, next_retry_at = ?3
                 WHERE id = ?4",
                params![new_count, now, next_retry, event_id],
            )?;
            Ok(true)
        }
    }

    /// 重置事件的重试计时器（在成功在线投递后调用，等待新的 ACK 超时再重试）
    pub async fn reset_retry_timer(&self, event_id: i64) -> Result<(), GaggleError> {
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();
        let next_retry = now + RETRY_BACKOFF_MS[0];
        db.execute(
            "UPDATE event_queue SET next_retry_at = ?1 WHERE id = ?2 AND delivered_at IS NULL",
            params![next_retry, event_id],
        )?;
        Ok(())
    }

    /// 获取队列统计信息
    pub async fn get_stats(&self) -> Result<QueueStats, GaggleError> {
        let db = self.db.lock().await;
        let pending: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM event_queue WHERE delivered_at IS NULL AND is_dead_letter = 0 AND next_retry_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let retry_pending: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM event_queue WHERE delivered_at IS NULL AND is_dead_letter = 0 AND next_retry_at IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let dead_letter: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM event_queue WHERE is_dead_letter = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let delivered: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM event_queue WHERE delivered_at IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(QueueStats {
            pending_count: pending,
            retry_pending_count: retry_pending,
            dead_letter_count: dead_letter,
            delivered_count: delivered,
        })
    }

    /// Delete delivered events older than `days` days.
    /// Returns the number of deleted rows.
    pub async fn cleanup_delivered(&self, days: i64) -> Result<usize, GaggleError> {
        let db = self.db.lock().await;
        let cutoff = chrono::Utc::now().timestamp_millis() - (days * 86_400_000);
        let rows = db.execute(
            "DELETE FROM event_queue WHERE delivered_at IS NOT NULL AND delivered_at < ?1",
            params![cutoff],
        )?;
        Ok(rows as usize)
    }

    /// Recover dead letter events for an agent: reset dead_letter flag so they
    /// can be replayed on reconnect. Returns recovered events in event_seq order.
    ///
    /// This ensures the core guarantee: "events never silently disappear."
    pub async fn recover_dead_letters(
        &self,
        agent_id: &str,
    ) -> Result<Vec<QueuedEvent>, GaggleError> {
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();

        // Reset dead_letter flag and set a fresh retry timer
        db.execute(
            "UPDATE event_queue
             SET is_dead_letter = 0, retry_count = 0, next_retry_at = ?1
             WHERE agent_id = ?2 AND is_dead_letter = 1 AND delivered_at IS NULL",
            params![now + RETRY_BACKOFF_MS[0], agent_id],
        )?;

        drop(db);

        // Return all pending (including the just-recovered ones) for replay
        self.get_pending(agent_id, 0).await
    }

    /// List dead letter events, optionally filtered by agent_id.
    /// Returns events ordered by created_at DESC.
    pub async fn list_dead_letters(
        &self,
        agent_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<QueuedEvent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = if agent_id.is_some() {
            db.prepare(
                "SELECT id, agent_id, event_type, payload, event_seq, created_at,
                        retry_count, next_retry_at, is_dead_letter
                 FROM event_queue
                 WHERE is_dead_letter = 1 AND agent_id = ?1
                 ORDER BY created_at DESC LIMIT ?2",
            )?
        } else {
            db.prepare(
                "SELECT id, agent_id, event_type, payload, event_seq, created_at,
                        retry_count, next_retry_at, is_dead_letter
                 FROM event_queue
                 WHERE is_dead_letter = 1
                 ORDER BY created_at DESC LIMIT ?1",
            )?
        };

        let events = if agent_id.is_some() {
            stmt.query_map(params![agent_id.unwrap(), limit], |row| {
                Ok(QueuedEvent {
                    id: row.get(0)?,
                    agent_id: row.get(1)?,
                    event_type: row.get(2)?,
                    payload: row.get(3)?,
                    event_seq: row.get(4)?,
                    created_at: row.get(5)?,
                    retry_count: row.get(6)?,
                    next_retry_at: row.get(7)?,
                    is_dead_letter: row.get::<_, i32>(8)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![limit], |row| {
                Ok(QueuedEvent {
                    id: row.get(0)?,
                    agent_id: row.get(1)?,
                    event_type: row.get(2)?,
                    payload: row.get(3)?,
                    event_seq: row.get(4)?,
                    created_at: row.get(5)?,
                    retry_count: row.get(6)?,
                    next_retry_at: row.get(7)?,
                    is_dead_letter: row.get::<_, i32>(8)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        Ok(events)
    }

    /// Retry a specific dead letter event: reset dead_letter flag.
    pub async fn retry_dead_letter(&self, event_id: i64) -> Result<bool, GaggleError> {
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();
        let rows = db.execute(
            "UPDATE event_queue
             SET is_dead_letter = 0, retry_count = 0, next_retry_at = ?1
             WHERE id = ?2 AND is_dead_letter = 1",
            params![now + RETRY_BACKOFF_MS[0], event_id],
        )?;
        Ok(rows > 0)
    }

    /// Clean up orphan events whose agent_id no longer exists in the agents table.
    /// These events can never be delivered because the agent has been deleted.
    /// Returns the number of deleted rows.
    pub async fn cleanup_orphan_events(&self, db_path: &str) -> Result<usize, GaggleError> {
        let orphan_conn = Connection::open(db_path)?;
        let orphan_ids: Vec<String> = orphan_conn
            .prepare(
                "SELECT DISTINCT eq.agent_id FROM event_queue eq
                 LEFT JOIN agents a ON eq.agent_id = a.id
                 WHERE eq.delivered_at IS NULL AND a.id IS NULL",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        if orphan_ids.is_empty() {
            return Ok(0);
        }

        let db = self.db.lock().await;
        let mut total = 0usize;
        for id in &orphan_ids {
            let rows = db.execute(
                "DELETE FROM event_queue WHERE agent_id = ?1 AND delivered_at IS NULL",
                params![id],
            )?;
            total += rows as usize;
        }

        if total > 0 {
            tracing::info!(
                orphan_agents = orphan_ids.len(),
                events_deleted = total,
                "cleanup_orphan_events: removed events for deleted agents"
            );
        }

        Ok(total)
    }

    /// Clean up pending events older than `days` that have never been delivered.
    /// These represent stale events for agents that are unlikely to ever reconnect.
    /// Returns the number of deleted rows.
    pub async fn cleanup_stale_pending(&self, days: i64) -> Result<usize, GaggleError> {
        let db = self.db.lock().await;
        let cutoff = chrono::Utc::now().timestamp_millis() - (days * 86_400_000);
        let rows = db.execute(
            "DELETE FROM event_queue
             WHERE delivered_at IS NULL
               AND is_dead_letter = 0
               AND created_at < ?1
               AND retry_count = 0",
            params![cutoff],
        )?;
        if rows > 0 {
            tracing::info!(deleted = rows, days, "cleanup_stale_pending: removed old undelivered events");
        }
        Ok(rows as usize)
    }

    /// Physically delete dead letter events older than `days` days.
    /// Returns the number of deleted rows.
    pub async fn cleanup_dead_letters(&self, days: i64) -> Result<usize, GaggleError> {
        let db = self.db.lock().await;
        let cutoff = chrono::Utc::now().timestamp_millis() - (days * 86_400_000);
        let rows = db.execute(
            "DELETE FROM event_queue WHERE is_dead_letter = 1 AND delivered_at IS NULL AND last_attempt_at < ?1",
            params![cutoff],
        )?;
        Ok(rows as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_enqueue_assigns_sequential_event_seq() {
        let q = EventQueue::new(":memory:").unwrap();

        let s1 = q.enqueue("agent_a", "msg", r#"{"type":"ping"}"#).await.unwrap();
        let s2 = q.enqueue("agent_a", "msg", r#"{"type":"pong"}"#).await.unwrap();
        let s3 = q.enqueue("agent_b", "msg", r#"{"type":"ping"}"#).await.unwrap();

        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 1, "different agents have independent seq counters");
    }

    #[tokio::test]
    async fn test_get_pending_returns_only_undelivered() {
        let q = EventQueue::new(":memory:").unwrap();

        q.enqueue("a1", "t1", "{}").await.unwrap(); // seq 1
        q.enqueue("a1", "t2", "{}").await.unwrap(); // seq 2
        q.enqueue("a1", "t3", "{}").await.unwrap(); // seq 3

        // Mark seq 2 as delivered
        q.mark_delivered("a1", &[2]).await.unwrap();

        let pending = q.get_pending("a1", 0).await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].event_seq, 1);
        assert_eq!(pending[1].event_seq, 3);
    }

    #[tokio::test]
    async fn test_mark_delivered_up_to_cumulative_ack() {
        let q = EventQueue::new(":memory:").unwrap();

        q.enqueue("a1", "t1", "{}").await.unwrap(); // seq 1
        q.enqueue("a1", "t2", "{}").await.unwrap(); // seq 2
        q.enqueue("a1", "t3", "{}").await.unwrap(); // seq 3
        q.enqueue("a1", "t4", "{}").await.unwrap(); // seq 4
        q.enqueue("a1", "t5", "{}").await.unwrap(); // seq 5

        // ACK seq 3 → should mark seq 1, 2, 3 as delivered
        let count = q.mark_delivered_up_to("a1", 3).await.unwrap();
        assert_eq!(count, 3);

        // Pending should only have seq 4, 5
        let pending = q.get_pending("a1", 0).await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].event_seq, 4);
        assert_eq!(pending[1].event_seq, 5);

        // ACK seq 4 → should mark only seq 4 (1-3 already delivered)
        let count2 = q.mark_delivered_up_to("a1", 4).await.unwrap();
        assert_eq!(count2, 1);

        let pending2 = q.get_pending("a1", 0).await.unwrap();
        assert_eq!(pending2.len(), 1);
        assert_eq!(pending2[0].event_seq, 5);
    }

    #[tokio::test]
    async fn test_mark_delivered_up_to_idempotent() {
        let q = EventQueue::new(":memory:").unwrap();

        q.enqueue("a1", "t1", "{}").await.unwrap(); // seq 1
        q.enqueue("a1", "t2", "{}").await.unwrap(); // seq 2

        let c1 = q.mark_delivered_up_to("a1", 5).await.unwrap();
        assert_eq!(c1, 2);

        // Calling again with same or lower seq should mark 0 (already delivered)
        let c2 = q.mark_delivered_up_to("a1", 5).await.unwrap();
        assert_eq!(c2, 0);
    }

    #[tokio::test]
    async fn test_get_pending_after_seq_filter() {
        let q = EventQueue::new(":memory:").unwrap();

        q.enqueue("a1", "t1", "{}").await.unwrap(); // seq 1
        q.enqueue("a1", "t2", "{}").await.unwrap(); // seq 2
        q.enqueue("a1", "t3", "{}").await.unwrap(); // seq 3

        // Resume from seq 1 → should only get seq 2 and 3
        let pending = q.get_pending("a1", 1).await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].event_seq, 2);
        assert_eq!(pending[1].event_seq, 3);
    }

    #[tokio::test]
    async fn test_retry_mechanism_and_dead_letter() {
        let q = EventQueue::new(":memory:").unwrap();

        q.enqueue("a1", "t1", "{}").await.unwrap(); // seq 1

        // Simulate retry attempts
        let pending = q.get_pending("a1", 0).await.unwrap();
        assert_eq!(pending.len(), 1);
        let event_id = pending[0].id;

        // First retry — should still have retries left
        let alive = q.mark_retry_attempt(event_id).await.unwrap();
        assert!(alive, "should have retries remaining after first attempt");

        // Retry up to MAX_RETRIES - 1 more times
        for _ in 1..(MAX_RETRIES - 1) {
            let alive = q.mark_retry_attempt(event_id).await.unwrap();
            assert!(alive);
        }

        // Final retry — should become dead letter
        let alive = q.mark_retry_attempt(event_id).await.unwrap();
        assert!(!alive, "should be dead letter after max retries");

        // Verify it no longer appears in pending
        let pending = q.get_pending("a1", 0).await.unwrap();
        assert_eq!(pending.len(), 0, "dead letter should not appear in pending");
    }

    #[tokio::test]
    async fn test_queue_stats() {
        let q = EventQueue::new(":memory:").unwrap();

        q.enqueue("a1", "t1", "{}").await.unwrap();
        q.enqueue("a1", "t2", "{}").await.unwrap();
        q.enqueue("a1", "t3", "{}").await.unwrap();

        let stats = q.get_stats().await.unwrap();
        assert!(stats.pending_count + stats.retry_pending_count >= 3);
        assert_eq!(stats.dead_letter_count, 0);
        assert_eq!(stats.delivered_count, 0);

        // Deliver one
        q.mark_delivered_up_to("a1", 1).await.unwrap();
        let stats = q.get_stats().await.unwrap();
        assert_eq!(stats.delivered_count, 1);
    }

    #[tokio::test]
    async fn test_recover_dead_letters() {
        let q = EventQueue::new(":memory:").unwrap();

        // Enqueue events and exhaust retries to create dead letters
        q.enqueue("a1", "t1", r#"{"data":1}"#).await.unwrap();
        q.enqueue("a1", "t2", r#"{"data":2}"#).await.unwrap();

        // Get the event IDs and mark as dead letters
        let pending = q.get_pending("a1", 0).await.unwrap();
        assert_eq!(pending.len(), 2);
        for evt in &pending {
            for _ in 0..MAX_RETRIES {
                q.mark_retry_attempt(evt.id).await.unwrap();
            }
        }

        // Verify they're dead letters now
        let stats = q.get_stats().await.unwrap();
        assert_eq!(stats.dead_letter_count, 2);
        let pending_after = q.get_pending("a1", 0).await.unwrap();
        assert_eq!(pending_after.len(), 0, "dead letters excluded from pending");

        // Recover dead letters
        let recovered = q.recover_dead_letters("a1").await.unwrap();
        assert_eq!(recovered.len(), 2, "recovered events should appear in pending");
        assert_eq!(recovered[0].event_seq, 1);
        assert_eq!(recovered[1].event_seq, 2);

        // Dead letter count should be 0 now
        let stats = q.get_stats().await.unwrap();
        assert_eq!(stats.dead_letter_count, 0);
    }

    #[tokio::test]
    async fn test_cleanup_dead_letters() {
        let q = EventQueue::new(":memory:").unwrap();

        q.enqueue("a1", "t1", "{}").await.unwrap();

        // Mark as dead letter
        let pending = q.get_pending("a1", 0).await.unwrap();
        for _ in 0..MAX_RETRIES {
            q.mark_retry_attempt(pending[0].id).await.unwrap();
        }

        let stats = q.get_stats().await.unwrap();
        assert_eq!(stats.dead_letter_count, 1);

        // Cleanup with -1 days: cutoff = now + 1 day → always in the future
        let deleted = q.cleanup_dead_letters(-1).await.unwrap();
        assert_eq!(deleted, 1);

        let stats = q.get_stats().await.unwrap();
        assert_eq!(stats.dead_letter_count, 0);
    }
}
