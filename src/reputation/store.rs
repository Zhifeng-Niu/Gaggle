//! 信誉数据存储层

use super::types::*;
use crate::error::GaggleError;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ReputationStore {
    db: Arc<Mutex<Connection>>,
}

impl ReputationStore {
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;

        // 信誉事件表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS reputation_events (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                space_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                outcome TEXT NOT NULL,
                rating INTEGER,
                counterparty_id TEXT NOT NULL,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;

        // 信誉摘要表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS reputation_summary (
                agent_id TEXT PRIMARY KEY,
                total_negotiations INTEGER DEFAULT 0,
                successful INTEGER DEFAULT 0,
                avg_rating REAL,
                fulfillment_rate REAL DEFAULT 0.0,
                reputation_score REAL DEFAULT 0.0,
                last_updated INTEGER NOT NULL
            )",
            [],
        )?;

        // 索引
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reputation_events_agent_id ON reputation_events(agent_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reputation_events_space_id ON reputation_events(space_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_reputation_events_created_at ON reputation_events(created_at)",
            [],
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// 创建信誉事件
    pub async fn create_event(&self, event: ReputationEvent) -> Result<(), GaggleError> {
        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO reputation_events (id, agent_id, space_id, event_type, outcome, rating, counterparty_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.id,
                event.agent_id,
                event.space_id,
                serde_json::to_string(&event.event_type)?,
                serde_json::to_string(&event.outcome)?,
                event.rating,
                event.counterparty_id,
                event.created_at,
            ],
        )?;
        Ok(())
    }

    /// 获取 Agent 的信誉摘要
    pub async fn get_summary(
        &self,
        agent_id: &str,
    ) -> Result<Option<ReputationSummary>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT agent_id, total_negotiations, successful, avg_rating, fulfillment_rate, reputation_score, last_updated
             FROM reputation_summary WHERE agent_id = ?1",
        )?;

        let summary = stmt
            .query_row(params![agent_id], |row| {
                Ok(ReputationSummary {
                    agent_id: row.get(0)?,
                    total_negotiations: row.get(1)?,
                    successful: row.get(2)?,
                    avg_rating: row.get(3)?,
                    fulfillment_rate: row.get(4)?,
                    reputation_score: row.get(5)?,
                    last_updated: row.get(6)?,
                })
            })
            .optional()?;

        Ok(summary)
    }

    /// 获取 Agent 的所有事件
    pub async fn get_agent_events(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<ReputationEvent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, agent_id, space_id, event_type, outcome, rating, counterparty_id, created_at
             FROM reputation_events
             WHERE agent_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;

        let events = stmt.query_map(params![agent_id, limit as i32], |row| {
            let event_type_str: String = row.get(3)?;
            let outcome_str: String = row.get(4)?;

            Ok(ReputationEvent {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                space_id: row.get(2)?,
                event_type: serde_json::from_str(&event_type_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                outcome: serde_json::from_str(&outcome_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                rating: row.get(5)?,
                counterparty_id: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;

        let mut result = Vec::new();
        for event in events {
            result.push(event?);
        }
        Ok(result)
    }

    /// 获取指定时间范围内的事件（用于计算 recency_weight）
    pub async fn get_events_in_range(
        &self,
        agent_id: &str,
        since: i64,
    ) -> Result<Vec<ReputationEvent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, agent_id, space_id, event_type, outcome, rating, counterparty_id, created_at
             FROM reputation_events
             WHERE agent_id = ?1 AND created_at >= ?2
             ORDER BY created_at ASC",
        )?;

        let events = stmt.query_map(params![agent_id, since], |row| {
            let event_type_str: String = row.get(3)?;
            let outcome_str: String = row.get(4)?;

            Ok(ReputationEvent {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                space_id: row.get(2)?,
                event_type: serde_json::from_str(&event_type_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                outcome: serde_json::from_str(&outcome_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                rating: row.get(5)?,
                counterparty_id: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;

        let mut result = Vec::new();
        for event in events {
            result.push(event?);
        }
        Ok(result)
    }

    /// 获取所有事件（用于重算）
    pub async fn get_all_agent_events(
        &self,
        agent_id: &str,
    ) -> Result<Vec<ReputationEvent>, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT id, agent_id, space_id, event_type, outcome, rating, counterparty_id, created_at
             FROM reputation_events
             WHERE agent_id = ?1
             ORDER BY created_at ASC",
        )?;

        let events = stmt.query_map(params![agent_id], |row| {
            let event_type_str: String = row.get(3)?;
            let outcome_str: String = row.get(4)?;

            Ok(ReputationEvent {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                space_id: row.get(2)?,
                event_type: serde_json::from_str(&event_type_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                outcome: serde_json::from_str(&outcome_str)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                rating: row.get(5)?,
                counterparty_id: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;

        let mut result = Vec::new();
        for event in events {
            result.push(event?);
        }
        Ok(result)
    }

    /// 更新或创建信誉摘要
    pub async fn upsert_summary(&self, summary: &ReputationSummary) -> Result<(), GaggleError> {
        let db = self.db.lock().await;
        db.execute(
            "INSERT INTO reputation_summary (agent_id, total_negotiations, successful, avg_rating, fulfillment_rate, reputation_score, last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(agent_id) DO UPDATE SET
                total_negotiations = ?2,
                successful = ?3,
                avg_rating = ?4,
                fulfillment_rate = ?5,
                reputation_score = ?6,
                last_updated = ?7",
            params![
                summary.agent_id,
                summary.total_negotiations,
                summary.successful,
                summary.avg_rating,
                summary.fulfillment_rate,
                summary.reputation_score,
                summary.last_updated,
            ],
        )?;
        Ok(())
    }

    /// 检查是否已有评分记录（防止重复评分）
    pub async fn has_rating_for_space(
        &self,
        agent_id: &str,
        space_id: &str,
    ) -> Result<bool, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT COUNT(*) as cnt FROM reputation_events
             WHERE agent_id = ?1 AND space_id = ?2 AND rating IS NOT NULL",
        )?;

        let count: i64 = stmt.query_row(params![agent_id, space_id], |row| row.get(0))?;
        Ok(count > 0)
    }
}
