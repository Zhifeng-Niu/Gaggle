//! Space Audit Trace — 所有 Space 重要动作的 append-only 审计日志。
//!
//! 回答：谁触发了什么？什么时候？效果是什么？
//! 与 state_events（状态重建专用）不同，space_audit 覆盖所有 Space 级别动作。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::GaggleError;

/// Space 级别动作类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    // 协商核心
    SpaceCreated,
    SpaceJoined,
    SpaceLeft,
    SpaceClosed,
    SpaceStatusTransition,
    MessageSent,
    ProposalSubmitted,
    ProposalResponded,
    BestTermsShared,
    RoundAdvanced,

    // Shared Reality
    StateSet,
    StateDeleted,

    // 执行引擎
    ContractCreated,
    MilestoneSubmitted,
    MilestoneAccepted,

    // 制度层
    RulesUpdated,
    CoalitionCreated,
    CoalitionJoined,
    CoalitionLeft,
    DelegationCreated,
    DelegationRevoked,
    RecruitmentCreated,
    RecruitmentAccepted,

    // RFP
    RfpCreated,
    ProposalsEvaluated,

    // 安全/验证
    RuleCheckDenied,
    ChainVerified,
}

impl AuditAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SpaceCreated => "space_created",
            Self::SpaceJoined => "space_joined",
            Self::SpaceLeft => "space_left",
            Self::SpaceClosed => "space_closed",
            Self::SpaceStatusTransition => "space_status_transition",
            Self::MessageSent => "message_sent",
            Self::ProposalSubmitted => "proposal_submitted",
            Self::ProposalResponded => "proposal_responded",
            Self::BestTermsShared => "best_terms_shared",
            Self::RoundAdvanced => "round_advanced",
            Self::StateSet => "state_set",
            Self::StateDeleted => "state_deleted",
            Self::ContractCreated => "contract_created",
            Self::MilestoneSubmitted => "milestone_submitted",
            Self::MilestoneAccepted => "milestone_accepted",
            Self::RulesUpdated => "rules_updated",
            Self::CoalitionCreated => "coalition_created",
            Self::CoalitionJoined => "coalition_joined",
            Self::CoalitionLeft => "coalition_left",
            Self::DelegationCreated => "delegation_created",
            Self::DelegationRevoked => "delegation_revoked",
            Self::RecruitmentCreated => "recruitment_created",
            Self::RecruitmentAccepted => "recruitment_accepted",
            Self::RfpCreated => "rfp_created",
            Self::ProposalsEvaluated => "proposals_evaluated",
            Self::RuleCheckDenied => "rule_check_denied",
            Self::ChainVerified => "chain_verified",
        }
    }
}

/// 审计日志条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub space_id: String,
    pub agent_id: String,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub details: Option<serde_json::Value>,
    pub created_at: i64,
    /// 关联 ID：同一操作链条中的事件共享同一个 correlation_id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// 该事件发生时的 shared state 版本号
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_version: Option<u64>,
}

/// 动作类别（前端过滤用）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionCategory {
    Negotiation,  // 协商核心
    State,        // 共享现实
    Execution,    // 执行引擎
    Institution,  // 制度层
    Security,     // 安全/验证
}

impl AuditAction {
    pub fn category(&self) -> ActionCategory {
        match self {
            Self::SpaceCreated
            | Self::SpaceJoined
            | Self::SpaceLeft
            | Self::SpaceClosed
            | Self::SpaceStatusTransition
            | Self::MessageSent
            | Self::ProposalSubmitted
            | Self::ProposalResponded
            | Self::BestTermsShared
            | Self::RoundAdvanced
            | Self::RfpCreated
            | Self::ProposalsEvaluated => ActionCategory::Negotiation,

            Self::StateSet | Self::StateDeleted => ActionCategory::State,

            Self::ContractCreated
            | Self::MilestoneSubmitted
            | Self::MilestoneAccepted => ActionCategory::Execution,

            Self::RulesUpdated
            | Self::CoalitionCreated
            | Self::CoalitionJoined
            | Self::CoalitionLeft
            | Self::DelegationCreated
            | Self::DelegationRevoked
            | Self::RecruitmentCreated
            | Self::RecruitmentAccepted => ActionCategory::Institution,

            Self::RuleCheckDenied | Self::ChainVerified => ActionCategory::Security,
        }
    }
}

#[derive(Clone)]
pub struct TraceStore {
    db: Arc<Mutex<Connection>>,
}

