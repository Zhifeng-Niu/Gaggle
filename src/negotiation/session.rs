//! Session管理

use crate::agents::Agent;
use crate::error::GaggleError;
use crate::negotiation::crypt::generate_key;
use crate::negotiation::proposal::{
    BestTermsShared, CreateRfpRequest, DimensionScores, EvaluateResponse, EvaluationWeights,
    Proposal, ProposalResponseAction, ProposalScore, ProposalStatus, ProposalType,
    RespondToProposalRequest, RoundInfo, RoundStatus, ShareBestTermsRequest, SubmitProposalRequest,
    quality_tier_score,
};
use crate::negotiation::space::{
    CloseSpaceRequest, CreateSpaceRequest, EncryptedContent, MessageVisibility, SendMessageRequest,
    Space, SpaceMessage, SpaceStatus, SpaceType,
};
use crate::negotiation::rules::SpaceRules;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use uuid::Uuid;

/// Space 行列索引常量（与 SELECT 列顺序对应）
/// SELECT id, name, creator_id, agent_ids, joined_agent_ids, status, space_type, rfp_context, context, encryption_key, created_at, updated_at, closed_at, buyer_id, seller_id, rules
const COL_ID: usize = 0;
const COL_NAME: usize = 1;
const COL_CREATOR_ID: usize = 2;
const COL_AGENT_IDS: usize = 3;
const COL_JOINED_IDS: usize = 4;
const COL_STATUS: usize = 5;
const COL_SPACE_TYPE: usize = 6;
const COL_RFP_CONTEXT: usize = 7;
const COL_CONTEXT: usize = 8;
const COL_ENC_KEY: usize = 9;
const COL_CREATED_AT: usize = 10;
const COL_UPDATED_AT: usize = 11;
const COL_CLOSED_AT: usize = 12;
const COL_BUYER_ID: usize = 13;
const COL_SELLER_ID: usize = 14;
const COL_RULES: usize = 15;
const COL_PENDING_JOINS: usize = 16;
const COL_VERSION: usize = 17;

const SPACE_COLUMNS: &str = "id, name, creator_id, agent_ids, joined_agent_ids, status, space_type, rfp_context, context, encryption_key, created_at, updated_at, closed_at, buyer_id, seller_id, rules, pending_join_requests, version";

