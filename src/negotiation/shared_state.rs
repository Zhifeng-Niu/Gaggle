//! Shared Reality Layer
//!
//! Space 内的版本化共享状态。所有 Agent 对 Shared State 达成一致认知。
//! State 是 Single Source of Truth，消息只是 State Transition Input。
//!
//! 核心原则：
//! - append-only event log：所有状态变更都是不可逆事件
//! - versioned state：每次变更递增版本号
//! - inspectable & replayable：可以从事件日志重建任意版本的状态

use crate::error::GaggleError;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

// ── 类型定义 ──────────────────────────────────────────

/// 状态变更事件类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StateEventType {
    /// 设置 key（新建或更新）
    Set,
    /// 删除 key
    Delete,
}

/// 状态变更事件 — append-only log 的每一行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEvent {
    pub id: String,
    pub space_id: String,
    pub event_type: StateEventType,
    pub key: String,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub author_id: String,
    /// 变更后的 space state version
    pub space_version: u64,
    pub timestamp: i64,
    /// 前一个 event 的 SHA-256 hash（链头为全零）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_hash: Option<String>,
    /// 本 event 的 SHA-256 hash
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_hash: Option<String>,
}

/// 当前共享状态的一个条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEntry {
    pub space_id: String,
    pub key: String,
    pub value: serde_json::Value,
    /// 该 key 最后更新时的 space version
    pub version: u64,
    pub author_id: String,
    pub updated_at: i64,
}

/// 设置状态请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetStateRequest {
    pub key: String,
    pub value: serde_json::Value,
    pub author_id: String,
    /// 乐观锁：如果指定，只有当前 version 匹配时才写入
    #[serde(default)]
    pub expected_version: Option<u64>,
    /// 幂等键：如果指定且之前已处理过相同 key 的请求，返回之前的结果而非创建新 entry
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

/// 状态变更结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateUpdateResult {
    pub key: String,
    pub old_value: Option<serde_json::Value>,
    pub new_value: serde_json::Value,
    pub previous_version: u64,
    pub new_version: u64,
    pub event_id: String,
}

/// 完整的 Space 共享状态快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceStateSnapshot {
    pub space_id: String,
    pub version: u64,
    pub entries: Vec<StateEntry>,
    pub updated_at: i64,
}

/// 状态变更事件的 REST API 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateEventResponse {
    pub events: Vec<StateEvent>,
    /// 总事件数（用于分页）
    pub total: usize,
}

// ── Hash Chain Functions ──────────────────────────────────