impl TraceStore {
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;"
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS space_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                space_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                action TEXT NOT NULL,
                target_type TEXT,
                target_id TEXT,
                details TEXT,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_space_time
             ON space_audit (space_id, created_at DESC)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_agent_time
             ON space_audit (agent_id, created_at DESC)",
            [],
        )?;

        // Phase 14 migration: correlation_id + state_version
        let _ = conn.execute_batch(
            "ALTER TABLE space_audit ADD COLUMN correlation_id TEXT;
             ALTER TABLE space_audit ADD COLUMN state_version INTEGER;",
        );
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_correlation
             ON space_audit (correlation_id)",
            [],
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// 记录一个审计事件。details 是可选的 JSON（before/after diff 等）。
    pub async fn log_action(
        &self,
        space_id: &str,
        agent_id: &str,
        action: AuditAction,
        target_type: Option<&str>,
        target_id: Option<&str>,
        details: Option<serde_json::Value>,
    ) -> Result<i64, GaggleError> {
        self.log_action_ex(
            space_id,
            agent_id,
            action,
            target_type,
            target_id,
            details,
            None,
            None,
        )
        .await
    }

    /// 记录审计事件（含 correlation_id 和 state_version）。
    pub async fn log_action_ex(
        &self,
        space_id: &str,
        agent_id: &str,
        action: AuditAction,
        target_type: Option<&str>,
        target_id: Option<&str>,
        details: Option<serde_json::Value>,
        correlation_id: Option<&str>,
        state_version: Option<u64>,
    ) -> Result<i64, GaggleError> {
        let db = self.db.lock().await;
        let now = chrono::Utc::now().timestamp_millis();
        let details_str = details
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        db.execute(
            "INSERT INTO space_audit (space_id, agent_id, action, target_type, target_id, details, created_at, correlation_id, state_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                space_id,
                agent_id,
                action.as_str(),
                target_type,
                target_id,
                details_str,
                now,
                correlation_id,
                state_version,
            ],
        )?;

        let id = db.last_insert_rowid();
        Ok(id)
    }

    /// 查询 Space 审计日志（按时间倒序）。
    pub async fn query_trace(
        &self,
        space_id: &str,
        limit: usize,
        before_id: Option<i64>,
        action_filter: Option<&str>,
    ) -> Result<Vec<AuditEntry>, GaggleError> {
        self.query_trace_ex(space_id, limit, before_id, action_filter, None)
            .await
    }

    /// 查询审计日志（含 agent 过滤）。
    pub async fn query_trace_ex(
        &self,
        space_id: &str,
        limit: usize,
        before_id: Option<i64>,
        action_filter: Option<&str>,
        agent_filter: Option<&str>,
    ) -> Result<Vec<AuditEntry>, GaggleError> {
        let db = self.db.lock().await;

        let mut sql = String::from(
            "SELECT id, space_id, agent_id, action, target_type, target_id, details, created_at, correlation_id, state_version \
             FROM space_audit WHERE space_id = ?1",
        );
        let mut param_idx = 2u32;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(space_id.to_string())];

        if let Some(bid) = before_id {
            sql.push_str(&format!(" AND id < ?{}", param_idx));
            params.push(Box::new(bid));
            param_idx += 1;
        }
        if let Some(act) = action_filter {
            sql.push_str(&format!(" AND action = ?{}", param_idx));
            params.push(Box::new(act.to_string()));
            param_idx += 1;
        }
        if let Some(ag) = agent_filter {
            sql.push_str(&format!(" AND agent_id = ?{}", param_idx));
            params.push(Box::new(ag.to_string()));
            param_idx += 1;
        }

        sql.push_str(&format!(
            " ORDER BY created_at DESC, id DESC LIMIT ?{}",
            param_idx
        ));
        params.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = db.prepare(&sql)?;
        let entries = stmt
            .query_map(param_refs.as_slice(), |row| {
                let details_str: Option<String> = row.get(6)?;
                let details = details_str.and_then(|s| serde_json::from_str(&s).ok());
                Ok(AuditEntry {
                    id: row.get(0)?,
                    space_id: row.get(1)?,
                    agent_id: row.get(2)?,
                    action: row.get(3)?,
                    target_type: row.get(4)?,
                    target_id: row.get(5)?,
                    details,
                    created_at: row.get(7)?,
                    correlation_id: row.get(8)?,
                    state_version: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// Trace 统计：按 action 类别分组计数。
    pub async fn trace_stats(
        &self,
        space_id: &str,
    ) -> Result<serde_json::Value, GaggleError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT action, COUNT(*) as cnt FROM space_audit WHERE space_id = ?1 GROUP BY action ORDER BY cnt DESC",
        )?;
        let rows: Vec<(String, i64)> = stmt
            .query_map(params![space_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let total: i64 = rows.iter().map(|(_, c)| *c).sum();
        let by_action: serde_json::Map<String, serde_json::Value> = rows
            .into_iter()
            .map(|(action, cnt)| (action, serde_json::Value::from(cnt)))
            .collect();

        // 按类别聚合
        let mut by_category = serde_json::Map::new();
        for (action, cnt) in &by_action {
            let cat = AuditAction::from_str(action)
                .map(|a| a.category())
                .unwrap_or(ActionCategory::Negotiation);
            let cat_str = serde_json::to_value(&cat)
                .unwrap_or_else(|_| serde_json::Value::String("negotiation".into()));
            let cat_key = cat_str.as_str().unwrap_or("negotiation");
            let current = by_category
                .get(cat_key)
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            by_category.insert(
                cat_key.to_string(),
                serde_json::Value::from(current + cnt.as_i64().unwrap_or(0)),
            );
        }

        Ok(serde_json::json!({
            "space_id": space_id,
            "total": total,
            "by_action": by_action,
            "by_category": by_category,
        }))
    }
}

/// Parse action string back to AuditAction for category lookup.
impl AuditAction {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "space_created" => Some(Self::SpaceCreated),
            "space_joined" => Some(Self::SpaceJoined),
            "space_left" => Some(Self::SpaceLeft),
            "space_closed" => Some(Self::SpaceClosed),
            "space_status_transition" => Some(Self::SpaceStatusTransition),
            "message_sent" => Some(Self::MessageSent),
            "proposal_submitted" => Some(Self::ProposalSubmitted),
            "proposal_responded" => Some(Self::ProposalResponded),
            "best_terms_shared" => Some(Self::BestTermsShared),
            "round_advanced" => Some(Self::RoundAdvanced),
            "state_set" => Some(Self::StateSet),
            "state_deleted" => Some(Self::StateDeleted),
            "contract_created" => Some(Self::ContractCreated),
            "milestone_submitted" => Some(Self::MilestoneSubmitted),
            "milestone_accepted" => Some(Self::MilestoneAccepted),
            "rules_updated" => Some(Self::RulesUpdated),
            "coalition_created" => Some(Self::CoalitionCreated),
            "coalition_joined" => Some(Self::CoalitionJoined),
            "coalition_left" => Some(Self::CoalitionLeft),
            "delegation_created" => Some(Self::DelegationCreated),
            "delegation_revoked" => Some(Self::DelegationRevoked),
            "recruitment_created" => Some(Self::RecruitmentCreated),
            "recruitment_accepted" => Some(Self::RecruitmentAccepted),
            "rfp_created" => Some(Self::RfpCreated),
            "proposals_evaluated" => Some(Self::ProposalsEvaluated),
            "rule_check_denied" => Some(Self::RuleCheckDenied),
            "chain_verified" => Some(Self::ChainVerified),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_log_and_query_trace() {
        let store = TraceStore::new(":memory:").unwrap();

        store
            .log_action(
                "space_1",
                "agent_a",
                AuditAction::MessageSent,
                Some("message"),
                Some("msg_001"),
                Some(serde_json::json!({"content": "hello"})),
            )
            .await
            .unwrap();

        store
            .log_action(
                "space_1",
                "agent_b",
                AuditAction::StateSet,
                Some("state_key"),
                Some("price"),
                Some(serde_json::json!({"key": "price", "value": "100", "old_value": null})),
            )
            .await
            .unwrap();

        store
            .log_action(
                "space_1",
                "agent_a",
                AuditAction::ProposalSubmitted,
                Some("proposal"),
                Some("prop_001"),
                None,
            )
            .await
            .unwrap();

        // Query all
        let entries = store.query_trace("space_1", 10, None, None).await.unwrap();
        assert_eq!(entries.len(), 3);
        // Most recent first
        assert_eq!(entries[0].action, "proposal_submitted");
        assert_eq!(entries[1].action, "state_set");
        assert_eq!(entries[2].action, "message_sent");

        // Filter by action
        let state_entries = store
            .query_trace("space_1", 10, None, Some("state_set"))
            .await
            .unwrap();
        assert_eq!(state_entries.len(), 1);
        assert_eq!(state_entries[0].agent_id, "agent_b");

        // Pagination with before_id
        let page = store
            .query_trace("space_1", 10, Some(entries[1].id), None)
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].action, "message_sent");
    }

    #[tokio::test]
    async fn test_query_empty_space() {
        let store = TraceStore::new(":memory:").unwrap();
        let entries = store.query_trace("nonexistent", 10, None, None).await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_details_json_preserved() {
        let store = TraceStore::new(":memory:").unwrap();

        let details = serde_json::json!({
            "key": "price",
            "value": "100",
            "old_value": "80",
            "version_before": 3,
            "version_after": 4
        });

        store
            .log_action("s1", "a1", AuditAction::StateSet, Some("state_key"), Some("price"), Some(details.clone()))
            .await
            .unwrap();

        let entries = store.query_trace("s1", 10, None, None).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].details.as_ref().unwrap()["version_before"], 3);
        assert_eq!(entries[0].details.as_ref().unwrap()["version_after"], 4);
    }

    #[tokio::test]
    async fn test_log_action_ex_with_correlation() {
        let store = TraceStore::new(":memory:").unwrap();

        let corr_id = "corr-001";
        store
            .log_action_ex(
                "s1",
                "agent_a",
                AuditAction::RuleCheckDenied,
                Some("state_key"),
                Some("price"),
                Some(serde_json::json!({"role": "observer", "reason": "no_write_state"})),
                Some(corr_id),
                Some(5),
            )
            .await
            .unwrap();

        store
            .log_action_ex(
                "s1",
                "agent_a",
                AuditAction::StateSet,
                Some("state_key"),
                Some("price"),
                Some(serde_json::json!({"value": "200"})),
                Some(corr_id),
                Some(6),
            )
            .await
            .unwrap();

        let entries = store.query_trace("s1", 10, None, None).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].correlation_id.as_deref(), Some(corr_id));
        assert_eq!(entries[0].state_version, Some(6));
        assert_eq!(entries[1].correlation_id.as_deref(), Some(corr_id));
        assert_eq!(entries[1].state_version, Some(5));
    }

    #[tokio::test]
    async fn test_agent_filter() {
        let store = TraceStore::new(":memory:").unwrap();

        store
            .log_action("s1", "agent_a", AuditAction::MessageSent, None, None, None)
            .await
            .unwrap();
        store
            .log_action("s1", "agent_b", AuditAction::MessageSent, None, None, None)
            .await
            .unwrap();
        store
            .log_action("s1", "agent_a", AuditAction::ProposalSubmitted, None, None, None)
            .await
            .unwrap();

        // Filter by agent_a
        let a_entries = store
            .query_trace_ex("s1", 10, None, None, Some("agent_a"))
            .await
            .unwrap();
        assert_eq!(a_entries.len(), 2);
        assert!(a_entries.iter().all(|e| e.agent_id == "agent_a"));

        // Filter by agent_b
        let b_entries = store
            .query_trace_ex("s1", 10, None, None, Some("agent_b"))
            .await
            .unwrap();
        assert_eq!(b_entries.len(), 1);
        assert_eq!(b_entries[0].agent_id, "agent_b");
    }

    #[tokio::test]
    async fn test_trace_stats() {
        let store = TraceStore::new(":memory:").unwrap();

        store
            .log_action("s1", "a1", AuditAction::MessageSent, None, None, None)
            .await
            .unwrap();
        store
            .log_action("s1", "a1", AuditAction::MessageSent, None, None, None)
            .await
            .unwrap();
        store
            .log_action("s1", "a1", AuditAction::StateSet, None, None, None)
            .await
            .unwrap();
        store
            .log_action("s1", "a1", AuditAction::RulesUpdated, None, None, None)
            .await
            .unwrap();

        let stats = store.trace_stats("s1").await.unwrap();
        assert_eq!(stats["total"], 4);
        assert_eq!(stats["by_action"]["message_sent"], 2);
        assert_eq!(stats["by_action"]["state_set"], 1);
        assert_eq!(stats["by_category"]["negotiation"], 2);
        assert_eq!(stats["by_category"]["state"], 1);
        assert_eq!(stats["by_category"]["institution"], 1);
    }

    #[tokio::test]
    async fn test_action_category() {
        assert_eq!(AuditAction::MessageSent.category(), ActionCategory::Negotiation);
        assert_eq!(AuditAction::StateSet.category(), ActionCategory::State);
        assert_eq!(AuditAction::ContractCreated.category(), ActionCategory::Execution);
        assert_eq!(AuditAction::RulesUpdated.category(), ActionCategory::Institution);
        assert_eq!(AuditAction::RuleCheckDenied.category(), ActionCategory::Security);
        assert_eq!(AuditAction::ChainVerified.category(), ActionCategory::Security);
    }
}