fn map_space(row: &rusqlite::Row) -> rusqlite::Result<Space> {
    let agent_ids_str: String = row.get(COL_AGENT_IDS)?;
    let joined_ids_str: String = row.get(COL_JOINED_IDS)?;
    let status_str: String = row.get(COL_STATUS)?;
    let space_type_str: String = row.get(COL_SPACE_TYPE)?;
    let rfp_context_str: Option<String> = row.get(COL_RFP_CONTEXT)?;
    let context_str: String = row.get(COL_CONTEXT)?;

    // rules 列：优先读取序列化 JSON，为空则从 space_type 推导默认规则
    let rules_str: Option<String> = row.get(COL_RULES).ok().flatten();
    let space_type = SpaceType::from_str_safe(&space_type_str);
    let rules = rules_str
        .and_then(|s| serde_json::from_str::<SpaceRules>(&s).ok())
        .filter(|r| *r != SpaceRules::default() || space_type_str == "bilateral" || space_type_str == "rfp")
        .unwrap_or_else(|| SpaceRules::from_space_type(&space_type));

    Ok(Space {
        id: row.get(COL_ID)?,
        name: row.get(COL_NAME)?,
        creator_id: row.get(COL_CREATOR_ID)?,
        agent_ids: serde_json::from_str(&agent_ids_str).unwrap_or_default(),
        joined_agent_ids: serde_json::from_str(&joined_ids_str).unwrap_or_default(),
        status: serde_json::from_str(&status_str).unwrap_or(SpaceStatus::Created),
        space_type: space_type.clone(),
        rules,
        rfp_context: rfp_context_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .filter(|v: &serde_json::Value| !v.is_null()),
        context: serde_json::from_str(&context_str).unwrap_or(serde_json::json!({})),
        encryption_key: row.get(COL_ENC_KEY)?,
        created_at: row.get(COL_CREATED_AT)?,
        updated_at: row.get(COL_UPDATED_AT)?,
        closed_at: row.get(COL_CLOSED_AT)?,
        buyer_id: row.get(COL_BUYER_ID)?,
        seller_id: row.get(COL_SELLER_ID)?,
        pending_join_requests: row.get(COL_PENDING_JOINS)
            .ok()
            .flatten()
            .and_then(|s: String| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        version: row.get(COL_VERSION).ok().unwrap_or(1),
    })
}

pub struct SpaceManager {
    db: Arc<Mutex<Connection>>,
    spaces: dashmap::DashMap<String, Space>,
    broadcast_txs: tokio::sync::RwLock<HashMap<String, broadcast::Sender<String>>>,
}

impl SpaceManager {
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;

        // 启用 WAL 模式：允许读写并发，显著提升高并发吞吐
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS spaces (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                creator_id TEXT NOT NULL,
                agent_ids TEXT NOT NULL,
                joined_agent_ids TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL,
                space_type TEXT NOT NULL DEFAULT 'bilateral',
                rfp_context TEXT,
                context TEXT NOT NULL,
                encryption_key TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                closed_at INTEGER
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS space_messages (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                msg_type TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                round INTEGER NOT NULL,
                metadata TEXT,
                visibility TEXT NOT NULL DEFAULT 'broadcast',
                recipient_ids TEXT DEFAULT '[]',
                FOREIGN KEY (space_id) REFERENCES spaces(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_messages_space ON space_messages(space_id)",
            [],
        )?;

        // 创建 proposals 表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS proposals (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                proposal_type TEXT NOT NULL,
                dimensions TEXT NOT NULL,
                round INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                parent_proposal_id TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (space_id) REFERENCES spaces(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_proposals_space ON proposals(space_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_proposals_sender ON proposals(sender_id)",
            [],
        )?;

        // 向后兼容：添加新列（如果不存在）
        let _ = conn.execute(
            "ALTER TABLE spaces ADD COLUMN space_type TEXT NOT NULL DEFAULT 'bilateral'",
            [],
        );
        let _ = conn.execute("ALTER TABLE spaces ADD COLUMN rfp_context TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE space_messages ADD COLUMN visibility TEXT NOT NULL DEFAULT 'broadcast'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE space_messages ADD COLUMN recipient_ids TEXT DEFAULT '[]'",
            [],
        );
        let _ = conn.execute("ALTER TABLE spaces ADD COLUMN buyer_id TEXT", []);
        let _ = conn.execute("ALTER TABLE spaces ADD COLUMN seller_id TEXT", []);
        // Phase 6: SpaceRules
        let _ = conn.execute("ALTER TABLE spaces ADD COLUMN rules TEXT", []);
        let _ = conn.execute("ALTER TABLE spaces ADD COLUMN pending_join_requests TEXT", []);
        // Phase P1: Space version for optimistic locking
        let _ = conn.execute("ALTER TABLE spaces ADD COLUMN version INTEGER NOT NULL DEFAULT 1", []);

        // Phase 9: SubSpaces
        Self::init_subspace_table(&conn)?;

        // Phase 10: Coalitions
        Self::init_coalition_table(&conn)?;

        // Phase 11: Delegations
        Self::init_delegation_table(&conn)?;

        // Phase 12: Recruitment
        Self::init_recruitment_table(&conn)?;

        // Phase P0: Deterministic State Machine Transition Log
        conn.execute(
            "CREATE TABLE IF NOT EXISTS space_status_transitions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                space_id TEXT NOT NULL,
                from_status TEXT NOT NULL,
                to_status TEXT NOT NULL,
                trigger TEXT NOT NULL,
                agent_id TEXT,
                space_version INTEGER NOT NULL,
                timestamp INTEGER NOT NULL,
                prev_hash TEXT,
                transition_hash TEXT
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_transitions_space_time
             ON space_status_transitions (space_id, timestamp ASC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_transitions_space_version
             ON space_status_transitions (space_id, space_version ASC)",
            [],
        )?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            spaces: dashmap::DashMap::new(),
            broadcast_txs: tokio::sync::RwLock::new(HashMap::new()),
        })
    }

    pub async fn create_space(
        &self,
        creator: &Agent,
        req: CreateSpaceRequest,
        my_role: Option<String>,
    ) -> Result<Space, GaggleError> {
        self.create_space_with_rules(creator, req, my_role, None).await
    }

    /// 创建 Space（可选 rules 覆盖）
    pub async fn create_space_with_rules(
        &self,
        creator: &Agent,
        req: CreateSpaceRequest,
        my_role: Option<String>,
        rules_overrides: Option<crate::negotiation::rules::SpaceRulesOverrides>,
    ) -> Result<Space, GaggleError> {
        let encryption_key = generate_key();

        let mut space = Space::new(
            req.name,
            creator.id.clone(),
            req.invitee_ids,
            req.context,
            encryption_key,
            my_role,
        );

        // 应用 rules 覆盖（如果有）
        if let Some(overrides) = rules_overrides {
            overrides.apply_to(&mut space.rules);
        }

        {
            let db = self.db.lock().unwrap();
            db.execute(
                &format!("INSERT INTO spaces ({SPACE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)"),
                params![
                    space.id,
                    space.name,
                    space.creator_id,
                    serde_json::to_string(&space.agent_ids)?,
                    serde_json::to_string(&space.joined_agent_ids)?,
                    serde_json::to_string(&space.status)?,
                    space.space_type.as_str(),
                    space.rfp_context.as_ref().and_then(|v| serde_json::to_string(v).ok()),
                    serde_json::to_string(&space.context)?,
                    space.encryption_key,
                    space.created_at,
                    space.updated_at,
                    Option::<i64>::None,
                    space.buyer_id,
                    space.seller_id,
                    serde_json::to_string(&space.rules)?,
                    serde_json::to_string(&space.pending_join_requests)?,
                    space.version,
                ],
            )?;
        }

        self.spaces.insert(space.id.clone(), space.clone());

        let (tx, _) = broadcast::channel::<String>(512);
        let mut broadcast_txs = self.broadcast_txs.write().await;
        broadcast_txs.insert(space.id.clone(), tx);

        Ok(space)
    }

    /// 持久化 Space 到数据库
    pub(crate) fn persist_space(&self, space: &Space) -> Result<(), GaggleError> {
        // Optimistic locking: only update if the DB version matches what we loaded.
        // version was already bumped by bump_version() before this call,
        // so we check against (space.version - 1).
        let expected_version = space.version - 1;
        let db = self.db.lock().unwrap();
        let rows = db.execute(
            "UPDATE spaces SET joined_agent_ids = ?1, agent_ids = ?2, status = ?3, updated_at = ?4, buyer_id = ?5, seller_id = ?6, pending_join_requests = ?7, rules = ?8, version = ?9 WHERE id = ?10 AND version = ?11",
            params![
                serde_json::to_string(&space.joined_agent_ids)?,
                serde_json::to_string(&space.agent_ids)?,
                serde_json::to_string(&space.status)?,
                space.updated_at,
                space.buyer_id,
                space.seller_id,
                serde_json::to_string(&space.pending_join_requests)?,
                serde_json::to_string(&space.rules)?,
                space.version,
                space.id,
                expected_version,
            ],
        )?;
        if rows == 0 {
            return Err(GaggleError::Conflict(format!(
                "Space {} version conflict: expected {}, space may have been modified concurrently",
                space.id, expected_version
            )));
        }
        Ok(())
    }

    /// 更新内存缓存中的 Space
    pub async fn update_cache(&self, space: &Space) {
        self.spaces.insert(space.id.clone(), space.clone());
    }

    /// 检查并应用规则 transitions
    /// 返回是否有规则被变更
    fn check_and_apply_transitions(
        &self,
        space: &mut Space,
        trigger: crate::negotiation::rules::RuleTrigger,
    ) -> Result<bool, GaggleError> {
        let changes = space.rules.check_transitions(&trigger);
        if changes.is_empty() {
            return Ok(false);
        }

        // 按顺序应用所有匹配的规则覆盖
        for change in &changes {
            change.apply_to(&mut space.rules);
        }
        space.updated_at = Utc::now().timestamp_millis();
        self.persist_space(space)?;
        Ok(true)
    }

    pub async fn get_space(&self, space_id: &str) -> Result<Option<Space>, GaggleError> {
        {
            if let Some(space) = self.spaces.get(space_id) {
                return Ok(Some(space.clone()));
            }
        }

        let result = {
            let db = self.db.lock().unwrap();
            let mut stmt =
                db.prepare(&format!("SELECT {SPACE_COLUMNS} FROM spaces WHERE id = ?1"))?;

            stmt.query_row(params![space_id], map_space).optional()?
        };

        if let Some(ref s) = result {
            self.spaces.insert(s.id.clone(), s.clone());
        }

        Ok(result)
    }

    pub async fn join_space(&self, agent: &Agent, space_id: &str) -> Result<Space, GaggleError> {
        let mut space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if space.status.is_terminal() {
            return Err(GaggleError::SpaceClosed(
                "Space is not accepting new members".to_string(),
            ));
        }

        // 如果已经 join 过，直接返回
        if space.joined_agent_ids.contains(&agent.id) {
            return Ok(space);
        }

        use crate::negotiation::rules::JoinPolicy;
        match space.rules.join_policy {
            JoinPolicy::InviteOnly => {
                // 仅允许 agent_ids 中的成员加入
                if !space.is_member(&agent.id) {
                    return Err(GaggleError::Forbidden(
                        "Agent not invited to this space".to_string(),
                    ));
                }
            }
            JoinPolicy::Open => {
                // 任何已注册 agent 可加入，检查 max_participants
                if let Some(max) = space.rules.max_participants {
                    if space.joined_agent_ids.len() >= max {
                        return Err(GaggleError::Forbidden(
                            "Space has reached maximum number of participants".to_string(),
                        ));
                    }
                }
                // 将 agent 添加到 agent_ids（如果不在其中）
                if !space.agent_ids.contains(&agent.id) {
                    space.agent_ids.push(agent.id.clone());
                }
            }
            JoinPolicy::ApprovalRequired => {
                // 已受邀成员可直接加入
                if space.is_member(&agent.id) {
                    // 继续走正常加入流程
                } else {
                    // 未受邀 → 添加到 pending_join_requests
                    if space.pending_join_requests.iter().any(|(id, _)| id == &agent.id) {
                        return Err(GaggleError::Forbidden(
                            "Join request already pending".to_string(),
                        ));
                    }
                    space.pending_join_requests.push((agent.id.clone(), Utc::now().timestamp_millis()));
                    space.bump_version();
                    self.persist_space(&space)?;
                    self.spaces.insert(space.id.clone(), space.clone());
                    return Err(GaggleError::Forbidden(
                        "Join request submitted, awaiting approval".to_string(),
                    ));
                }
            }
        }

        // 加入
        space.joined_agent_ids.push(agent.id.clone());
        space.updated_at = Utc::now().timestamp_millis();

        // 计算并分配角色
        if space.buyer_id.is_none() && space.seller_id.as_deref() != Some(&agent.id) {
            space.buyer_id = Some(agent.id.clone());
        } else if space.seller_id.is_none() && space.buyer_id.as_deref() != Some(&agent.id) {
            space.seller_id = Some(agent.id.clone());
        }

        // 检查是否所有成员都已加入
        let should_activate = space.all_joined();
        if should_activate {
            if let Ok(_t) = space.activate() {
                let _ = self.record_transition(
                    &space.id, "created", "active",
                    "all_agents_joined", Some(&agent.id), space.version,
                );
            }
        }

        // Phase 13: 检查 OnSpaceActivated 和 OnMemberCount transitions
        if should_activate {
            let _ = self.check_and_apply_transitions(&mut space, crate::negotiation::rules::RuleTrigger::OnSpaceActivated);
        }
        let member_count = space.joined_agent_ids.len();
        let _ = self.check_and_apply_transitions(&mut space, crate::negotiation::rules::RuleTrigger::OnMemberCount { count: member_count });

        space.bump_version();
        self.persist_space(&space)?;
        self.spaces.insert(space.id.clone(), space.clone());

        Ok(space)
    }

    /// 审批 join request（仅 creator 可审批）
    pub async fn approve_join_request(
        &self,
        approver_id: &str,
        space_id: &str,
        target_agent_id: &str,
    ) -> Result<Space, GaggleError> {
        let mut space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if space.creator_id != approver_id {
            return Err(GaggleError::Forbidden(
                "Only the space creator can approve join requests".to_string(),
            ));
        }

        // 找到 pending request
        let idx = space
            .pending_join_requests
            .iter()
            .position(|(id, _)| id == target_agent_id)
            .ok_or_else(|| GaggleError::NotFound("No pending join request from this agent".to_string()))?;

        // 移除 pending request
        space.pending_join_requests.remove(idx);

        // 检查 max_participants
        if let Some(max) = space.rules.max_participants {
            if space.joined_agent_ids.len() >= max {
                return Err(GaggleError::Forbidden(
                    "Space has reached maximum number of participants".to_string(),
                ));
            }
        }

        // 加入 agent
        if !space.agent_ids.contains(&target_agent_id.to_string()) {
            space.agent_ids.push(target_agent_id.to_string());
        }
        space.joined_agent_ids.push(target_agent_id.to_string());
        space.updated_at = Utc::now().timestamp_millis();

        // 分配角色
        if space.buyer_id.is_none() && space.seller_id.as_deref() != Some(target_agent_id) {
            space.buyer_id = Some(target_agent_id.to_string());
        } else if space.seller_id.is_none() && space.buyer_id.as_deref() != Some(target_agent_id) {
            space.seller_id = Some(target_agent_id.to_string());
        }

        if space.all_joined() && space.status == SpaceStatus::Created {
            space.activate().map_err(|e| GaggleError::ValidationError(e))?;
            let _ = self.record_transition(
                &space.id, "created", "active",
                "all_agents_joined", Some(target_agent_id), space.version,
            );
            let _ = self.check_and_apply_transitions(&mut space, crate::negotiation::rules::RuleTrigger::OnSpaceActivated);
        }
        let member_count = space.joined_agent_ids.len();
        let _ = self.check_and_apply_transitions(&mut space, crate::negotiation::rules::RuleTrigger::OnMemberCount { count: member_count });

        space.bump_version();
        self.persist_space(&space)?;
        self.spaces.insert(space.id.clone(), space.clone());

        Ok(space)
    }

    /// 拒绝 join request（仅 creator 可拒绝）
    pub async fn reject_join_request(
        &self,
        rejector_id: &str,
        space_id: &str,
        target_agent_id: &str,
    ) -> Result<Space, GaggleError> {
        let mut space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if space.creator_id != rejector_id {
            return Err(GaggleError::Forbidden(
                "Only the space creator can reject join requests".to_string(),
            ));
        }

        let idx = space
            .pending_join_requests
            .iter()
            .position(|(id, _)| id == target_agent_id)
            .ok_or_else(|| GaggleError::NotFound("No pending join request from this agent".to_string()))?;

        space.pending_join_requests.remove(idx);
        space.bump_version();

        self.persist_space(&space)?;
        self.spaces.insert(space.id.clone(), space.clone());

        Ok(space)
    }

    /// 创建 RFP Space（多方谈判）
    pub async fn create_rfp(
        &self,
        creator: &Agent,
        req: CreateRfpRequest,
    ) -> Result<Space, GaggleError> {
        let encryption_key = generate_key();

        let space = Space::new_rfp(
            req.name,
            creator.id.clone(),
            req.provider_ids,
            req.rfp_context,
            req.context,
            encryption_key,
        );

        {
            let db = self.db.lock().unwrap();
            db.execute(
                &format!("INSERT INTO spaces ({SPACE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)"),
                params![
                    space.id,
                    space.name,
                    space.creator_id,
                    serde_json::to_string(&space.agent_ids)?,
                    serde_json::to_string(&space.joined_agent_ids)?,
                    serde_json::to_string(&space.status)?,
                    space.space_type.as_str(),
                    space.rfp_context.as_ref().and_then(|v| serde_json::to_string(v).ok()),
                    serde_json::to_string(&space.context)?,
                    space.encryption_key,
                    space.created_at,
                    space.updated_at,
                    Option::<i64>::None,
                    space.buyer_id,
                    space.seller_id,
                    serde_json::to_string(&space.rules)?,
                    serde_json::to_string(&space.pending_join_requests)?,
                    space.version,
                ],
            )?;
        }

        self.spaces.insert(space.id.clone(), space.clone());

        let (tx, _) = broadcast::channel::<String>(512);
        let mut broadcast_txs = self.broadcast_txs.write().await;
        broadcast_txs.insert(space.id.clone(), tx);

        Ok(space)
    }

    pub async fn send_message(
        &self,
        agent: &Agent,
        space_id: &str,
        req: SendMessageRequest,
    ) -> Result<SpaceMessage, GaggleError> {
        let space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if !space.is_member(&agent.id) {
            return Err(GaggleError::Forbidden(
                "Agent not member of this space".to_string(),
            ));
        }

        if space.status.is_terminal() {
            return Err(GaggleError::SpaceClosed(format!(
                "Space is in terminal state: {}",
                space.status.as_str()
            )));
        }

        let count = {
            let db = self.db.lock().unwrap();
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM space_messages WHERE space_id = ?1",
                params![space_id],
                |row| row.get(0),
            )?;
            count
        };

        // Capacity governance: enforce max_messages limit from SpaceRules
        if let Some(max) = space.rules.max_messages {
            if count as usize >= max {
                return Err(GaggleError::ValidationError(format!(
                    "Space capacity exceeded: {} messages (limit: {})",
                    count, max
                )));
            }
        }

        let round = space.current_round(count as u32 + 1);

        let message = SpaceMessage::new(
            space_id.to_string(),
            agent.id.clone(),
            req.msg_type,
            req.content,
            round,
            req.metadata,
        );

        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO space_messages (id, space_id, sender_id, msg_type, content, timestamp, round, metadata, visibility, recipient_ids)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    message.id,
                    message.space_id,
                    message.sender_id,
                    serde_json::to_string(&message.msg_type)?,
                    serde_json::to_string(&message.content)?,
                    message.timestamp,
                    message.round,
                    serde_json::to_string(&message.metadata)?,
                    message.visibility.as_str(),
                    serde_json::to_string(&message.recipient_ids)?,
                ],
            )?;
        }

        Ok(message)
    }

    /// 发送定向消息（RFP 谈判中消费者与单一 Provider 私密沟通）
    pub async fn send_directed_message(
        &self,
        agent: &Agent,
        space_id: &str,
        req: SendMessageRequest,
        recipient_ids: Vec<String>,
    ) -> Result<SpaceMessage, GaggleError> {
        let space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if !space.is_member(&agent.id) {
            return Err(GaggleError::Forbidden(
                "Agent not member of this space".to_string(),
            ));
        }

        if space.status.is_terminal() {
            return Err(GaggleError::SpaceClosed(format!(
                "Space is in terminal state: {}",
                space.status.as_str()
            )));
        }

        let count = {
            let db = self.db.lock().unwrap();
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM space_messages WHERE space_id = ?1",
                params![space_id],
                |row| row.get(0),
            )?;
            count
        };

        // Capacity governance: enforce max_messages limit from SpaceRules
        if let Some(max) = space.rules.max_messages {
            if count as usize >= max {
                return Err(GaggleError::ValidationError(format!(
                    "Space capacity exceeded: {} messages (limit: {})",
                    count, max
                )));
            }
        }

        let round = space.current_round(count as u32 + 1);

        let message = SpaceMessage::new_directed(
            space_id.to_string(),
            agent.id.clone(),
            req.msg_type,
            req.content,
            round,
            recipient_ids,
            req.metadata,
        );

        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO space_messages (id, space_id, sender_id, msg_type, content, timestamp, round, metadata, visibility, recipient_ids)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    message.id,
                    message.space_id,
                    message.sender_id,
                    serde_json::to_string(&message.msg_type)?,
                    serde_json::to_string(&message.content)?,
                    message.timestamp,
                    message.round,
                    serde_json::to_string(&message.metadata)?,
                    message.visibility.as_str(),
                    serde_json::to_string(&message.recipient_ids)?,
                ],
            )?;
        }

        Ok(message)
    }

    pub async fn get_messages(
        &self,
        space_id: &str,
        after: Option<i64>,
        limit: u32,
    ) -> Result<Vec<SpaceMessage>, GaggleError> {
        self.get_messages_for_agent(space_id, None, after, limit)
            .await
    }

    /// 获取特定 Agent 可见的消息
    pub async fn get_messages_for_agent(
        &self,
        space_id: &str,
        agent_id: Option<&str>,
        after: Option<i64>,
        limit: u32,
    ) -> Result<Vec<SpaceMessage>, GaggleError> {
        let all_messages = {
            let db = self.db.lock().unwrap();

            if let Some(after_ts) = after {
                let sql =
                    "SELECT id, space_id, sender_id, msg_type, content, timestamp, round, metadata, visibility, recipient_ids
                           FROM space_messages WHERE space_id = ?1 AND timestamp > ?2
                           ORDER BY timestamp ASC LIMIT ?3";
                let mut stmt = db.prepare(sql)?;
                let rows = stmt
                    .query_map(params![space_id, after_ts, limit], |row| {
                        Self::map_message(row)
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            } else {
                let sql =
                    "SELECT id, space_id, sender_id, msg_type, content, timestamp, round, metadata, visibility, recipient_ids
                           FROM space_messages WHERE space_id = ?1
                           ORDER BY timestamp ASC LIMIT ?2";
                let mut stmt = db.prepare(sql)?;
                let rows = stmt
                    .query_map(params![space_id, limit], Self::map_message)?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            }
        };

        // 解密旧格式消息（content 仍然以 {"cipher":...} 存储在 DB 中的记录）
        let enc_key = self
            .get_space(space_id)
            .await?
            .map(|s| s.encryption_key);

        let all_messages = if let Some(ref key) = enc_key {
            all_messages
                .into_iter()
                .map(|mut msg| {
                    if msg.content.starts_with('{') {
                        if let Ok(enc) =
                            serde_json::from_str::<EncryptedContent>(&msg.content)
                        {
                            if let Ok(plain) =
                                crate::negotiation::crypt::decrypt_content(&enc, key)
                            {
                                msg.content = plain;
                            }
                        }
                    }
                    msg
                })
                .collect()
        } else {
            all_messages
        };

        // 如果指定了 agent_id，过滤可见性
        if let Some(agent) = agent_id {
            Ok(all_messages
                .into_iter()
                .filter(|msg| msg.is_visible_to(agent))
                .collect())
        } else {
            Ok(all_messages)
        }
    }

    fn map_message(row: &rusqlite::Row) -> rusqlite::Result<SpaceMessage> {
        let msg_type_str: String = row.get(3)?;
        let content_str: String = row.get(4)?;
        let metadata_str: String = row.get(7)?;
        let visibility_str: String = row.get(8)?;
        let recipient_ids_str: String = row.get(9)?;

        // content 在 DB 中有两种格式：
        // 新格式（明文）：带引号的 JSON 字符串 → "\"hello\""
        // 旧格式（加密）：EncryptedContent JSON → '{"cipher":"...","nonce":"...","version":1}'
        let content = if content_str.starts_with('"') {
            // 新格式：JSON 字符串，去掉引号
            serde_json::from_str::<String>(&content_str).unwrap_or(content_str)
        } else {
            // 旧格式或其他：保留原样，上层 get_messages_for_agent 会处理解密
            content_str
        };

        Ok(SpaceMessage {
            id: row.get(0)?,
            space_id: row.get(1)?,
            sender_id: row.get(2)?,
            msg_type: serde_json::from_str(&msg_type_str)
                .unwrap_or(crate::negotiation::message::MessageType::Text),
            content,
            timestamp: row.get(5)?,
            round: row.get(6)?,
            metadata: serde_json::from_str(&metadata_str).ok(),
            visibility: match visibility_str.as_str() {
                "directed" => MessageVisibility::Directed,
                "private" => MessageVisibility::Private,
                _ => MessageVisibility::Broadcast,
            },
            recipient_ids: serde_json::from_str(&recipient_ids_str).unwrap_or_default(),
        })
    }

    pub async fn close_space(
        &self,
        agent: &Agent,
        space_id: &str,
        req: CloseSpaceRequest,
    ) -> Result<Space, GaggleError> {
        let mut space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if !space.is_member(&agent.id) {
            return Err(GaggleError::Forbidden(
                "Agent not member of this space".to_string(),
            ));
        }

        let concluded = req.conclusion == "concluded";
        let expected_version = space.version;
        let transition = space.close(concluded, "close_request", Some(&agent.id)).map_err(|e| GaggleError::ValidationError(e))?;
        tracing::info!(
            space_id = %space.id, from = ?transition.from, to = ?transition.to,
            trigger = %transition.trigger, agent_id = ?transition.agent_id,
            "Space status transition"
        );

        // Record transition to append-only log before bumping version
        let _ = self.record_transition(
            &space.id,
            transition.from.as_str(),
            transition.to.as_str(),
            &transition.trigger,
            transition.agent_id.as_deref(),
            space.version,
        );

        space.bump_version();

        {
            let db = self.db.lock().unwrap();
            let rows = db.execute(
                "UPDATE spaces SET status = ?1, updated_at = ?2, closed_at = ?3, version = ?4 WHERE id = ?5 AND version = ?6",
                params![
                    serde_json::to_string(&space.status)?,
                    space.updated_at,
                    space.closed_at,
                    space.version,
                    space_id,
                    expected_version,
                ],
            )?;
            if rows == 0 {
                return Err(GaggleError::Conflict(format!(
                    "Space {} version conflict during close", space_id
                )));
            }
        }

        self.spaces.insert(space.id.clone(), space.clone());

        Ok(space)
    }

    pub async fn get_agent_spaces(&self, agent_id: &str) -> Result<Vec<Space>, GaggleError> {
        let search_pattern = format!("%\"{}\"%", agent_id);

        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            &format!("SELECT {SPACE_COLUMNS} FROM spaces WHERE creator_id = ?1 OR agent_ids LIKE ?2 ORDER BY updated_at DESC")
        )?;

        let spaces = stmt
            .query_map(params![agent_id, search_pattern], map_space)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(spaces)
    }

    pub async fn get_broadcast_tx(&self, space_id: &str) -> Option<broadcast::Sender<String>> {
        let broadcast_txs = self.broadcast_txs.read().await;
        broadcast_txs.get(space_id).cloned()
    }

    /// 获取 Space 总数
    pub async fn count_spaces(&self) -> Result<usize, GaggleError> {
        let db = self.db.lock().unwrap();
        let count: i64 =
            db.query_row("SELECT COUNT(*) FROM spaces", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    // ── Deterministic State Machine Transition Log ──────────

    /// Record a state machine transition to the append-only log with hash chain.
    /// This is the single source of truth for "how did the Space state evolve."
    ///
    /// Every call to space.activate(), space.close(), or expire_space() must
    /// invoke this to maintain the deterministic transition history.
    pub(crate) fn record_transition(
        &self,
        space_id: &str,
        from_status: &str,
        to_status: &str,
        trigger: &str,
        agent_id: Option<&str>,
        space_version: u64,
    ) -> Result<crate::negotiation::space::PersistedTransition, GaggleError> {
        use crate::negotiation::space::{
            PersistedTransition, compute_transition_hash, TRANSITION_GENESIS_HASH,
        };

        let db = self.db.lock().unwrap();
        let now = Utc::now().timestamp_millis();

        // Get previous transition's hash for chain integrity
        let prev_hash: String = db
            .query_row(
                "SELECT transition_hash FROM space_status_transitions
                 WHERE space_id = ?1 ORDER BY id DESC LIMIT 1",
                rusqlite::params![space_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten()
            .unwrap_or_else(|| TRANSITION_GENESIS_HASH.to_string());

        let transition_hash = compute_transition_hash(
            &prev_hash,
            space_id,
            from_status,
            to_status,
            trigger,
            space_version,
            now,
        );

        db.execute(
            "INSERT INTO space_status_transitions
             (space_id, from_status, to_status, trigger, agent_id, space_version, timestamp, prev_hash, transition_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                space_id,
                from_status,
                to_status,
                trigger,
                agent_id,
                space_version,
                now,
                prev_hash,
                transition_hash,
            ],
        )?;

        let id = db.last_insert_rowid();

        Ok(PersistedTransition {
            id,
            space_id: space_id.to_string(),
            from_status: from_status.to_string(),
            to_status: to_status.to_string(),
            trigger: trigger.to_string(),
            agent_id: agent_id.map(|s| s.to_string()),
            space_version,
            timestamp: now,
            prev_hash: Some(prev_hash),
            transition_hash: Some(transition_hash),
        })
    }

    /// Query the transition history for a Space.
    /// Returns transitions in chronological order with hash chain intact.
    pub async fn get_transition_history(
        &self,
        space_id: &str,
        limit: usize,
        before_id: Option<i64>,
    ) -> Result<crate::negotiation::space::TransitionHistory, GaggleError> {
        use crate::negotiation::space::TransitionHistory;

        let db = self.db.lock().unwrap();

        let total: usize = db
            .query_row(
                "SELECT COUNT(*) FROM space_status_transitions WHERE space_id = ?1",
                rusqlite::params![space_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let transitions = if let Some(bid) = before_id {
            let mut stmt = db.prepare(
                "SELECT id, space_id, from_status, to_status, trigger, agent_id,
                        space_version, timestamp, prev_hash, transition_hash
                 FROM space_status_transitions
                 WHERE space_id = ?1 AND id < ?2
                 ORDER BY timestamp ASC, id ASC LIMIT ?3",
            )?;
            Self::map_transitions(&mut stmt, rusqlite::params![space_id, bid, limit])?
        } else {
            let mut stmt = db.prepare(
                "SELECT id, space_id, from_status, to_status, trigger, agent_id,
                        space_version, timestamp, prev_hash, transition_hash
                 FROM space_status_transitions
                 WHERE space_id = ?1
                 ORDER BY timestamp ASC, id ASC LIMIT ?2",
            )?;
            Self::map_transitions(&mut stmt, rusqlite::params![space_id, limit])?
        };

        // Get current status and version
        let (current_status, current_version) = db
            .query_row(
                "SELECT status, version FROM spaces WHERE id = ?1",
                rusqlite::params![space_id],
                |row| {
                    let status: String = row.get(0)?;
                    let version: u64 = row.get(1).unwrap_or(0);
                    Ok((status, version))
                },
            )
            .optional()?
            .unwrap_or(("unknown".to_string(), 0));

        Ok(TransitionHistory {
            space_id: space_id.to_string(),
            transitions,
            total,
            current_status,
            current_version,
        })
    }

    /// Verify the hash chain integrity of a Space's transition log.
    /// Returns (total_transitions, verified_count, failed_count).
    pub async fn verify_transition_chain(
        &self,
        space_id: &str,
    ) -> Result<(usize, usize, usize), GaggleError> {
        use crate::negotiation::space::TRANSITION_GENESIS_HASH;

        let transitions = {
            let db = self.db.lock().unwrap();
            let mut stmt = db.prepare(
                "SELECT id, space_id, from_status, to_status, trigger, agent_id,
                        space_version, timestamp, prev_hash, transition_hash
                 FROM space_status_transitions
                 WHERE space_id = ?1
                 ORDER BY timestamp ASC, id ASC",
            )?;
            Self::map_transitions(&mut stmt, rusqlite::params![space_id])?
        };

        let total = transitions.len();
        let mut verified = 0;
        let mut failed = 0;
        let mut prev_hash = TRANSITION_GENESIS_HASH.to_string();

        for t in &transitions {
            let expected = crate::negotiation::space::compute_transition_hash(
                &prev_hash,
                &t.space_id,
                &t.from_status,
                &t.to_status,
                &t.trigger,
                t.space_version,
                t.timestamp,
            );

            match (&t.prev_hash, &t.transition_hash) {
                (Some(ph), Some(th)) if ph == &prev_hash && th == &expected => {
                    verified += 1;
                }
                _ => {
                    failed += 1;
                }
            }
            prev_hash = t.transition_hash.clone().unwrap_or_default();
        }

        Ok((total, verified, failed))
    }

    fn map_transitions(
        stmt: &mut rusqlite::Statement,
        params: &[&dyn rusqlite::types::ToSql],
    ) -> Result<Vec<crate::negotiation::space::PersistedTransition>, GaggleError> {
        let transitions = stmt
            .query_map(params, |row| {
                Ok(crate::negotiation::space::PersistedTransition {
                    id: row.get(0)?,
                    space_id: row.get(1)?,
                    from_status: row.get(2)?,
                    to_status: row.get(3)?,
                    trigger: row.get(4)?,
                    agent_id: row.get(5)?,
                    space_version: row.get(6)?,
                    timestamp: row.get(7)?,
                    prev_hash: row.get(8)?,
                    transition_hash: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(transitions)
    }

    // ── Space Lifecycle Governance ──────────────────────────

    /// Find all active spaces whose round deadline has passed.
    /// Returns (space_id, agent_ids) pairs for notification.
    pub async fn find_expired_spaces(&self) -> Result<Vec<(String, Vec<String>)>, GaggleError> {
        let now_ms = Utc::now().timestamp_millis();
        let db = self.db.lock().unwrap();

        // Find active/created spaces with rules containing a deadline that has passed
        let mut stmt = db.prepare(
            "SELECT id, agent_ids, rules FROM spaces WHERE status IN ('\"active\"', '\"created\"', 'active', 'created')",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let agent_ids_str: String = row.get(1)?;
            let rules_str: String = row.get(2)?;
            Ok((id, agent_ids_str, rules_str))
        })?;

        let mut expired = Vec::new();
        for row in rows {
            let (id, agent_ids_str, rules_str) = row?;
            let rules: SpaceRules = serde_json::from_str(&rules_str).unwrap_or_default();

            // Check if deadline exists and has passed
            let deadline_ms = rules.rounds.as_ref().and_then(|r| r.deadline);
            if let Some(dl) = deadline_ms {
                if dl <= now_ms {
                    let agent_ids: Vec<String> =
                        serde_json::from_str(&agent_ids_str).unwrap_or_default();
                    expired.push((id, agent_ids));
                }
            }
        }

        Ok(expired)
    }

    /// Transition a space to Expired status. No agent authorization required
    /// — this is a system-level lifecycle enforcement.
    pub async fn expire_space(&self, space_id: &str) -> Result<Space, GaggleError> {
        let mut space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if !space.status.can_transition_to(&SpaceStatus::Expired) {
            return Ok(space); // Already terminal, skip
        }

        let old_status_str = space.status.as_str().to_string();
        let now_ms = Utc::now().timestamp_millis();
        let expected_version = space.version;

        // Record transition to append-only log before mutating space
        let _ = self.record_transition(
            space_id, &old_status_str, "expired",
            "lifecycle_governor", None::<&str>, space.version,
        );

        space.status = SpaceStatus::Expired;
        space.updated_at = now_ms;
        space.closed_at = Some(now_ms);
        space.version += 1;

        {
            let db = self.db.lock().unwrap();
            let rows = db.execute(
                "UPDATE spaces SET status = ?1, updated_at = ?2, closed_at = ?3, version = ?4 WHERE id = ?5 AND version = ?6",
                params![
                    serde_json::to_string(&space.status)?,
                    space.updated_at,
                    space.closed_at,
                    space.version,
                    space_id,
                    expected_version,
                ],
            )?;
            if rows == 0 {
                return Err(GaggleError::Conflict(format!(
                    "Space {} version conflict during expiry: expected {}",
                    space_id, expected_version
                )));
            }
        }

        self.spaces.insert(space.id.clone(), space.clone());
        tracing::info!(space_id = %space_id, "Space expired by lifecycle governor");
        Ok(space)
    }

    // ==================== RFP Proposal 相关方法 ====================

    /// 提交结构化提案
    pub async fn submit_proposal(
        &self,
        agent: &Agent,
        space_id: &str,
        req: SubmitProposalRequest,
    ) -> Result<Proposal, GaggleError> {
        let space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if !space.is_member(&agent.id) {
            return Err(GaggleError::Forbidden(
                "Agent not member of this space".to_string(),
            ));
        }

        // State machine guard: reject if space is in terminal state
        if space.status.is_terminal() {
            return Err(GaggleError::SpaceClosed(format!(
                "Cannot submit proposal: space is {}",
                space.status.as_str()
            )));
        }

        // 规则驱动：检查 agent 角色是否有权提案
        let agent_role = space.get_role(&agent.id).unwrap_or("member");
        if !space.rules.role_can_propose(agent_role) {
            // 向后兼容：RFP creator 无权提案
            if agent.id == space.creator_id && space.rules.has_rounds() {
                return Err(GaggleError::Forbidden(
                    "RFP creator cannot submit proposals".to_string(),
                ));
            }
            return Err(GaggleError::Forbidden(
                format!("Role '{}' cannot submit proposals in this space", agent_role),
            ));
        }

        // 获取当前轮次 + capacity check
        let round = {
            let db = self.db.lock().unwrap();
            let count: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM proposals WHERE space_id = ?1",
                    params![space_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Capacity governance: enforce max_proposals limit from SpaceRules
            if let Some(max) = space.rules.max_proposals {
                if count as usize >= max {
                    return Err(GaggleError::ValidationError(format!(
                        "Space proposal capacity exceeded: {} proposals (limit: {})",
                        count, max
                    )));
                }
            }

            count as u32 + 1
        };

        let proposal = Proposal::new(
            space_id.to_string(),
            agent.id.clone(),
            req.proposal_type,
            req.dimensions,
            round,
            req.parent_proposal_id,
        );

        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO proposals (id, space_id, sender_id, proposal_type, dimensions, round, status, parent_proposal_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    proposal.id,
                    proposal.space_id,
                    proposal.sender_id,
                    proposal.proposal_type.as_str(),
                    serde_json::to_string(&proposal.dimensions)?,
                    proposal.round,
                    proposal.status.as_str(),
                    proposal.parent_proposal_id,
                    proposal.created_at,
                    proposal.updated_at,
                ],
            )?;
        }

        // Phase 13: 第一个提案时触发 OnFirstProposal
        if round == 1 {
            let mut space = self.get_space(space_id).await?.unwrap();
            let _ = self.check_and_apply_transitions(&mut space, crate::negotiation::rules::RuleTrigger::OnFirstProposal);
            self.spaces.insert(space.id.clone(), space.clone());
        }

        Ok(proposal)
    }

    /// 获取 Space 的所有提案
    pub async fn get_space_proposals(&self, space_id: &str) -> Result<Vec<Proposal>, GaggleError> {
        let db = self.db.lock().unwrap();

        let mut stmt = db.prepare(
            "SELECT id, space_id, sender_id, proposal_type, dimensions, round, status, parent_proposal_id, created_at, updated_at
             FROM proposals WHERE space_id = ?1 ORDER BY created_at ASC",
        )?;

        let proposals = stmt
            .query_map(params![space_id], |row| {
                let proposal_type_str: String = row.get(3)?;
                let dimensions_str: String = row.get(4)?;
                let status_str: String = row.get(6)?;

                Ok(Proposal {
                    id: row.get(0)?,
                    space_id: row.get(1)?,
                    sender_id: row.get(2)?,
                    proposal_type: match proposal_type_str.as_str() {
                        "counter" => ProposalType::Counter,
                        "best_and_final" => ProposalType::BestAndFinal,
                        _ => ProposalType::Initial,
                    },
                    dimensions: serde_json::from_str(&dimensions_str).unwrap_or_default(),
                    round: row.get(5)?,
                    status: match status_str.as_str() {
                        "accepted" => ProposalStatus::Accepted,
                        "rejected" => ProposalStatus::Rejected,
                        "superseded" => ProposalStatus::Superseded,
                        _ => ProposalStatus::Pending,
                    },
                    parent_proposal_id: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(proposals)
    }

    /// 回复提案（接受/拒绝/反提案）
    pub async fn respond_to_proposal(
        &self,
        agent: &Agent,
        space_id: &str,
        req: RespondToProposalRequest,
    ) -> Result<(Proposal, Option<Proposal>), GaggleError> {
        let space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        // 只有创建者可以回复提案
        if agent.id != space.creator_id {
            return Err(GaggleError::Forbidden(
                "Only space creator can respond to proposals".to_string(),
            ));
        }

        // State machine guard
        if space.status.is_terminal() {
            return Err(GaggleError::SpaceClosed(format!(
                "Cannot respond to proposal: space is {}",
                space.status.as_str()
            )));
        }

        // 获取原提案
        let mut original_proposal = {
            let db = self.db.lock().unwrap();
            let mut stmt = db.prepare(
                "SELECT id, space_id, sender_id, proposal_type, dimensions, round, status, parent_proposal_id, created_at, updated_at
                 FROM proposals WHERE id = ?1",
            )?;

            stmt.query_row(params![&req.proposal_id], |row| {
                let proposal_type_str: String = row.get(3)?;
                let dimensions_str: String = row.get(4)?;
                let status_str: String = row.get(6)?;

                Ok(Proposal {
                    id: row.get(0)?,
                    space_id: row.get(1)?,
                    sender_id: row.get(2)?,
                    proposal_type: match proposal_type_str.as_str() {
                        "counter" => ProposalType::Counter,
                        "best_and_final" => ProposalType::BestAndFinal,
                        _ => ProposalType::Initial,
                    },
                    dimensions: serde_json::from_str(&dimensions_str).unwrap_or_default(),
                    round: row.get(5)?,
                    status: match status_str.as_str() {
                        "accepted" => ProposalStatus::Accepted,
                        "rejected" => ProposalStatus::Rejected,
                        "superseded" => ProposalStatus::Superseded,
                        _ => ProposalStatus::Pending,
                    },
                    parent_proposal_id: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })
            .optional()?
            .ok_or_else(|| GaggleError::NotFound("Proposal not found".to_string()))?
        };

        // 检查提案状态
        if !original_proposal.is_pending() {
            return Err(GaggleError::ValidationError(
                "Proposal is not pending".to_string(),
            ));
        }

        let counter_proposal = match req.action {
            ProposalResponseAction::Accept => {
                original_proposal.accept().map_err(|e| GaggleError::ValidationError(e))?;
                // 拒绝同一轮次的其他待处理提案
                {
                    let db = self.db.lock().unwrap();
                    db.execute(
                        "UPDATE proposals SET status = 'superseded', updated_at = ?1
                         WHERE space_id = ?2 AND round = ?3 AND status = 'pending' AND id != ?4",
                        params![
                            Utc::now().timestamp_millis(),
                            space_id,
                            original_proposal.round,
                            original_proposal.id,
                        ],
                    )?;
                }
                None
            }
            ProposalResponseAction::Reject => {
                original_proposal.reject().map_err(|e| GaggleError::ValidationError(e))?;
                None
            }
            ProposalResponseAction::Counter => {
                original_proposal.supersede().map_err(|e| GaggleError::ValidationError(e))?;
                let counter_dimensions = req.counter_dimensions.unwrap_or_default();

                let counter = Proposal::new(
                    space_id.to_string(),
                    agent.id.clone(),
                    ProposalType::Counter,
                    counter_dimensions,
                    original_proposal.round + 1,
                    Some(original_proposal.id.clone()),
                );

                // 插入反提案
                {
                    let db = self.db.lock().unwrap();
                    db.execute(
                        "INSERT INTO proposals (id, space_id, sender_id, proposal_type, dimensions, round, status, parent_proposal_id, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            counter.id,
                            counter.space_id,
                            counter.sender_id,
                            counter.proposal_type.as_str(),
                            serde_json::to_string(&counter.dimensions)?,
                            counter.round,
                            counter.status.as_str(),
                            counter.parent_proposal_id,
                            counter.created_at,
                            counter.updated_at,
                        ],
                    )?;
                }

                Some(counter)
            }
        };

        // 更新原提案状态
        {
            let db = self.db.lock().unwrap();
            db.execute(
                "UPDATE proposals SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    original_proposal.status.as_str(),
                    original_proposal.updated_at,
                    original_proposal.id,
                ],
            )?;
        }

        Ok((original_proposal, counter_proposal))
    }

    /// 分享最优条款（匿名）
    pub async fn share_best_terms(
        &self,
        agent: &Agent,
        space_id: &str,
        req: ShareBestTermsRequest,
    ) -> Result<BestTermsShared, GaggleError> {
        let space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        // 只有创建者可以分享最优条款
        if agent.id != space.creator_id {
            return Err(GaggleError::Forbidden(
                "Only space creator can share best terms".to_string(),
            ));
        }

        let shared_at = Utc::now().timestamp_millis();

        Ok(BestTermsShared {
            space_id: space_id.to_string(),
            best_dimensions: req.best_dimensions,
            shared_at,
        })
    }

    /// 获取 Provider 在 Space 中的提案
    pub async fn get_provider_proposals(
        &self,
        space_id: &str,
        provider_id: &str,
    ) -> Result<Vec<Proposal>, GaggleError> {
        let db = self.db.lock().unwrap();

        let mut stmt = db.prepare(
            "SELECT id, space_id, sender_id, proposal_type, dimensions, round, status, parent_proposal_id, created_at, updated_at
             FROM proposals WHERE space_id = ?1 AND sender_id = ?2 ORDER BY created_at ASC",
        )?;

        let proposals = stmt
            .query_map(params![space_id, provider_id], |row| {
                let proposal_type_str: String = row.get(3)?;
                let dimensions_str: String = row.get(4)?;
                let status_str: String = row.get(6)?;

                Ok(Proposal {
                    id: row.get(0)?,
                    space_id: row.get(1)?,
                    sender_id: row.get(2)?,
                    proposal_type: match proposal_type_str.as_str() {
                        "counter" => ProposalType::Counter,
                        "best_and_final" => ProposalType::BestAndFinal,
                        _ => ProposalType::Initial,
                    },
                    dimensions: serde_json::from_str(&dimensions_str).unwrap_or_default(),
                    round: row.get(5)?,
                    status: match status_str.as_str() {
                        "accepted" => ProposalStatus::Accepted,
                        "rejected" => ProposalStatus::Rejected,
                        "superseded" => ProposalStatus::Superseded,
                        _ => ProposalStatus::Pending,
                    },
                    parent_proposal_id: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(proposals)
    }

    /// Agent 离开 Space
    pub async fn leave_space(
        &self,
        agent: &Agent,
        space_id: &str,
    ) -> Result<Space, GaggleError> {
        let mut space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if !space.is_member(&agent.id) {
            return Err(GaggleError::Forbidden(
                "Agent not member of this space".to_string(),
            ));
        }

        // 验证 agent 已经 joined
        if !space.joined_agent_ids.contains(&agent.id) {
            return Err(GaggleError::Forbidden(
                "Agent has not joined this space".to_string(),
            ));
        }

        // Creator 离开 → cancel space
        if agent.id == space.creator_id {
            let t = space.close(false, "creator_left", Some(&agent.id)).map_err(|e| GaggleError::ValidationError(e))?;
            tracing::info!(space_id = %space.id, from = ?t.from, to = ?t.to, "Space cancelled: creator left");
            let _ = self.record_transition(
                &space.id, t.from.as_str(), t.to.as_str(),
                &t.trigger, t.agent_id.as_deref(), space.version,
            );
        }
        // 规则驱动：非 creator 离开行为由 lock_condition 决定
        // 向后兼容：bilateral 默认 Never（可离开但不 cancel），旧逻辑等效
        else if space.status == SpaceStatus::Active {
            match space.rules.lock_condition {
                crate::negotiation::rules::LockCondition::Never => {
                    // 随时可离开，但不 cancel space（多方场景）
                    // 向后兼容 bilateral：bilateral 只有 2 人，离开等效 cancel
                    if space.agent_ids.len() <= 2 {
                        let t = space.close(false, "member_left_bilateral", Some(&agent.id)).map_err(|e| GaggleError::ValidationError(e))?;
                        tracing::info!(space_id = %space.id, from = ?t.from, to = ?t.to, "Space cancelled: bilateral member left");
                        let _ = self.record_transition(
                            &space.id, t.from.as_str(), t.to.as_str(),
                            &t.trigger, t.agent_id.as_deref(), space.version,
                        );
                    }
                    // 多方空间只移除成员
                }
                crate::negotiation::rules::LockCondition::OnFirstProposal => {
                    // 检查是否有提案
                    let has_proposals = {
                        let db = self.db.lock().unwrap();
                        let count: i64 = db
                            .query_row(
                                "SELECT COUNT(*) FROM proposals WHERE space_id = ?1",
                                params![space_id],
                                |row| row.get(0),
                            )
                            .unwrap_or(0);
                        count > 0
                    };
                    if has_proposals {
                        return Err(GaggleError::Forbidden(
                            "Cannot leave after proposals have been submitted".to_string(),
                        ));
                    }
                }
                crate::negotiation::rules::LockCondition::OnConclude => {
                    // 成交前可离开
                    if space.agent_ids.len() <= 2 {
                        let t = space.close(false, "member_left_pre_conclude", Some(&agent.id)).map_err(|e| GaggleError::ValidationError(e))?;
                        tracing::info!(space_id = %space.id, from = ?t.from, to = ?t.to, "Space cancelled: member left before conclusion");
                        let _ = self.record_transition(
                            &space.id, t.from.as_str(), t.to.as_str(),
                            &t.trigger, t.agent_id.as_deref(), space.version,
                        );
                    }
                }
                crate::negotiation::rules::LockCondition::Manual => {
                    return Err(GaggleError::Forbidden(
                        "Leaving requires mutual agreement in this space".to_string(),
                    ));
                }
            }
        }
        // RFP space + 单个 provider 离开 → 保持 Active（其他 providers 可继续）

        // 从 agent_ids 和 joined_agent_ids 中移除
        space.agent_ids.retain(|id| id != &agent.id);
        space.joined_agent_ids.retain(|id| id != &agent.id);
        space.bump_version();

        self.persist_space(&space)?;
        // persist_space doesn't update closed_at; patch it with version guard
        if space.closed_at.is_some() {
            let db = self.db.lock().unwrap();
            db.execute(
                "UPDATE spaces SET closed_at = ?1 WHERE id = ?2 AND version = ?3",
                params![space.closed_at, space_id, space.version],
            )?;
        }

        self.spaces.insert(space.id.clone(), space.clone());

        Ok(space)
    }

    /// 硬删除 Space（仅创建者可操作）
    /// 删除 space_messages、proposals、spaces 记录，清理内存缓存
    pub async fn hard_delete_space(
        &self,
        agent: &Agent,
        space_id: &str,
    ) -> Result<(), GaggleError> {
        let space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        // 验证调用者是 space 创建者
        if agent.id != space.creator_id {
            return Err(GaggleError::Forbidden(
                "Only space creator can delete the space".to_string(),
            ));
        }

        // 删除相关记录（事务保证一致性）
        {
            let db = self.db.lock().unwrap();
            db.execute("DELETE FROM space_messages WHERE space_id = ?1", params![space_id])?;
            db.execute("DELETE FROM proposals WHERE space_id = ?1", params![space_id])?;
            db.execute("DELETE FROM spaces WHERE id = ?1", params![space_id])?;
        }

        // 清理内存缓存
        {
            self.spaces.remove(space_id);
        }

        // 清理 broadcast channel
        {
            let mut broadcast_txs = self.broadcast_txs.write().await;
            broadcast_txs.remove(space_id);
        }

        Ok(())
    }

    // ==================== 加权评估 & 轮次管理 ====================

    /// 加权评估 RFP Space 中的所有 pending 提案
    pub async fn evaluate_proposals(
        &self,
        space_id: &str,
        weights: &EvaluationWeights,
    ) -> Result<EvaluateResponse, GaggleError> {
        let proposals = self.get_space_proposals(space_id).await?;

        // 只评估 pending 的提案
        let pending: Vec<&Proposal> = proposals.iter().filter(|p| p.is_pending()).collect();

        if pending.is_empty() {
            return Ok(EvaluateResponse {
                scores: Vec::new(),
                sorted_by: "weighted_score".to_string(),
            });
        }

        // 收集各维度值用于归一化
        let prices: Vec<f64> = pending
            .iter()
            .filter_map(|p| p.dimensions.price)
            .collect();
        let timelines: Vec<f64> = pending
            .iter()
            .filter_map(|p| p.dimensions.timeline_days)
            .collect();

        let min_price = prices.iter().copied().fold(f64::MAX, f64::min);
        let max_price = prices.iter().copied().fold(f64::MIN, f64::max);
        let min_timeline = timelines.iter().copied().fold(f64::MAX, f64::min);
        let max_timeline = timelines.iter().copied().fold(f64::MIN, f64::max);

        let mut scores: Vec<ProposalScore> = pending
            .iter()
            .map(|p| {
                // 价格评分：最低价得 1.0，越高分越低
                let price_score = p.dimensions.price.map_or(0.5, |price| {
                    if (max_price - min_price).abs() < f64::EPSILON {
                        1.0
                    } else {
                        1.0 - (price - min_price) / (max_price - min_price)
                    }
                });

                // 周期评分：最短天数得 1.0，越长分越低
                let timeline_score = p.dimensions.timeline_days.map_or(0.5, |days| {
                    if (max_timeline - min_timeline).abs() < f64::EPSILON {
                        1.0
                    } else {
                        1.0 - (days - min_timeline) / (max_timeline - min_timeline)
                    }
                });

                // 质量评分：按 quality_tier 映射
                let quality_score = p
                    .dimensions
                    .quality_tier
                    .as_deref()
                    .map_or(0.5, quality_tier_score);

                let dimension_scores = DimensionScores {
                    price_score,
                    timeline_score,
                    quality_score,
                };

                let weighted_score = weights.price * price_score
                    + weights.timeline * timeline_score
                    + weights.quality * quality_score;

                ProposalScore {
                    proposal_id: p.id.clone(),
                    provider_id: p.sender_id.clone(),
                    weighted_score,
                    dimension_scores,
                }
            })
            .collect();

        // 按加权分降序排列
        scores.sort_by(|a, b| {
            b.weighted_score
                .partial_cmp(&a.weighted_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(EvaluateResponse {
            scores,
            sorted_by: "weighted_score".to_string(),
        })
    }

    /// 获取当前轮次信息
    pub async fn get_round_info(&self, space_id: &str) -> Result<RoundInfo, GaggleError> {
        let space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        let rfp_ctx = space.rfp_context.as_ref();

        // 从 rfp_context 读取轮次信息，默认从 proposals 推断
        let current_round = rfp_ctx
            .and_then(|v| v.get("current_round"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        let allowed_rounds = rfp_ctx
            .and_then(|v| v.get("allowed_rounds"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let round_status_str = rfp_ctx
            .and_then(|v| v.get("round_status"))
            .and_then(|v| v.as_str())
            .unwrap_or("open");

        let round_status = match round_status_str {
            "closed" => RoundStatus::Closed,
            "expired" => RoundStatus::Expired,
            _ => RoundStatus::Open,
        };

        let round_deadline = rfp_ctx
            .and_then(|v| v.get("round_deadline"))
            .and_then(|v| v.as_i64());

        Ok(RoundInfo {
            current_round,
            allowed_rounds,
            round_status,
            round_deadline,
        })
    }

    /// 推进到下一轮（仅 RFP Space 创建者可操作）
    pub async fn advance_round(
        &self,
        agent: &Agent,
        space_id: &str,
    ) -> Result<RoundInfo, GaggleError> {
        let mut space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if !space.rules.has_rounds() {
            return Err(GaggleError::ValidationError(
                "Round management is only available for spaces with round configuration".to_string(),
            ));
        }

        if agent.id != space.creator_id {
            return Err(GaggleError::Forbidden(
                "Only space creator can advance rounds".to_string(),
            ));
        }

        // 获取当前轮次
        let mut rfp_ctx = space.rfp_context.clone().unwrap_or_default();
        let current_round = rfp_ctx
            .get("current_round")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as u32;

        let allowed_rounds = rfp_ctx
            .get("allowed_rounds")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        // 检查是否已达到最大轮次
        if let Some(max) = allowed_rounds {
            if current_round >= max {
                return Err(GaggleError::ValidationError(
                    format!("Already at maximum round ({}/{})", current_round, max),
                ));
            }
        }

        // 将当前轮次的 pending 提案标记为 superseded
        {
            let db = self.db.lock().unwrap();
            db.execute(
                "UPDATE proposals SET status = 'superseded', updated_at = ?1
                 WHERE space_id = ?2 AND round = ?3 AND status = 'pending'",
                params![Utc::now().timestamp_millis(), space_id, current_round],
            )?;
        }

        // 推进轮次
        let new_round = current_round + 1;
        rfp_ctx["current_round"] = serde_json::json!(new_round);
        rfp_ctx["round_status"] = serde_json::json!("open");

        // 清除旧 deadline
        if let Some(obj) = rfp_ctx.as_object_mut() {
            obj.remove("round_deadline");
        }

        space.rfp_context = Some(rfp_ctx);
        space.updated_at = Utc::now().timestamp_millis();
        let expected_ver = space.version;
        space.bump_version();

        {
            let db = self.db.lock().unwrap();
            let rows = db.execute(
                "UPDATE spaces SET rfp_context = ?1, updated_at = ?2, version = ?3 WHERE id = ?4 AND version = ?5",
                params![
                    serde_json::to_string(&space.rfp_context)?,
                    space.updated_at,
                    space.version,
                    space_id,
                    expected_ver,
                ],
            )?;
            if rows == 0 {
                return Err(GaggleError::Conflict(format!(
                    "Space {} version conflict during RFP evaluation", space_id
                )));
            }
        }

        // Phase 13: 检查 OnRoundAdvance transition
        let _ = self.check_and_apply_transitions(&mut space, crate::negotiation::rules::RuleTrigger::OnRoundAdvance { round: new_round });

        // 更新内存缓存
        {
            self.spaces.insert(space.id.clone(), space.clone());
        }

        Ok(RoundInfo {
            current_round: new_round,
            allowed_rounds,
            round_status: RoundStatus::Open,
            round_deadline: None,
        })
    }

    // ── Phase 9: SubSpace ──────────────────────────────────

    /// 初始化 sub_spaces 表（在 new() 中调用）
    fn init_subspace_table(conn: &Connection) -> Result<(), GaggleError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sub_spaces (
                id TEXT PRIMARY KEY,
                parent_space_id TEXT NOT NULL,
                name TEXT NOT NULL,
                creator_id TEXT NOT NULL,
                agent_ids TEXT NOT NULL,
                rules TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                closed_at INTEGER,
                FOREIGN KEY (parent_space_id) REFERENCES spaces(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subspaces_parent ON sub_spaces(parent_space_id)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subspaces_creator ON sub_spaces(creator_id)",
            [],
        )?;

        // 子空间消息表（复用 space_messages 结构，但用 sub_space_id 标识）
        conn.execute(
            "CREATE TABLE IF NOT EXISTS subspace_messages (
                id TEXT PRIMARY KEY,
                sub_space_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                msg_type TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                round INTEGER NOT NULL,
                metadata TEXT,
                visibility TEXT NOT NULL DEFAULT 'broadcast',
                recipient_ids TEXT DEFAULT '[]',
                FOREIGN KEY (sub_space_id) REFERENCES sub_spaces(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subspace_messages ON subspace_messages(sub_space_id)",
            [],
        )?;

        // 子空间提案表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS subspace_proposals (
                id TEXT PRIMARY KEY,
                sub_space_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                proposal_type TEXT NOT NULL,
                dimensions TEXT NOT NULL,
                round INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                parent_proposal_id TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (sub_space_id) REFERENCES sub_spaces(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_subspace_proposals ON subspace_proposals(sub_space_id)",
            [],
        )?;

        Ok(())
    }

    /// 创建子空间
    pub async fn create_subspace(
        &self,
        parent_space_id: &str,
        creator_id: &str,
        req: crate::negotiation::subspace::CreateSubSpaceRequest,
    ) -> Result<crate::negotiation::SubSpace, GaggleError> {
        // 验证父空间存在
        let parent = self
            .get_space(parent_space_id)
            .await?
            .ok_or_else(|| GaggleError::NotFound(format!("Parent space not found: {}", parent_space_id)))?;

        // 验证创建者是父空间成员
        if !parent.is_member(creator_id) {
            return Err(GaggleError::Forbidden(
                "Creator must be a member of the parent space".to_string(),
            ));
        }

        // 验证所有成员是父空间成员的子集
        for aid in &req.agent_ids {
            if !parent.is_member(aid) {
                return Err(GaggleError::ValidationError(
                    format!("Agent {} is not a member of parent space", aid),
                ));
            }
        }

        let sub = crate::negotiation::SubSpace::new(
            parent_space_id.to_string(),
            creator_id.to_string(),
            req,
        );

        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO sub_spaces (id, parent_space_id, name, creator_id, agent_ids, rules, status, created_at, updated_at, closed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    sub.id,
                    sub.parent_space_id,
                    sub.name,
                    sub.creator_id,
                    serde_json::to_string(&sub.agent_ids)?,
                    serde_json::to_string(&sub.rules)?,
                    serde_json::to_string(&sub.status)?,
                    sub.created_at,
                    sub.updated_at,
                    sub.closed_at,
                ],
            )?;
        }

        // 为子空间创建独立的 broadcast channel
        let (tx, _) = broadcast::channel::<String>(256);
        {
            let mut broadcast_txs = self.broadcast_txs.write().await;
            broadcast_txs.insert(format!("sub:{}", sub.id), tx);
        }

        Ok(sub)
    }

    /// 获取子空间
    pub async fn get_subspace(&self, sub_space_id: &str) -> Result<Option<crate::negotiation::SubSpace>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, parent_space_id, name, creator_id, agent_ids, rules, status, created_at, updated_at, closed_at
             FROM sub_spaces WHERE id = ?1"
        )?;

        let result = stmt.query_row(params![sub_space_id], |row| {
            let agent_ids_str: String = row.get(4)?;
            let rules_str: String = row.get(5)?;
            let status_str: String = row.get(6)?;

            Ok(crate::negotiation::SubSpace {
                id: row.get(0)?,
                parent_space_id: row.get(1)?,
                name: row.get(2)?,
                creator_id: row.get(3)?,
                agent_ids: serde_json::from_str(&agent_ids_str).unwrap_or_default(),
                rules: serde_json::from_str(&rules_str).unwrap_or_default(),
                status: serde_json::from_str(&status_str).unwrap_or(SpaceStatus::Active),
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                closed_at: row.get(9)?,
            })
        }).optional()?;

        Ok(result)
    }

    /// 列出父空间的所有子空间
    pub async fn list_subspaces(&self, parent_space_id: &str) -> Result<Vec<crate::negotiation::SubSpace>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, parent_space_id, name, creator_id, agent_ids, rules, status, created_at, updated_at, closed_at
             FROM sub_spaces WHERE parent_space_id = ?1 ORDER BY created_at"
        )?;

        let subs = stmt.query_map(params![parent_space_id], |row| {
            let agent_ids_str: String = row.get(4)?;
            let rules_str: String = row.get(5)?;
            let status_str: String = row.get(6)?;

            Ok(crate::negotiation::SubSpace {
                id: row.get(0)?,
                parent_space_id: row.get(1)?,
                name: row.get(2)?,
                creator_id: row.get(3)?,
                agent_ids: serde_json::from_str(&agent_ids_str).unwrap_or_default(),
                rules: serde_json::from_str(&rules_str).unwrap_or_default(),
                status: serde_json::from_str(&status_str).unwrap_or(SpaceStatus::Active),
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                closed_at: row.get(9)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(subs)
    }

    /// 持久化子空间更新
    pub(crate) fn persist_subspace(&self, sub: &crate::negotiation::SubSpace) -> Result<(), GaggleError> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE sub_spaces SET agent_ids = ?1, status = ?2, updated_at = ?3, rules = ?4, closed_at = ?5 WHERE id = ?6",
            params![
                serde_json::to_string(&sub.agent_ids)?,
                serde_json::to_string(&sub.status)?,
                sub.updated_at,
                serde_json::to_string(&sub.rules)?,
                sub.closed_at,
                sub.id,
            ],
        )?;
        Ok(())
    }

    /// 子空间发送消息
    pub async fn send_subspace_message(
        &self,
        sub_space_id: &str,
        sender_id: &str,
        msg_type: crate::negotiation::MessageType,
        content: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<SpaceMessage, GaggleError> {
        let sub = self
            .get_subspace(sub_space_id)
            .await?
            .ok_or_else(|| GaggleError::NotFound(format!("Sub-space not found: {}", sub_space_id)))?;

        if sub.status.is_terminal() {
            return Err(GaggleError::ValidationError(format!("Sub-space is in terminal state: {}", sub.status.as_str())));
        }
        if !sub.is_member(sender_id) {
            return Err(GaggleError::Forbidden("Not a member of this sub-space".to_string()));
        }

        let msg = SpaceMessage::new(
            sub_space_id.to_string(),
            sender_id.to_string(),
            msg_type,
            content.to_string(),
            1, // round
            metadata,
        );

        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO subspace_messages (id, sub_space_id, sender_id, msg_type, content, timestamp, round, metadata, visibility, recipient_ids)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    msg.id,
                    msg.space_id,
                    msg.sender_id,
                    msg.msg_type.as_str(),
                    msg.content,
                    msg.timestamp,
                    msg.round,
                    msg.metadata.as_ref().and_then(|v| serde_json::to_string(v).ok()),
                    msg.visibility.as_str(),
                    serde_json::to_string(&msg.recipient_ids)?,
                ],
            )?;
        }

        Ok(msg)
    }

    /// 获取子空间消息
    pub async fn get_subspace_messages(
        &self,
        sub_space_id: &str,
    ) -> Result<Vec<SpaceMessage>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, sub_space_id, sender_id, msg_type, content, timestamp, round, metadata, visibility, recipient_ids
             FROM subspace_messages WHERE sub_space_id = ?1 ORDER BY timestamp"
        )?;

        let messages = stmt.query_map(params![sub_space_id], |row| {
            let meta_str: Option<String> = row.get(7)?;
            let vis_str: String = row.get(8)?;
            let recip_str: String = row.get(9)?;

            Ok(SpaceMessage {
                id: row.get(0)?,
                space_id: row.get(1)?,
                sender_id: row.get(2)?,
                msg_type: crate::negotiation::MessageType::from_str_safe(row.get::<_, String>(3)?.as_str()),
                content: row.get(4)?,
                timestamp: row.get(5)?,
                round: row.get(6)?,
                metadata: meta_str.and_then(|s| serde_json::from_str(&s).ok()),
                visibility: match vis_str.as_str() {
                    "directed" => MessageVisibility::Directed,
                    "private" => MessageVisibility::Private,
                    _ => MessageVisibility::Broadcast,
                },
                recipient_ids: serde_json::from_str(&recip_str).unwrap_or_default(),
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(messages)
    }

    /// 子空间提交提案
    pub async fn submit_subspace_proposal(
        &self,
        sub_space_id: &str,
        sender_id: &str,
        proposal_type: ProposalType,
        dimensions: crate::negotiation::ProposalDimensions,
    ) -> Result<Proposal, GaggleError> {
        let sub = self
            .get_subspace(sub_space_id)
            .await?
            .ok_or_else(|| GaggleError::NotFound(format!("Sub-space not found: {}", sub_space_id)))?;

        if sub.status.is_terminal() {
            return Err(GaggleError::ValidationError(format!("Sub-space is in terminal state: {}", sub.status.as_str())));
        }
        if !sub.is_member(sender_id) {
            return Err(GaggleError::Forbidden("Not a member of this sub-space".to_string()));
        }

        let now = Utc::now().timestamp_millis();
        let proposal = Proposal {
            id: Uuid::new_v4().to_string(),
            space_id: sub_space_id.to_string(),
            sender_id: sender_id.to_string(),
            proposal_type,
            dimensions,
            round: 1,
            status: ProposalStatus::Pending,
            parent_proposal_id: None,
            created_at: now,
            updated_at: now,
        };

        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO subspace_proposals (id, sub_space_id, sender_id, proposal_type, dimensions, round, status, parent_proposal_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    proposal.id,
                    proposal.space_id,
                    proposal.sender_id,
                    proposal.proposal_type.as_str(),
                    serde_json::to_string(&proposal.dimensions)?,
                    proposal.round,
                    proposal.status.as_str(),
                    proposal.parent_proposal_id,
                    proposal.created_at,
                    proposal.updated_at,
                ],
            )?;
        }

        Ok(proposal)
    }

    /// 获取子空间提案列表
    pub async fn get_subspace_proposals(
        &self,
        sub_space_id: &str,
    ) -> Result<Vec<Proposal>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, sub_space_id, sender_id, proposal_type, dimensions, round, status, parent_proposal_id, created_at, updated_at
             FROM subspace_proposals WHERE sub_space_id = ?1 ORDER BY created_at"
        )?;

        let proposals = stmt.query_map(params![sub_space_id], |row| {
            let dims_str: String = row.get(4)?;
            let status_str: String = row.get(6)?;

            Ok(Proposal {
                id: row.get(0)?,
                space_id: row.get(1)?,
                sender_id: row.get(2)?,
                proposal_type: ProposalType::from_str_safe(row.get::<_, String>(3)?.as_str()),
                dimensions: serde_json::from_str(&dims_str).unwrap_or_default(),
                round: row.get(5)?,
                status: ProposalStatus::from_str_safe(&status_str),
                parent_proposal_id: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;

        Ok(proposals)
    }

    /// 关闭子空间
    pub async fn close_subspace(
        &self,
        sub_space_id: &str,
        closer_id: &str,
        concluded: bool,
    ) -> Result<crate::negotiation::SubSpace, GaggleError> {
        let mut sub = self
            .get_subspace(sub_space_id)
            .await?
            .ok_or_else(|| GaggleError::NotFound(format!("Sub-space not found: {}", sub_space_id)))?;

        if sub.creator_id != closer_id {
            return Err(GaggleError::Forbidden("Only the creator can close a sub-space".to_string()));
        }

        sub.close(concluded, "subspace_close", Some(closer_id)).map_err(|e| GaggleError::ValidationError(e))?;
        self.persist_subspace(&sub)?;
        Ok(sub)
    }

    /// 获取子空间的 broadcast channel
    pub async fn get_subspace_broadcast_tx(&self, sub_space_id: &str) -> Option<broadcast::Sender<String>> {
        let txs = self.broadcast_txs.read().await;
        txs.get(&format!("sub:{}", sub_space_id)).cloned()
    }

    // ── Phase 10: Coalition ────────────────────────────────

    /// 初始化 coalitions 表
    fn init_coalition_table(conn: &Connection) -> Result<(), GaggleError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS coalitions (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL,
                name TEXT NOT NULL,
                leader_id TEXT NOT NULL,
                member_ids TEXT NOT NULL,
                stance TEXT,
                internal_space_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (space_id) REFERENCES spaces(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_coalitions_space ON coalitions(space_id)",
            [],
        )?;

        Ok(())
    }

    fn map_coalition(row: &rusqlite::Row) -> rusqlite::Result<crate::negotiation::Coalition> {
        let member_ids_str: String = row.get(4)?;
        let stance_str: Option<String> = row.get(5)?;
        let status_str: String = row.get(7)?;

        Ok(crate::negotiation::Coalition {
            id: row.get(0)?,
            space_id: row.get(1)?,
            name: row.get(2)?,
            leader_id: row.get(3)?,
            member_ids: serde_json::from_str(&member_ids_str).unwrap_or_default(),
            stance: stance_str.and_then(|s| serde_json::from_str(&s).ok()),
            internal_space_id: row.get(6)?,
            status: crate::negotiation::CoalitionStatus::from_str_safe(&status_str),
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    }

    /// 创建联盟（自动创建内部子空间）
    pub async fn create_coalition(
        &self,
        space_id: &str,
        leader_id: &str,
        req: crate::negotiation::coalition::CreateCoalitionRequest,
    ) -> Result<crate::negotiation::Coalition, GaggleError> {
        // 验证父空间存在
        let space = self.get_space(space_id).await?
            .ok_or_else(|| GaggleError::NotFound(format!("Space not found: {}", space_id)))?;

        // 验证 leader 是空间成员
        if !space.is_member(leader_id) {
            return Err(GaggleError::Forbidden("Leader must be a member of the space".to_string()));
        }

        // 验证所有初始成员是空间成员
        for mid in &req.member_ids {
            if !space.is_member(mid) {
                return Err(GaggleError::ValidationError(
                    format!("Agent {} is not a member of the space", mid),
                ));
            }
        }

        // 自动创建内部子空间（用于联盟内部协调）
        let internal_sub = self.create_subspace(
            space_id,
            leader_id,
            crate::negotiation::subspace::CreateSubSpaceRequest {
                name: format!("{} - Internal", req.name),
                agent_ids: req.member_ids.clone(),
                rules: None,
            },
        ).await?;

        let coalition = crate::negotiation::Coalition::new(
            space_id.to_string(),
            leader_id.to_string(),
            req,
            internal_sub.id,
        );

        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO coalitions (id, space_id, name, leader_id, member_ids, stance, internal_space_id, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    coalition.id,
                    coalition.space_id,
                    coalition.name,
                    coalition.leader_id,
                    serde_json::to_string(&coalition.member_ids)?,
                    coalition.stance.as_ref().and_then(|v| serde_json::to_string(v).ok()),
                    coalition.internal_space_id,
                    coalition.status.as_str(),
                    coalition.created_at,
                    coalition.updated_at,
                ],
            )?;
        }

        Ok(coalition)
    }

    /// 获取联盟
    pub async fn get_coalition(&self, coalition_id: &str) -> Result<Option<crate::negotiation::Coalition>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, space_id, name, leader_id, member_ids, stance, internal_space_id, status, created_at, updated_at
             FROM coalitions WHERE id = ?1"
        )?;

        let result = stmt.query_row(params![coalition_id], Self::map_coalition).optional()?;
        Ok(result)
    }

    /// 列出空间的所有联盟
    pub async fn list_coalitions(&self, space_id: &str) -> Result<Vec<crate::negotiation::Coalition>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, space_id, name, leader_id, member_ids, stance, internal_space_id, status, created_at, updated_at
             FROM coalitions WHERE space_id = ?1 AND status = 'active' ORDER BY created_at"
        )?;

        let coalitions = stmt.query_map(params![space_id], Self::map_coalition)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(coalitions)
    }

    /// 加入联盟
    pub async fn join_coalition(
        &self,
        coalition_id: &str,
        agent_id: &str,
    ) -> Result<crate::negotiation::Coalition, GaggleError> {
        let mut coalition = self.get_coalition(coalition_id).await?
            .ok_or_else(|| GaggleError::NotFound(format!("Coalition not found: {}", coalition_id)))?;

        if coalition.status != crate::negotiation::CoalitionStatus::Active {
            return Err(GaggleError::ValidationError("Coalition is not active".to_string()));
        }

        // 验证 agent 是空间成员
        let space = self.get_space(&coalition.space_id).await?
            .ok_or_else(|| GaggleError::NotFound("Space not found".to_string()))?;
        if !space.is_member(agent_id) {
            return Err(GaggleError::Forbidden("Must be a member of the parent space".to_string()));
        }

        coalition.add_member(agent_id);

        // 同步到内部子空间
        let mut sub = self.get_subspace(&coalition.internal_space_id).await?
            .ok_or_else(|| GaggleError::NotFound("Internal sub-space not found".to_string()))?;
        if !sub.agent_ids.contains(&agent_id.to_string()) {
            sub.agent_ids.push(agent_id.to_string());
            sub.updated_at = Utc::now().timestamp_millis();
            self.persist_subspace(&sub)?;
        }

        self.persist_coalition(&coalition)?;
        Ok(coalition)
    }

    /// 离开联盟
    pub async fn leave_coalition(
        &self,
        coalition_id: &str,
        agent_id: &str,
    ) -> Result<crate::negotiation::Coalition, GaggleError> {
        let mut coalition = self.get_coalition(coalition_id).await?
            .ok_or_else(|| GaggleError::NotFound(format!("Coalition not found: {}", coalition_id)))?;

        if coalition.is_leader(agent_id) {
            return Err(GaggleError::ValidationError("Leader cannot leave; disband the coalition instead".to_string()));
        }

        if !coalition.remove_member(agent_id) {
            return Err(GaggleError::ValidationError("Not a member of this coalition".to_string()));
        }

        self.persist_coalition(&coalition)?;
        Ok(coalition)
    }

    /// 更新联盟立场
    pub async fn update_coalition_stance(
        &self,
        coalition_id: &str,
        leader_id: &str,
        stance: serde_json::Value,
    ) -> Result<crate::negotiation::Coalition, GaggleError> {
        let mut coalition = self.get_coalition(coalition_id).await?
            .ok_or_else(|| GaggleError::NotFound(format!("Coalition not found: {}", coalition_id)))?;

        if !coalition.is_leader(leader_id) {
            return Err(GaggleError::Forbidden("Only the leader can update stance".to_string()));
        }

        coalition.stance = Some(stance);
        coalition.updated_at = Utc::now().timestamp_millis();
        self.persist_coalition(&coalition)?;
        Ok(coalition)
    }

    /// 解散联盟
    pub async fn disband_coalition(
        &self,
        coalition_id: &str,
        leader_id: &str,
    ) -> Result<crate::negotiation::Coalition, GaggleError> {
        let mut coalition = self.get_coalition(coalition_id).await?
            .ok_or_else(|| GaggleError::NotFound(format!("Coalition not found: {}", coalition_id)))?;

        if !coalition.is_leader(leader_id) {
            return Err(GaggleError::Forbidden("Only the leader can disband".to_string()));
        }

        coalition.disband();
        self.persist_coalition(&coalition)?;

        // 关闭内部子空间
        let _ = self.close_subspace(&coalition.internal_space_id, leader_id, false).await;

        Ok(coalition)
    }

    /// 持久化联盟
    fn persist_coalition(&self, coalition: &crate::negotiation::Coalition) -> Result<(), GaggleError> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE coalitions SET member_ids = ?1, stance = ?2, status = ?3, updated_at = ?4 WHERE id = ?5",
            params![
                serde_json::to_string(&coalition.member_ids)?,
                coalition.stance.as_ref().and_then(|v| serde_json::to_string(v).ok()),
                coalition.status.as_str(),
                coalition.updated_at,
                coalition.id,
            ],
        )?;
        Ok(())
    }

    // ── Phase 11: Delegation ────────────────────────────────

    fn init_delegation_table(conn: &Connection) -> Result<(), GaggleError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS delegations (
                id TEXT PRIMARY KEY,
                delegator_id TEXT NOT NULL,
                delegate_id TEXT NOT NULL,
                space_id TEXT NOT NULL,
                scope TEXT NOT NULL,
                expires_at INTEGER,
                status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL,
                FOREIGN KEY (space_id) REFERENCES spaces(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_delegations_space ON delegations(space_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_delegations_delegator ON delegations(delegator_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_delegations_delegate ON delegations(delegate_id)",
            [],
        )?;

        Ok(())
    }

    fn map_delegation(row: &rusqlite::Row) -> rusqlite::Result<crate::negotiation::Delegation> {
        let scope_str: String = row.get(4)?;
        let status_str: String = row.get(6)?;

        Ok(crate::negotiation::Delegation {
            id: row.get(0)?,
            delegator_id: row.get(1)?,
            delegate_id: row.get(2)?,
            space_id: row.get(3)?,
            scope: crate::negotiation::DelegationScope::from_str_safe(&scope_str),
            expires_at: row.get(5)?,
            status: crate::negotiation::DelegationStatus::from_str_safe(&status_str),
            created_at: row.get(7)?,
        })
    }

    /// 创建委托
    pub async fn create_delegation(
        &self,
        delegator_id: &str,
        req: crate::negotiation::delegation::CreateDelegationRequest,
    ) -> Result<crate::negotiation::Delegation, GaggleError> {
        // 验证 space 存在
        let space = self.get_space(&req.space_id).await?
            .ok_or_else(|| GaggleError::NotFound(format!("Space not found: {}", req.space_id)))?;

        // 验证委托人是空间成员
        if !space.is_member(delegator_id) {
            return Err(GaggleError::Forbidden("Delegator must be a space member".to_string()));
        }
        // 验证代理人是空间成员
        if !space.is_member(&req.delegate_id) {
            return Err(GaggleError::Forbidden("Delegate must be a space member".to_string()));
        }
        // 不能委托给自己
        if delegator_id == req.delegate_id {
            return Err(GaggleError::ValidationError("Cannot delegate to yourself".to_string()));
        }

        let delegation = crate::negotiation::Delegation::new(
            delegator_id.to_string(),
            req,
        );

        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO delegations (id, delegator_id, delegate_id, space_id, scope, expires_at, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    delegation.id,
                    delegation.delegator_id,
                    delegation.delegate_id,
                    delegation.space_id,
                    delegation.scope.as_str(),
                    delegation.expires_at,
                    delegation.status.as_str(),
                    delegation.created_at,
                ],
            )?;
        }

        Ok(delegation)
    }

    /// 获取委托
    pub async fn get_delegation(&self, delegation_id: &str) -> Result<Option<crate::negotiation::Delegation>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, delegator_id, delegate_id, space_id, scope, expires_at, status, created_at
             FROM delegations WHERE id = ?1"
        )?;
        let result = stmt.query_row(params![delegation_id], Self::map_delegation).optional()?;
        Ok(result)
    }

    /// 列出 space 的活跃委托
    pub async fn list_delegations(&self, space_id: &str) -> Result<Vec<crate::negotiation::Delegation>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, delegator_id, delegate_id, space_id, scope, expires_at, status, created_at
             FROM delegations WHERE space_id = ?1 AND status = 'active' ORDER BY created_at"
        )?;
        let delegations = stmt.query_map(params![space_id], Self::map_delegation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(delegations)
    }

    /// 列出 agent 作为委托人的所有委托
    pub async fn list_delegations_by_delegator(&self, delegator_id: &str) -> Result<Vec<crate::negotiation::Delegation>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, delegator_id, delegate_id, space_id, scope, expires_at, status, created_at
             FROM delegations WHERE delegator_id = ?1 ORDER BY created_at"
        )?;
        let delegations = stmt.query_map(params![delegator_id], Self::map_delegation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(delegations)
    }

    /// 列出 agent 作为代理人的所有活跃委托
    pub async fn list_delegations_by_delegate(&self, delegate_id: &str) -> Result<Vec<crate::negotiation::Delegation>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, delegator_id, delegate_id, space_id, scope, expires_at, status, created_at
             FROM delegations WHERE delegate_id = ?1 AND status = 'active' ORDER BY created_at"
        )?;
        let delegations = stmt.query_map(params![delegate_id], Self::map_delegation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(delegations)
    }

    /// 撤销委托
    pub async fn revoke_delegation(
        &self,
        delegation_id: &str,
        revoker_id: &str,
    ) -> Result<crate::negotiation::Delegation, GaggleError> {
        let mut delegation = self.get_delegation(delegation_id).await?
            .ok_or_else(|| GaggleError::NotFound(format!("Delegation not found: {}", delegation_id)))?;

        // 只有委托人可以撤销
        if delegation.delegator_id != revoker_id {
            return Err(GaggleError::Forbidden("Only the delegator can revoke".to_string()));
        }

        delegation.revoke();

        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE delegations SET status = 'revoked' WHERE id = ?1",
            params![delegation.id],
        )?;

        Ok(delegation)
    }

    // ── Phase 12: Recruitment ───────────────────────────────

    fn init_recruitment_table(conn: &Connection) -> Result<(), GaggleError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS recruitment_requests (
                id TEXT PRIMARY KEY,
                space_id TEXT NOT NULL,
                recruiter_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                role TEXT NOT NULL,
                pitch TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'pending',
                created_at INTEGER NOT NULL,
                FOREIGN KEY (space_id) REFERENCES spaces(id)
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_recruitment_space ON recruitment_requests(space_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_recruitment_target ON recruitment_requests(target_id)",
            [],
        )?;

        Ok(())
    }

    fn map_recruitment(row: &rusqlite::Row) -> rusqlite::Result<crate::negotiation::RecruitmentRequest> {
        let status_str: String = row.get(6)?;
        Ok(crate::negotiation::RecruitmentRequest {
            id: row.get(0)?,
            space_id: row.get(1)?,
            recruiter_id: row.get(2)?,
            target_id: row.get(3)?,
            role: row.get(4)?,
            pitch: row.get(5)?,
            status: crate::negotiation::RecruitmentStatus::from_str_safe(&status_str),
            created_at: row.get(7)?,
        })
    }

    /// 创建招募请求
    pub async fn create_recruitment(
        &self,
        space_id: &str,
        recruiter_id: &str,
        req: crate::negotiation::recruitment::CreateRecruitmentRequest,
    ) -> Result<crate::negotiation::RecruitmentRequest, GaggleError> {
        let space = self.get_space(space_id).await?
            .ok_or_else(|| GaggleError::NotFound(format!("Space not found: {}", space_id)))?;

        // 验证 recruiter 是成员
        if !space.is_member(recruiter_id) {
            return Err(GaggleError::Forbidden("Recruiter must be a space member".to_string()));
        }

        // 验证 recruiter 的角色有 can_invite 权限
        let role = space.get_role(recruiter_id).unwrap_or("participant");
        if !space.rules.roles.get(role).map(|rc| rc.can_invite).unwrap_or(false) {
            return Err(GaggleError::Forbidden(
                format!("Role '{}' does not have invite permission", role),
            ));
        }

        // 验证 max_participants
        if let Some(max) = space.rules.max_participants {
            if space.joined_agent_ids.len() >= max {
                return Err(GaggleError::ValidationError(
                    format!("Space has reached max participants ({}/{})", space.joined_agent_ids.len(), max),
                ));
            }
        }

        // target 不能已经是成员
        if space.is_member(&req.target_id) {
            return Err(GaggleError::ValidationError("Target is already a member".to_string()));
        }

        let recruitment = crate::negotiation::RecruitmentRequest::new(
            space_id.to_string(),
            recruiter_id.to_string(),
            req,
        );

        {
            let db = self.db.lock().unwrap();
            db.execute(
                "INSERT INTO recruitment_requests (id, space_id, recruiter_id, target_id, role, pitch, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    recruitment.id,
                    recruitment.space_id,
                    recruitment.recruiter_id,
                    recruitment.target_id,
                    recruitment.role,
                    recruitment.pitch,
                    recruitment.status.as_str(),
                    recruitment.created_at,
                ],
            )?;
        }

        Ok(recruitment)
    }

    /// 获取招募请求
    pub async fn get_recruitment(&self, recruitment_id: &str) -> Result<Option<crate::negotiation::RecruitmentRequest>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, space_id, recruiter_id, target_id, role, pitch, status, created_at
             FROM recruitment_requests WHERE id = ?1"
        )?;
        let result = stmt.query_row(params![recruitment_id], Self::map_recruitment).optional()?;
        Ok(result)
    }

    /// 列出空间的招募请求
    pub async fn list_recruitments(&self, space_id: &str) -> Result<Vec<crate::negotiation::RecruitmentRequest>, GaggleError> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, space_id, recruiter_id, target_id, role, pitch, status, created_at
             FROM recruitment_requests WHERE space_id = ?1 ORDER BY created_at"
        )?;
        let list = stmt.query_map(params![space_id], Self::map_recruitment)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(list)
    }

    /// 接受招募 — 第二步：更新 DB + 加入 Space
    pub async fn accept_recruitment_join(
        &self,
        recruitment_id: &str,
        target_id: &str,
        recruitment: &crate::negotiation::RecruitmentRequest,
    ) -> Result<Space, GaggleError> {
        // 更新招募状态
        {
            let db = self.db.lock().unwrap();
            db.execute(
                "UPDATE recruitment_requests SET status = 'accepted' WHERE id = ?1",
                params![recruitment_id],
            )?;
        }

        // 获取并修改 space
        let mut space = self.get_space(&recruitment.space_id).await?
            .ok_or_else(|| GaggleError::NotFound("Space not found".to_string()))?;

        if !space.agent_ids.contains(&target_id.to_string()) {
            space.agent_ids.push(target_id.to_string());
        }
        if !space.joined_agent_ids.contains(&target_id.to_string()) {
            space.joined_agent_ids.push(target_id.to_string());
        }
        if space.all_joined() && !space.status.is_terminal() {
            if space.status == SpaceStatus::Created {
                space.activate().map_err(|e| GaggleError::ValidationError(e))?;
                let _ = self.record_transition(
                    &space.id, "created", "active",
                    "recruitment_accepted", Some(target_id), space.version,
                );
            }
            // If already Active, no transition needed
        }
        space.bump_version();
        self.persist_space(&space)?;
        self.update_cache(&space).await;
        Ok(space)
    }

    /// 拒绝招募（target 调用）
    pub async fn reject_recruitment(
        &self,
        recruitment_id: &str,
        target_id: &str,
    ) -> Result<crate::negotiation::RecruitmentRequest, GaggleError> {
        let mut recruitment = self.get_recruitment(recruitment_id).await?
            .ok_or_else(|| GaggleError::NotFound(format!("Recruitment not found: {}", recruitment_id)))?;

        if recruitment.target_id != target_id {
            return Err(GaggleError::Forbidden("Only the target can reject".to_string()));
        }

        recruitment.reject().map_err(|e| GaggleError::ValidationError(e))?;

        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE recruitment_requests SET status = 'rejected' WHERE id = ?1",
            params![recruitment.id],
        )?;

        Ok(recruitment)
    }
}

#[cfg(test)]
mod transition_log_tests {
    use super::*;
    use crate::agents::types::{AgentType, RegisterRequest};
    use crate::AgentRegistry;
    use crate::negotiation::space::SpaceStatus;

    fn setup() -> (AgentRegistry, SpaceManager) {
        let registry = AgentRegistry::new(":memory:").unwrap();
        let sm = SpaceManager::new(":memory:").unwrap();
        (registry, sm)
    }

    async fn make_agent(registry: &AgentRegistry, name: &str) -> Agent {
        let resp: crate::agents::types::RegisterResponse = registry.register(
            RegisterRequest {
                agent_type: AgentType::Consumer,
                name: name.to_string(),
                metadata: serde_json::json!({}),
                public_key: None,
                organization: None,
                callback_url: None,
            },
            None,
        ).await.unwrap();
        registry.get_by_id(&resp.id).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn test_transition_recorded_on_activate() {
        let (registry, sm) = setup();
        let creator = make_agent(&registry, "creator").await;
        let invitee = make_agent(&registry, "invitee").await;

        let space = sm.create_space(
            &creator,
            crate::negotiation::CreateSpaceRequest {
                name: "Test".into(),
                invitee_ids: vec![invitee.id.clone()],
                context: serde_json::json!({}),
            },
            None,
        ).await.unwrap();

        // Join both agents → activates space
        sm.join_space(&invitee, &space.id).await.unwrap();

        let history = sm.get_transition_history(&space.id, 100, None).await.unwrap();
        assert_eq!(history.transitions.len(), 1);
        assert_eq!(history.transitions[0].from_status, "created");
        assert_eq!(history.transitions[0].to_status, "active");
        assert_eq!(history.transitions[0].trigger, "all_agents_joined");
        assert!(history.transitions[0].transition_hash.is_some());
        assert!(history.transitions[0].prev_hash.is_some());
    }

    #[tokio::test]
    async fn test_transition_recorded_on_close() {
        let (registry, sm) = setup();
        let creator = make_agent(&registry, "creator").await;
        let invitee = make_agent(&registry, "invitee").await;

        let space = sm.create_space(
            &creator,
            crate::negotiation::CreateSpaceRequest {
                name: "Test".into(),
                invitee_ids: vec![invitee.id.clone()],
                context: serde_json::json!({}),
            },
            None,
        ).await.unwrap();

        sm.join_space(&invitee, &space.id).await.unwrap();

        // Close as concluded
        sm.close_space(&creator, &space.id, crate::negotiation::CloseSpaceRequest {
            conclusion: "concluded".into(),
            final_terms: None,
        }).await.unwrap();

        let history = sm.get_transition_history(&space.id, 100, None).await.unwrap();
        assert_eq!(history.transitions.len(), 2);
        assert_eq!(history.transitions[1].from_status, "active");
        assert_eq!(history.transitions[1].to_status, "concluded");
    }

    #[tokio::test]
    async fn test_transition_hash_chain_integrity() {
        let (registry, sm) = setup();
        let creator = make_agent(&registry, "creator").await;
        let invitee = make_agent(&registry, "invitee").await;

        let space = sm.create_space(
            &creator,
            crate::negotiation::CreateSpaceRequest {
                name: "Test".into(),
                invitee_ids: vec![invitee.id.clone()],
                context: serde_json::json!({}),
            },
            None,
        ).await.unwrap();

        sm.join_space(&invitee, &space.id).await.unwrap();
        sm.close_space(&creator, &space.id, crate::negotiation::CloseSpaceRequest {
            conclusion: "concluded".into(),
            final_terms: None,
        }).await.unwrap();

        let (total, verified, failed) = sm.verify_transition_chain(&space.id).await.unwrap();
        assert_eq!(total, 2);
        assert_eq!(verified, 2);
        assert_eq!(failed, 0, "hash chain should be intact");
    }

    #[tokio::test]
    async fn test_transition_history_pagination() {
        let (registry, sm) = setup();
        let creator = make_agent(&registry, "creator").await;
        let invitee = make_agent(&registry, "invitee").await;

        let space = sm.create_space(
            &creator,
            crate::negotiation::CreateSpaceRequest {
                name: "Test".into(),
                invitee_ids: vec![invitee.id.clone()],
                context: serde_json::json!({}),
            },
            None,
        ).await.unwrap();

        sm.join_space(&invitee, &space.id).await.unwrap();
        sm.close_space(&creator, &space.id, crate::negotiation::CloseSpaceRequest {
            conclusion: "concluded".into(),
            final_terms: None,
        }).await.unwrap();

        // Total count should be 2
        let full = sm.get_transition_history(&space.id, 100, None).await.unwrap();
        assert_eq!(full.total, 2);

        // Limit to 1
        let page = sm.get_transition_history(&space.id, 1, None).await.unwrap();
        assert_eq!(page.transitions.len(), 1);
        assert_eq!(page.total, 2); // total is always full count
    }

    #[tokio::test]
    async fn test_no_transitions_for_new_space() {
        let (registry, sm) = setup();
        let creator = make_agent(&registry, "creator").await;

        let space = sm.create_space(
            &creator,
            crate::negotiation::CreateSpaceRequest {
                name: "Empty".into(),
                invitee_ids: vec![],
                context: serde_json::json!({}),
            },
            None,
        ).await.unwrap();

        let history = sm.get_transition_history(&space.id, 100, None).await.unwrap();
        assert_eq!(history.transitions.len(), 0);
        assert_eq!(history.total, 0);
    }

    #[tokio::test]
    async fn test_expire_space_records_transition() {
        let (registry, sm) = setup();
        let creator = make_agent(&registry, "creator").await;

        // Create a space with rules that have a past deadline
        let space = sm.create_space_with_rules(
            &creator,
            crate::negotiation::CreateSpaceRequest {
                name: "Expiring".into(),
                invitee_ids: vec![],
                context: serde_json::json!({}),
            },
            None,
            Some(crate::negotiation::rules::SpaceRulesOverrides {
                rounds: Some(Some(crate::negotiation::rules::RoundConfig {
                    max_rounds: 3,
                    deadline: Some(chrono::Utc::now().timestamp_millis() - 10000),
                    auto_advance: false,
                    evaluation_criteria: None,
                    share_best_terms: false,
                })),
                visibility: None,
                can_propose: None,
                lock_condition: None,
                reveal_mode: None,
                roles: None,
                max_participants: None,
                join_policy: None,
                transitions: None,
            }),
        ).await.unwrap();

        // Manually set status to Active via DB (bypass persist_space version check)
        {
            let db = sm.db.lock().unwrap();
            db.execute(
                "UPDATE spaces SET status = '\"active\"' WHERE id = ?1",
                rusqlite::params![space.id],
            ).unwrap();
        }
        // Clear cache to force reload
        sm.spaces.remove(&space.id);

        // Expire
        sm.expire_space(&space.id).await.unwrap();

        let history = sm.get_transition_history(&space.id, 100, None).await.unwrap();
        assert_eq!(history.transitions.len(), 1);
        assert_eq!(history.transitions[0].from_status, "active");
        assert_eq!(history.transitions[0].to_status, "expired");
        assert_eq!(history.transitions[0].trigger, "lifecycle_governor");
        assert!(history.transitions[0].agent_id.is_none());
    }
}
