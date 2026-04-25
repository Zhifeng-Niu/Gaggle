//! Session管理

use crate::agents::Agent;
use crate::error::GaggleError;
use crate::negotiation::crypt::{encrypt_content, generate_key};
use crate::negotiation::proposal::{
    BestTermsShared, CreateRfpRequest, Proposal, ProposalResponseAction, ProposalStatus,
    ProposalType, RespondToProposalRequest, ShareBestTermsRequest, SubmitProposalRequest,
};
use crate::negotiation::space::{
    CloseSpaceRequest, CreateSpaceRequest, EncryptedContent, MessageVisibility, SendMessageRequest,
    Space, SpaceMessage, SpaceStatus, SpaceType,
};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};

/// Space 行列索引常量（与 SELECT 列顺序对应）
/// SELECT id, name, creator_id, agent_ids, joined_agent_ids, status, space_type, rfp_context, context, encryption_key, created_at, updated_at, closed_at
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

const SPACE_COLUMNS: &str = "id, name, creator_id, agent_ids, joined_agent_ids, status, space_type, rfp_context, context, encryption_key, created_at, updated_at, closed_at, buyer_id, seller_id";

fn map_space(row: &rusqlite::Row) -> rusqlite::Result<Space> {
    let agent_ids_str: String = row.get(COL_AGENT_IDS)?;
    let joined_ids_str: String = row.get(COL_JOINED_IDS)?;
    let status_str: String = row.get(COL_STATUS)?;
    let space_type_str: String = row.get(COL_SPACE_TYPE)?;
    let rfp_context_str: Option<String> = row.get(COL_RFP_CONTEXT)?;
    let context_str: String = row.get(COL_CONTEXT)?;

    Ok(Space {
        id: row.get(COL_ID)?,
        name: row.get(COL_NAME)?,
        creator_id: row.get(COL_CREATOR_ID)?,
        agent_ids: serde_json::from_str(&agent_ids_str).unwrap_or_default(),
        joined_agent_ids: serde_json::from_str(&joined_ids_str).unwrap_or_default(),
        status: serde_json::from_str(&status_str).unwrap_or(SpaceStatus::Created),
        space_type: SpaceType::from_str_safe(&space_type_str),
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
    })
}

pub struct SpaceManager {
    db: Arc<Mutex<Connection>>,
    spaces: RwLock<HashMap<String, Space>>,
    broadcast_txs: RwLock<HashMap<String, broadcast::Sender<String>>>,
}