/// 初始 prev_hash（链头）
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// 计算 event 的 SHA-256 hash
/// hash = SHA-256(prev_hash || space_id || event_type || key || old_value || new_value || author_id || space_version || timestamp)
fn compute_event_hash(
    prev_hash: &str,
    space_id: &str,
    event_type: &str,
    key: &str,
    old_value: &Option<String>,
    new_value: &Option<String>,
    author_id: &str,
    space_version: u64,
    timestamp: i64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(space_id.as_bytes());
    hasher.update(event_type.as_bytes());
    hasher.update(key.as_bytes());
    hasher.update(old_value.as_deref().unwrap_or(""));
    hasher.update(new_value.as_deref().unwrap_or(""));
    hasher.update(author_id.as_bytes());
    hasher.update(space_version.to_string().as_bytes());
    hasher.update(timestamp.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

// ── SharedStateManager ──────────────────────────────────

pub struct SharedStateManager {
    db: Arc<Mutex<Connection>>,
}

impl SharedStateManager {
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        // 共享状态表：每个 space 的 key-value store
        conn.execute(
            "CREATE TABLE IF NOT EXISTS shared_state (
                space_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                version INTEGER NOT NULL,
                author_id TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (space_id, key)
            )",
            [],
        )?;

        // 状态变更事件日志 — append-only with hash chain
        conn.execute(
            "CREATE TABLE IF NOT EXISTS state_events (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                key TEXT NOT NULL,
                old_value TEXT,
                new_value TEXT,
                author_id TEXT NOT NULL,
                space_version INTEGER NOT NULL,
                timestamp INTEGER NOT NULL,
                prev_hash TEXT,
                event_hash TEXT
            )",
            [],
        )?;

        // Migrate: add hash columns if missing (safe ALTER for existing DBs)
        let _ = conn.execute_batch(
            "ALTER TABLE state_events ADD COLUMN prev_hash TEXT;
             ALTER TABLE state_events ADD COLUMN event_hash TEXT;"
        );

        // Space 级别的 state version 追踪
        conn.execute(
            "CREATE TABLE IF NOT EXISTS space_state_versions (
                space_id TEXT PRIMARY KEY,
                version INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_state_events_space
             ON state_events (space_id, timestamp DESC)",
            [],
        )?;

        // 幂等键表：防止重复 state 操作
        conn.execute(
            "CREATE TABLE IF NOT EXISTS state_idempotency (
                space_id TEXT NOT NULL,
                idempotency_key TEXT NOT NULL,
                event_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (space_id, idempotency_key)
            )",
            [],
        )?;

        // Agent 游标表：跟踪每个 agent 在每个 space 中已见到的 state version
        conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_cursors (
                agent_id TEXT NOT NULL,
                space_id TEXT NOT NULL,
                last_seen_version INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (agent_id, space_id)
            )",
            [],
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// 获取 Space 的当前 state version
    pub async fn get_version(&self, space_id: &str) -> Result<u64, GaggleError> {
        let db = self.db.lock().await;
        let version: u64 = db
            .query_row(
                "SELECT version FROM space_state_versions WHERE space_id = ?1",
                params![space_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(version)
    }

    /// 设置一个状态 key（创建或更新）。支持幂等：如果提供 idempotency_key
    /// 且之前已处理过，返回之前的结果而不创建新 entry。
    pub async fn set(
        &self,
        space_id: &str,
        req: SetStateRequest,
    ) -> Result<StateUpdateResult, GaggleError> {
        let db = self.db.lock().await;

        // 幂等检查：如果 idempotency_key 已存在，返回之前的结果
        if let Some(ref idem_key) = req.idempotency_key {
            let existing: Option<String> = db
                .query_row(
                    "SELECT event_id FROM state_idempotency WHERE space_id = ?1 AND idempotency_key = ?2",
                    params![space_id, idem_key],
                    |row| row.get(0),
                )
                .optional()
                .ok()
                .flatten();

            if let Some(event_id) = existing {
                // 幂等命中：重建之前的结果
                let old_value: Option<serde_json::Value> = db
                    .query_row(
                        "SELECT old_value FROM state_events WHERE id = ?1",
                        params![event_id],
                        |row| {
                            let s: Option<String> = row.get(0)?;
                            Ok(s.and_then(|v| serde_json::from_str(&v).ok()))
                        },
                    )
                    .ok()
                    .flatten();

                return Ok(StateUpdateResult {
                    key: req.key.clone(),
                    old_value,
                    new_value: req.value.clone(),
                    previous_version: 0, // 不重要，调用者应检查 idempotency
                    new_version: 0,
                    event_id,
                });
            }
        }

        let now = Utc::now().timestamp_millis();

        // 读取旧值
        let old_value: Option<serde_json::Value> = db
            .query_row(
                "SELECT value FROM shared_state WHERE space_id = ?1 AND key = ?2",
                params![space_id, req.key],
                |row| {
                    let s: String = row.get(0)?;
                    Ok(serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
                },
            )
            .optional()
            .ok()
            .flatten();

        // 乐观锁检查
        if let Some(expected) = req.expected_version {
            let current: u64 = db
                .query_row(
                    "SELECT version FROM shared_state WHERE space_id = ?1 AND key = ?2",
                    params![space_id, req.key],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            if current != expected {
                return Err(GaggleError::ValidationError(format!(
                    "Version conflict: expected {}, got {}",
                    expected, current
                )));
            }
        }

        let previous_version = Self::get_version_locked(&db, space_id);

        // 递增 space version
        db.execute(
            "INSERT INTO space_state_versions (space_id, version, updated_at)
             VALUES (?1, 1, ?2)
             ON CONFLICT(space_id) DO UPDATE SET version = version + 1, updated_at = ?2",
            params![space_id, now],
        )?;

        let new_version: u64 = db.query_row(
            "SELECT version FROM space_state_versions WHERE space_id = ?1",
            params![space_id],
            |row| row.get(0),
        )?;

        let value_json = serde_json::to_string(&req.value)
            .map_err(|e| GaggleError::ValidationError(e.to_string()))?;

        // UPSERT 状态
        db.execute(
            "INSERT INTO shared_state (space_id, key, value, version, author_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(space_id, key) DO UPDATE SET
                value = excluded.value,
                version = excluded.version,
                author_id = excluded.author_id,
                updated_at = excluded.updated_at",
            params![space_id, req.key, value_json, new_version, req.author_id, now],
        )?;

        // 写入事件日志（带 hash chain）
        let event_id = Uuid::new_v4().to_string();
        let old_json = old_value
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        // Hash chain: get previous event's hash
        let prev_hash: String = db
            .query_row(
                "SELECT event_hash FROM state_events WHERE space_id = ?1 ORDER BY space_version DESC LIMIT 1",
                params![space_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or_else(|| GENESIS_HASH.to_string());

        let event_hash = compute_event_hash(
            &prev_hash,
            space_id,
            StateEventType::Set.as_str(),
            &req.key,
            &old_json,
            &Some(value_json.clone()),
            &req.author_id,
            new_version,
            now,
        );

        db.execute(
            "INSERT INTO state_events (id, space_id, event_type, key, old_value, new_value, author_id, space_version, timestamp, prev_hash, event_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                event_id,
                space_id,
                StateEventType::Set.as_str(),
                req.key,
                old_json,
                value_json,
                req.author_id,
                new_version,
                now,
                prev_hash,
                event_hash,
            ],
        )?;

        // 记录幂等键（如果提供）
        if let Some(ref idem_key) = req.idempotency_key {
            let _ = db.execute(
                "INSERT OR IGNORE INTO state_idempotency (space_id, idempotency_key, event_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![space_id, idem_key, event_id, now],
            );
        }

        Ok(StateUpdateResult {
            key: req.key,
            old_value,
            new_value: req.value,
            previous_version,
            new_version,
            event_id,
        })
    }

    /// 删除一个状态 key
    pub async fn delete(
        &self,
        space_id: &str,
        key: &str,
        author_id: &str,
    ) -> Result<StateUpdateResult, GaggleError> {
        let db = self.db.lock().await;
        let now = Utc::now().timestamp_millis();

        // 读取旧值
        let old_value: Option<serde_json::Value> = db
            .query_row(
                "SELECT value FROM shared_state WHERE space_id = ?1 AND key = ?2",
                params![space_id, key],
                |row| {
                    let s: String = row.get(0)?;
                    Ok(serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))
                },
            )
            .optional()
            .ok()
            .flatten();

        if old_value.is_none() {
            return Err(GaggleError::NotFound(format!(
                "State key '{}' not found in space",
                key
            )));
        }

        let previous_version = Self::get_version_locked(&db, space_id);

        // 递增 version
        db.execute(
            "INSERT INTO space_state_versions (space_id, version, updated_at)
             VALUES (?1, 1, ?2)
             ON CONFLICT(space_id) DO UPDATE SET version = version + 1, updated_at = ?2",
            params![space_id, now],
        )?;

        let new_version: u64 = db.query_row(
            "SELECT version FROM space_state_versions WHERE space_id = ?1",
            params![space_id],
            |row| row.get(0),
        )?;

        // 删除状态
        db.execute(
            "DELETE FROM shared_state WHERE space_id = ?1 AND key = ?2",
            params![space_id, key],
        )?;

        // 写入事件日志（带 hash chain）
        let event_id = Uuid::new_v4().to_string();
        let old_json = old_value
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        // Hash chain: get previous event's hash
        let prev_hash: String = db
            .query_row(
                "SELECT event_hash FROM state_events WHERE space_id = ?1 ORDER BY space_version DESC LIMIT 1",
                params![space_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or_else(|| GENESIS_HASH.to_string());

        let event_hash = compute_event_hash(
            &prev_hash,
            space_id,
            StateEventType::Delete.as_str(),
            key,
            &old_json,
            &None,
            author_id,
            new_version,
            now,
        );

        db.execute(
            "INSERT INTO state_events (id, space_id, event_type, key, old_value, new_value, author_id, space_version, timestamp, prev_hash, event_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, ?9, ?10)",
            params![
                event_id,
                space_id,
                StateEventType::Delete.as_str(),
                key,
                old_json,
                author_id,
                new_version,
                now,
                prev_hash,
                event_hash,
            ],
        )?;

        Ok(StateUpdateResult {
            key: key.to_string(),
            old_value,
            new_value: serde_json::Value::Null,
            previous_version,
            new_version,
            event_id,
        })
    }

    /// 获取 Space 的完整共享状态快照
    pub async fn get_snapshot(&self, space_id: &str) -> Result<SpaceStateSnapshot, GaggleError> {
        let db = self.db.lock().await;

        let version: u64 = db
            .query_row(
                "SELECT version FROM space_state_versions WHERE space_id = ?1",
                params![space_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let updated_at: i64 = db
            .query_row(
                "SELECT COALESCE(MAX(updated_at), 0) FROM shared_state WHERE space_id = ?1",
                params![space_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let mut stmt = db.prepare(
            "SELECT space_id, key, value, version, author_id, updated_at
             FROM shared_state WHERE space_id = ?1 ORDER BY key",
        )?;

        let entries = stmt
            .query_map(params![space_id], |row| {
                let value_str: String = row.get(2)?;
                Ok(StateEntry {
                    space_id: row.get(0)?,
                    key: row.get(1)?,
                    value: serde_json::from_str(&value_str).unwrap_or(serde_json::Value::Null),
                    version: row.get(3)?,
                    author_id: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(SpaceStateSnapshot {
            space_id: space_id.to_string(),
            version,
            entries,
            updated_at,
        })
    }

    /// 获取单个 key 的值
    pub async fn get_key(
        &self,
        space_id: &str,
        key: &str,
    ) -> Result<Option<StateEntry>, GaggleError> {
        let db = self.db.lock().await;
        let result = db
            .query_row(
                "SELECT space_id, key, value, version, author_id, updated_at
                 FROM shared_state WHERE space_id = ?1 AND key = ?2",
                params![space_id, key],
                |row| {
                    let value_str: String = row.get(2)?;
                    Ok(StateEntry {
                        space_id: row.get(0)?,
                        key: row.get(1)?,
                        value: serde_json::from_str(&value_str)
                            .unwrap_or(serde_json::Value::Null),
                        version: row.get(3)?,
                        author_id: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    /// 获取事件日志（按时间倒序，支持分页）
    pub async fn get_events(
        &self,
        space_id: &str,
        limit: usize,
        before_version: Option<u64>,
    ) -> Result<StateEventResponse, GaggleError> {
        let db = self.db.lock().await;

        // 总数
        let total: usize = db
            .query_row(
                "SELECT COUNT(*) FROM state_events WHERE space_id = ?1",
                params![space_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let events = if let Some(before_ver) = before_version {
            let mut stmt = db.prepare(
                "SELECT id, space_id, event_type, key, old_value, new_value, author_id, space_version, timestamp, prev_hash, event_hash
                 FROM state_events WHERE space_id = ?1 AND space_version < ?2
                 ORDER BY timestamp DESC LIMIT ?3",
            )?;
            Self::map_events(&mut stmt, params![space_id, before_ver, limit])?
        } else {
            let mut stmt = db.prepare(
                "SELECT id, space_id, event_type, key, old_value, new_value, author_id, space_version, timestamp, prev_hash, event_hash
                 FROM state_events WHERE space_id = ?1
                 ORDER BY timestamp DESC LIMIT ?2",
            )?;
            Self::map_events(&mut stmt, params![space_id, limit])?
        };

        Ok(StateEventResponse { events, total })
    }

    /// 获取指定版本之后的所有 state events（用于 state delta 同步）。
    /// 返回 (events, current_version) — events 按版本升序排列。
    pub async fn get_events_since(
        &self,
        space_id: &str,
        after_version: u64,
    ) -> Result<(Vec<StateEvent>, u64), GaggleError> {
        let db = self.db.lock().await;

        let current_version: u64 = db
            .query_row(
                "SELECT version FROM space_state_versions WHERE space_id = ?1",
                params![space_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let mut stmt = db.prepare(
            "SELECT id, space_id, event_type, key, old_value, new_value, author_id, space_version, timestamp, prev_hash, event_hash
             FROM state_events WHERE space_id = ?1 AND space_version > ?2
             ORDER BY space_version ASC, timestamp ASC",
        )?;
        let events = Self::map_events(&mut stmt, params![space_id, after_version])?;

        Ok((events, current_version))
    }

    // ── Agent Cursor（Memory Continuity）────────────────────

    /// 更新 Agent 在某个 Space 中的游标（last_seen_version）
    pub async fn update_cursor(
        &self,
        agent_id: &str,
        space_id: &str,
        version: u64,
    ) -> Result<(), GaggleError> {
        let db = self.db.lock().await;
        let now = Utc::now().timestamp_millis();
        db.execute(
            "INSERT INTO agent_cursors (agent_id, space_id, last_seen_version, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(agent_id, space_id) DO UPDATE SET
                last_seen_version = MAX(last_seen_version, excluded.last_seen_version),
                updated_at = excluded.updated_at",
            params![agent_id, space_id, version, now],
        )?;
        Ok(())
    }

    /// 获取 Agent 在某个 Space 中的游标。返回 None 表示从未同步过。
    pub async fn get_cursor(
        &self,
        agent_id: &str,
        space_id: &str,
    ) -> Result<Option<u64>, GaggleError> {
        let db = self.db.lock().await;
        let version: Option<u64> = db
            .query_row(
                "SELECT last_seen_version FROM agent_cursors
                 WHERE agent_id = ?1 AND space_id = ?2",
                params![agent_id, space_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(version)
    }

    /// 获取 Agent 在所有 Space 中的游标。返回 Vec<(space_id, last_seen_version)>。
    pub async fn get_all_cursors(
        &self,
        agent_id: &str,
    ) -> Result<Vec<(String, u64)>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT space_id, last_seen_version FROM agent_cursors
             WHERE agent_id = ?1 ORDER BY updated_at DESC",
        )?;
        let cursors = stmt
            .query_map(params![agent_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(cursors)
    }

    /// Get all agents' cursor positions for a given space.
    /// Returns Vec<(agent_id, last_seen_version)> — used for reality alignment visualization.
    pub async fn get_space_agent_cursors(
        &self,
        space_id: &str,
    ) -> Result<Vec<(String, u64)>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT agent_id, last_seen_version FROM agent_cursors
             WHERE space_id = ?1 ORDER BY last_seen_version DESC",
        )?;
        let cursors = stmt
            .query_map(params![space_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(cursors)
    }

    /// 从某个 version 开始重放事件，重建状态
    pub async fn reconstruct_at_version(
        &self,
        space_id: &str,
        target_version: u64,
    ) -> Result<SpaceStateSnapshot, GaggleError> {
        let db = self.db.lock().await;

        // 获取 target_version 及之前的所有事件
        let mut stmt = db.prepare(
            "SELECT id, space_id, event_type, key, old_value, new_value, author_id, space_version, timestamp, prev_hash, event_hash
             FROM state_events WHERE space_id = ?1 AND space_version <= ?2
             ORDER BY timestamp ASC",
        )?;
        let events = Self::map_events(&mut stmt, params![space_id, target_version])?;

        // 从空状态开始重放
        let mut state: std::collections::HashMap<String, (serde_json::Value, u64, String, i64)> =
            std::collections::HashMap::new();

        for event in &events {
            match event.event_type {
                StateEventType::Set => {
                    if let Some(ref val) = event.new_value {
                        state.insert(
                            event.key.clone(),
                            (val.clone(), event.space_version, event.author_id.clone(), event.timestamp),
                        );
                    }
                }
                StateEventType::Delete => {
                    state.remove(&event.key);
                }
            }
        }

        let entries: Vec<StateEntry> = state
            .into_iter()
            .map(|(key, (value, version, author_id, updated_at))| StateEntry {
                space_id: space_id.to_string(),
                key,
                value,
                version,
                author_id,
                updated_at,
            })
            .collect();

        Ok(SpaceStateSnapshot {
            space_id: space_id.to_string(),
            version: target_version,
            entries,
            updated_at: events
                .last()
                .map(|e| e.timestamp)
                .unwrap_or(0),
        })
    }

    /// 验证 materialized state 与 reconstructed state 一致性
    /// 这是 "reconstructed state = reality" 的终极证明
    /// 返回 (一致, materialized entries 数, reconstructed entries 数, 差异列表)
    pub async fn verify_state_integrity(
        &self,
        space_id: &str,
    ) -> Result<(bool, usize, usize, Vec<String>), GaggleError> {
        let current_version = self.get_version(space_id).await?;
        if current_version == 0 {
            return Ok((true, 0, 0, vec![]));
        }

        // 1. Get materialized state (shared_state table)
        let materialized = {
            let db = self.db.lock().await;
            let mut stmt = db.prepare(
                "SELECT key, value FROM shared_state WHERE space_id = ?1 ORDER BY key",
            )?;
            let rows: Vec<(String, String)> = stmt
                .query_map(params![space_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        }; // db lock released

        // 2. Reconstruct state from events
        let reconstructed = self.reconstruct_at_version(space_id, current_version).await?;

        // 3. Compare
        let mat_count = materialized.len();
        let rec_count = reconstructed.entries.len();
        let mut diffs = Vec::new();

        // Build reconstructed map for easy lookup
        let rec_map: std::collections::HashMap<String, String> = reconstructed
            .entries
            .iter()
            .map(|e| (e.key.clone(), serde_json::to_string(&e.value).unwrap_or_default()))
            .collect();

        // Build materialized map
        let mat_map: std::collections::HashMap<String, String> = materialized
            .into_iter()
            .collect();

        // Check keys in materialized but not in reconstructed
        for (key, val) in &mat_map {
            match rec_map.get(key) {
                Some(rec_val) if rec_val == val => {}
                Some(rec_val) => {
                    diffs.push(format!("key '{}' value mismatch: materialized={} reconstructed={}", key, val, rec_val));
                }
                None => {
                    diffs.push(format!("key '{}' in materialized but missing from reconstructed", key));
                }
            }
        }

        // Check keys in reconstructed but not in materialized
        for (key, val) in &rec_map {
            if !mat_map.contains_key(key) {
                diffs.push(format!("key '{}' in reconstructed but missing from materialized (value={})", key, val));
            }
        }

        let consistent = diffs.is_empty();
        Ok((consistent, mat_count, rec_count, diffs))
    }

    // ── 内部辅助 ──────────────────────────────────────

    fn get_version_locked(db: &Connection, space_id: &str) -> u64 {
        db.query_row(
            "SELECT version FROM space_state_versions WHERE space_id = ?1",
            params![space_id],
            |row| row.get(0),
        )
        .unwrap_or(0)
    }

    /// 验证 Space 的事件 hash chain 完整性
    /// 返回 (总事件数, 验证通过数, 验证失败数, 最新 event_hash)
    pub async fn verify_chain(
        &self,
        space_id: &str,
    ) -> Result<(usize, usize, usize, Option<String>), GaggleError> {
        let events = {
            let db = self.db.lock().await;
            let mut stmt = db.prepare(
                "SELECT id, space_id, event_type, key, old_value, new_value, author_id, space_version, timestamp, prev_hash, event_hash
                 FROM state_events WHERE space_id = ?1
                 ORDER BY space_version ASC, timestamp ASC",
            )?;
            let events = Self::map_events(&mut stmt, params![space_id])?;
            events
        }; // db lock released here

        let total = events.len();
        let mut verified = 0;
        let mut failed = 0;
        let mut prev_hash = GENESIS_HASH.to_string();

        for event in &events {
            let expected_hash = compute_event_hash(
                &prev_hash,
                &event.space_id,
                event.event_type.as_str(),
                &event.key,
                &event.old_value.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                &event.new_value.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                &event.author_id,
                event.space_version,
                event.timestamp,
            );

            match (&event.prev_hash, &event.event_hash) {
                (Some(ph), Some(eh)) if ph == &prev_hash && eh == &expected_hash => {
                    verified += 1;
                }
                (None, _) | (_, None) => {
                    // Legacy events without hash — count as verified (migrated data)
                    verified += 1;
                }
                _ => {
                    failed += 1;
                }
            }
            prev_hash = event.event_hash.clone().unwrap_or_default();
        }

        Ok((total, verified, failed, if total > 0 { Some(prev_hash) } else { None }))
    }

    fn map_events(
        stmt: &mut rusqlite::Statement,
        p: &[&dyn rusqlite::types::ToSql],
    ) -> Result<Vec<StateEvent>, GaggleError> {
        let events = stmt
            .query_map(p, |row| {
                let event_type_str: String = row.get(2)?;
                let old_str: Option<String> = row.get(4)?;
                let new_str: Option<String> = row.get(5)?;

                Ok(StateEvent {
                    id: row.get(0)?,
                    space_id: row.get(1)?,
                    event_type: StateEventType::from_str(&event_type_str),
                    key: row.get(3)?,
                    old_value: old_str
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    new_value: new_str
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    author_id: row.get(6)?,
                    space_version: row.get(7)?,
                    timestamp: row.get(8)?,
                    prev_hash: row.get(9).unwrap_or(None),
                    event_hash: row.get(10).unwrap_or(None),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(events)
    }
}

impl StateEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            StateEventType::Set => "set",
            StateEventType::Delete => "delete",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "delete" => StateEventType::Delete,
            _ => StateEventType::Set,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_events_since_delta_sync() {
        let mgr = SharedStateManager::new(":memory:").unwrap();

        // 写入 3 个 state 变更
        mgr.set(
            "s1",
            SetStateRequest {
                key: "price".to_string(),
                value: serde_json::json!(100),
                author_id: "agent_a".to_string(),
                expected_version: None,
                idempotency_key: None,
            },
        )
        .await
        .unwrap();
        mgr.set(
            "s1",
            SetStateRequest {
                key: "quantity".to_string(),
                value: serde_json::json!(50),
                author_id: "agent_a".to_string(),
                expected_version: None,
                idempotency_key: None,
            },
        )
        .await
        .unwrap();
        mgr.set(
            "s1",
            SetStateRequest {
                key: "price".to_string(),
                value: serde_json::json!(120),
                author_id: "agent_b".to_string(),
                expected_version: None,
                idempotency_key: None,
            },
        )
        .await
        .unwrap();

        // version 0 之后应该得到所有 3 个 events
        let (events, current_ver) = mgr.get_events_since("s1", 0).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(current_ver, 3);
        assert!(events[0].space_version <= events[1].space_version);

        // version 1 之后应该得到 2 个 events（version 2 和 3）
        let (events2, ver2) = mgr.get_events_since("s1", 1).await.unwrap();
        assert_eq!(events2.len(), 2);
        assert_eq!(ver2, 3);
        assert!(events2[0].space_version > 1);

        // version 3 之后应该得到 0 个 events（已是最新）
        let (events3, ver3) = mgr.get_events_since("s1", 3).await.unwrap();
        assert!(events3.is_empty());
        assert_eq!(ver3, 3);

        // 不存在的 space：空 events
        let (events4, ver4) = mgr.get_events_since("nonexistent", 0).await.unwrap();
        assert!(events4.is_empty());
        assert_eq!(ver4, 0);
    }

    #[tokio::test]
    async fn test_idempotent_set_returns_same_result() {
        let mgr = SharedStateManager::new(":memory:").unwrap();

        // 第一次写入
        let r1 = mgr.set(
            "s1",
            SetStateRequest {
                key: "price".to_string(),
                value: serde_json::json!(100),
                author_id: "agent_a".to_string(),
                expected_version: None,
                idempotency_key: Some("op_001".to_string()),
            },
        ).await.unwrap();
        assert_eq!(r1.new_value, serde_json::json!(100));
        assert!(!r1.event_id.is_empty());

        // 幂等重复：同样的 idempotency_key 应返回之前的结果
        let r2 = mgr.set(
            "s1",
            SetStateRequest {
                key: "price".to_string(),
                value: serde_json::json!(999),  // 不同的 value，但应该被忽略
                author_id: "agent_a".to_string(),
                expected_version: None,
                idempotency_key: Some("op_001".to_string()),
            },
        ).await.unwrap();
        // 幂等命中：返回原始 event_id
        assert_eq!(r2.event_id, r1.event_id);

        // 验证 state 只变化了一次（version 应该是 1，不是 2）
        let snap = mgr.get_snapshot("s1").await.unwrap();
        assert_eq!(snap.version, 1);
        assert_eq!(snap.entries[0].value, serde_json::json!(100));

        // 不同的 idempotency_key 应该正常执行
        let r3 = mgr.set(
            "s1",
            SetStateRequest {
                key: "price".to_string(),
                value: serde_json::json!(120),
                author_id: "agent_b".to_string(),
                expected_version: None,
                idempotency_key: Some("op_002".to_string()),
            },
        ).await.unwrap();
        assert_ne!(r3.event_id, r1.event_id);
        let snap2 = mgr.get_snapshot("s1").await.unwrap();
        assert_eq!(snap2.version, 2);
    }

    #[tokio::test]
    async fn test_set_without_idempotency_key_works_normally() {
        let mgr = SharedStateManager::new(":memory:").unwrap();

        // 不提供 idempotency_key — 行为与之前完全一致
        let r1 = mgr.set(
            "s1",
            SetStateRequest {
                key: "price".to_string(),
                value: serde_json::json!(100),
                author_id: "agent_a".to_string(),
                expected_version: None,
                idempotency_key: None,
            },
        ).await.unwrap();

        let r2 = mgr.set(
            "s1",
            SetStateRequest {
                key: "price".to_string(),
                value: serde_json::json!(200),
                author_id: "agent_a".to_string(),
                expected_version: None,
                idempotency_key: None,
            },
        ).await.unwrap();

        assert_ne!(r1.event_id, r2.event_id);
        let snap = mgr.get_snapshot("s1").await.unwrap();
        assert_eq!(snap.version, 2);
    }

    #[tokio::test]
    async fn test_agent_cursor_update_and_get() {
        let mgr = SharedStateManager::new(":memory:").unwrap();

        // 初始状态：无 cursor
        let cursor = mgr.get_cursor("agent_a", "space_1").await.unwrap();
        assert!(cursor.is_none());

        // 更新 cursor
        mgr.update_cursor("agent_a", "space_1", 5).await.unwrap();
        let cursor = mgr.get_cursor("agent_a", "space_1").await.unwrap();
        assert_eq!(cursor, Some(5));

        // 更新到更高版本
        mgr.update_cursor("agent_a", "space_1", 10).await.unwrap();
        let cursor = mgr.get_cursor("agent_a", "space_1").await.unwrap();
        assert_eq!(cursor, Some(10));

        // 更新到较低版本不应降级（MAX 保护）
        mgr.update_cursor("agent_a", "space_1", 3).await.unwrap();
        let cursor = mgr.get_cursor("agent_a", "space_1").await.unwrap();
        assert_eq!(cursor, Some(10)); // 仍然是 10
    }

    #[tokio::test]
    async fn test_agent_cursor_all_spaces() {
        let mgr = SharedStateManager::new(":memory:").unwrap();

        // 多个 space 的 cursor
        mgr.update_cursor("agent_a", "space_1", 5).await.unwrap();
        mgr.update_cursor("agent_a", "space_2", 8).await.unwrap();
        mgr.update_cursor("agent_a", "space_3", 1).await.unwrap();

        let cursors = mgr.get_all_cursors("agent_a").await.unwrap();
        assert_eq!(cursors.len(), 3);

        // 不同 agent 的 cursor 互不干扰
        mgr.update_cursor("agent_b", "space_1", 99).await.unwrap();
        let cursor_a = mgr.get_cursor("agent_a", "space_1").await.unwrap();
        assert_eq!(cursor_a, Some(5));

        let cursor_b = mgr.get_cursor("agent_b", "space_1").await.unwrap();
        assert_eq!(cursor_b, Some(99));
    }

    #[tokio::test]
    async fn test_agent_cursor_resume_flow() {
        let mgr = SharedStateManager::new(":memory:").unwrap();

        // 模拟 resume flow：写入 3 个 state 变更
        mgr.set("s1", SetStateRequest {
            key: "price".to_string(),
            value: serde_json::json!(100),
            author_id: "agent_a".to_string(),
            expected_version: None,
            idempotency_key: None,
        }).await.unwrap();
        mgr.set("s1", SetStateRequest {
            key: "quantity".to_string(),
            value: serde_json::json!(50),
            author_id: "agent_a".to_string(),
            expected_version: None,
            idempotency_key: None,
        }).await.unwrap();
        mgr.set("s1", SetStateRequest {
            key: "price".to_string(),
            value: serde_json::json!(120),
            author_id: "agent_b".to_string(),
            expected_version: None,
            idempotency_key: None,
        }).await.unwrap();

        // Agent A 之前看到 version 1，重连后
        mgr.update_cursor("agent_a", "s1", 1).await.unwrap();

        // Resume: 使用 cursor 获取 delta
        let cursor = mgr.get_cursor("agent_a", "s1").await.unwrap().unwrap_or(0);
        let (events, current_ver) = mgr.get_events_since("s1", cursor).await.unwrap();
        assert_eq!(events.len(), 2); // version 2 和 3
        assert_eq!(current_ver, 3);

        // 更新 cursor
        mgr.update_cursor("agent_a", "s1", current_ver).await.unwrap();
        let cursor = mgr.get_cursor("agent_a", "s1").await.unwrap();
        assert_eq!(cursor, Some(3));

        // 再次 resume：无新事件
        let (events2, ver2) = mgr.get_events_since("s1", cursor.unwrap()).await.unwrap();
        assert!(events2.is_empty());
        assert_eq!(ver2, 3);
    }
}