impl SpaceManager {
    pub fn new(db_path: &str) -> Result<Self, GaggleError> {
        let conn = Connection::open(db_path)?;

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

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            spaces: RwLock::new(HashMap::new()),
            broadcast_txs: RwLock::new(HashMap::new()),
        })
    }

    pub async fn create_space(
        &self,
        creator: &Agent,
        req: CreateSpaceRequest,
        my_role: Option<String>,
    ) -> Result<Space, GaggleError> {
        let encryption_key = generate_key();

        let space = Space::new(
            req.name,
            creator.id.clone(),
            req.invitee_ids,
            req.context,
            encryption_key,
            my_role,
        );

        {
            let db = self.db.lock().await;
            db.execute(
                &format!("INSERT INTO spaces ({SPACE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"),
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
                ],
            )?;
        }

        let mut spaces = self.spaces.write().await;
        spaces.insert(space.id.clone(), space.clone());

        let (tx, _) = broadcast::channel::<String>(100);
        let mut broadcast_txs = self.broadcast_txs.write().await;
        broadcast_txs.insert(space.id.clone(), tx);

        Ok(space)
    }

    pub async fn get_space(&self, space_id: &str) -> Result<Option<Space>, GaggleError> {
        {
            let spaces = self.spaces.read().await;
            if let Some(space) = spaces.get(space_id) {
                return Ok(Some(space.clone()));
            }
        }

        let result = {
            let db = self.db.lock().await;
            let mut stmt =
                db.prepare(&format!("SELECT {SPACE_COLUMNS} FROM spaces WHERE id = ?1"))?;

            stmt.query_row(params![space_id], map_space).optional()?
        };

        if let Some(ref s) = result {
            let mut spaces = self.spaces.write().await;
            spaces.insert(s.id.clone(), s.clone());
        }

        Ok(result)
    }

    pub async fn join_space(&self, agent: &Agent, space_id: &str) -> Result<Space, GaggleError> {
        let mut space = self
            .get_space(space_id)
            .await?
            .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

        if !space.is_member(&agent.id) {
            return Err(GaggleError::Forbidden(
                "Agent not invited to this space".to_string(),
            ));
        }

        if space.status != SpaceStatus::Created {
            return Err(GaggleError::SpaceClosed(
                "Space is not in Created status".to_string(),
            ));
        }

        // 如果已经 join 过，直接返回
        if space.joined_agent_ids.contains(&agent.id) {
            return Ok(space);
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
            space.activate();
        }

        {
            let db = self.db.lock().await;
            db.execute(
                "UPDATE spaces SET joined_agent_ids = ?1, status = ?2, updated_at = ?3, buyer_id = ?4, seller_id = ?5 WHERE id = ?6",
                params![
                    serde_json::to_string(&space.joined_agent_ids)?,
                    serde_json::to_string(&space.status)?,
                    space.updated_at,
                    space.buyer_id,
                    space.seller_id,
                    space_id,
                ],
            )?;
        }

        let mut spaces = self.spaces.write().await;
        spaces.insert(space.id.clone(), space.clone());

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
            let db = self.db.lock().await;
            db.execute(
                &format!("INSERT INTO spaces ({SPACE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"),
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
                ],
            )?;
        }

        let mut spaces = self.spaces.write().await;
        spaces.insert(space.id.clone(), space.clone());

        let (tx, _) = broadcast::channel::<String>(100);
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

        if space.status != SpaceStatus::Active {
            return Err(GaggleError::SpaceClosed("Space is not active".to_string()));
        }

        let count = {
            let db = self.db.lock().await;
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM space_messages WHERE space_id = ?1",
                params![space_id],
                |row| row.get(0),
            )?;
            count
        };

        let round = space.current_round(count as u32 + 1);
        let encrypted = encrypt_content(&req.content, &space.encryption_key)?;

        let message = SpaceMessage::new(
            space_id.to_string(),
            agent.id.clone(),
            req.msg_type,
            encrypted,
            round,
            req.metadata,
        );

        {
            let db = self.db.lock().await;
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

        if space.status != SpaceStatus::Active {
            return Err(GaggleError::SpaceClosed("Space is not active".to_string()));
        }

        let count = {
            let db = self.db.lock().await;
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM space_messages WHERE space_id = ?1",
                params![space_id],
                |row| row.get(0),
            )?;
            count
        };

        let round = space.current_round(count as u32 + 1);
        let encrypted = encrypt_content(&req.content, &space.encryption_key)?;

        let message = SpaceMessage::new_directed(
            space_id.to_string(),
            agent.id.clone(),
            req.msg_type,
            encrypted,
            round,
            recipient_ids,
            req.metadata,
        );

        {
            let db = self.db.lock().await;
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
        let db = self.db.lock().await;

        let all_messages = if let Some(after_ts) = after {
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

        Ok(SpaceMessage {
            id: row.get(0)?,
            space_id: row.get(1)?,
            sender_id: row.get(2)?,
            msg_type: serde_json::from_str(&msg_type_str)
                .unwrap_or(crate::negotiation::message::MessageType::Text),
            content: serde_json::from_str(&content_str).unwrap_or(EncryptedContent {
                cipher: String::new(),
                nonce: String::new(),
                version: 1,
            }),
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
        space.close(concluded);

        {
            let db = self.db.lock().await;
            db.execute(
                "UPDATE spaces SET status = ?1, updated_at = ?2, closed_at = ?3 WHERE id = ?4",
                params![
                    serde_json::to_string(&space.status)?,
                    space.updated_at,
                    space.closed_at,
                    space_id,
                ],
            )?;
        }

        let mut spaces = self.spaces.write().await;
        spaces.insert(space.id.clone(), space.clone());

        Ok(space)
    }

    pub async fn get_agent_spaces(&self, agent_id: &str) -> Result<Vec<Space>, GaggleError> {
        let search_pattern = format!("%\"{}\"%", agent_id);

        let db = self.db.lock().await;
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
        let db = self.db.lock().await;
        let count: i64 =
            db.query_row("SELECT COUNT(*) FROM spaces", [], |row| row.get(0))?;
        Ok(count as usize)
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

        // RFP Space 中，只有 Provider（非创建者）可以提交提案
        if space.space_type == SpaceType::Rfp && agent.id == space.creator_id {
            return Err(GaggleError::Forbidden(
                "RFP creator cannot submit proposals".to_string(),
            ));
        }

        // 获取当前轮次
        let round = {
            let db = self.db.lock().await;
            let count: i64 = db
                .query_row(
                    "SELECT COUNT(*) FROM proposals WHERE space_id = ?1",
                    params![space_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
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
            let db = self.db.lock().await;
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

        Ok(proposal)
    }

    /// 获取 Space 的所有提案
    pub async fn get_space_proposals(&self, space_id: &str) -> Result<Vec<Proposal>, GaggleError> {
        let db = self.db.lock().await;

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

        // 获取原提案
        let mut original_proposal = {
            let db = self.db.lock().await;
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
                original_proposal.accept();
                // 拒绝同一轮次的其他待处理提案
                {
                    let db = self.db.lock().await;
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
                original_proposal.reject();
                None
            }
            ProposalResponseAction::Counter => {
                original_proposal.supersede();
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
                    let db = self.db.lock().await;
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
            let db = self.db.lock().await;
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
        let db = self.db.lock().await;

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
            space.close(false);
        }
        // Active bilateral space + 非creator 离开 → cancel
        else if space.status == SpaceStatus::Active && space.space_type == SpaceType::Bilateral {
            space.close(false);
        }
        // RFP space + 单个 provider 离开 → 保持 Active（其他 providers 可继续）

        // 从 agent_ids 和 joined_agent_ids 中移除
        space.agent_ids.retain(|id| id != &agent.id);
        space.joined_agent_ids.retain(|id| id != &agent.id);
        space.updated_at = Utc::now().timestamp_millis();

        {
            let db = self.db.lock().await;
            db.execute(
                "UPDATE spaces SET agent_ids = ?1, joined_agent_ids = ?2, status = ?3, updated_at = ?4, closed_at = ?5 WHERE id = ?6",
                params![
                    serde_json::to_string(&space.agent_ids)?,
                    serde_json::to_string(&space.joined_agent_ids)?,
                    serde_json::to_string(&space.status)?,
                    space.updated_at,
                    space.closed_at,
                    space_id,
                ],
            )?;
        }

        let mut spaces = self.spaces.write().await;
        spaces.insert(space.id.clone(), space.clone());

        Ok(space)
    }
}
