//! Negotiation Space定义

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use super::message::MessageType;
use super::rules::SpaceRules;

/// Space状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SpaceStatus {
    /// 已创建，等待双方加入
    Created,
    /// 谈判进行中
    Active,
    /// 已成交
    Concluded,
    /// 已取消
    Cancelled,
    /// 已过期
    Expired,
}

/// 状态转换记录 — 不可变事实，用于审计和 timeline reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusTransition {
    pub from: SpaceStatus,
    pub to: SpaceStatus,
    pub at: i64,
    pub trigger: String,
    pub agent_id: Option<String>,
}

impl SpaceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpaceStatus::Created => "created",
            SpaceStatus::Active => "active",
            SpaceStatus::Concluded => "concluded",
            SpaceStatus::Cancelled => "cancelled",
            SpaceStatus::Expired => "expired",
        }
    }

    /// Check whether transition from self to target is legal.
    ///
    /// Legal transitions:
    ///   Created   → Active | Cancelled | Expired
    ///   Active    → Concluded | Cancelled | Expired
    ///   Concluded → (terminal)
    ///   Cancelled → (terminal)
    ///   Expired   → (terminal)
    pub fn can_transition_to(&self, target: &SpaceStatus) -> bool {
        matches!(
            (self, target),
            (SpaceStatus::Created, SpaceStatus::Active)
                | (SpaceStatus::Created, SpaceStatus::Cancelled)
                | (SpaceStatus::Created, SpaceStatus::Expired)
                | (SpaceStatus::Active, SpaceStatus::Concluded)
                | (SpaceStatus::Active, SpaceStatus::Cancelled)
                | (SpaceStatus::Active, SpaceStatus::Expired)
        )
    }

    /// Returns true for terminal states that cannot transition further.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SpaceStatus::Concluded | SpaceStatus::Cancelled | SpaceStatus::Expired
        )
    }
}

/// Space类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpaceType {
    /// 双边谈判（1对1）
    #[default]
    Bilateral,
    /// 多方 RFP（1对N）
    Rfp,
}

impl std::str::FromStr for SpaceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "rfp" => Ok(SpaceType::Rfp),
            "bilateral" => Ok(SpaceType::Bilateral),
            _ => Err(format!("Unknown SpaceType: {}", s)),
        }
    }
}

impl SpaceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpaceType::Bilateral => "bilateral",
            SpaceType::Rfp => "rfp",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        s.parse().unwrap_or(SpaceType::Bilateral)
    }
}

/// 消息可见性
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageVisibility {
    /// 广播给所有成员
    #[default]
    Broadcast,
    /// 定向发送（仅 recipient_ids 可见）
    Directed,
    /// 私密消息（仅发送者可见）
    Private,
}

impl MessageVisibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageVisibility::Broadcast => "broadcast",
            MessageVisibility::Directed => "directed",
            MessageVisibility::Private => "private",
        }
    }
}

/// Space主体结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Space {
    /// 唯一标识符
    pub id: String,
    /// 空间名称/主题
    pub name: String,
    /// 创建者Agent ID
    pub creator_id: String,
    /// 参与方Agent IDs
    pub agent_ids: Vec<String>,
    /// 已加入的Agent IDs（用于判断是否所有人都已加入）
    #[serde(default)]
    pub joined_agent_ids: Vec<String>,
    /// 当前状态
    pub status: SpaceStatus,
    /// Space 类型（双边/多方 RFP）— 向后兼容，从 rules 推导
    #[serde(default)]
    pub space_type: SpaceType,
    /// 空间规则配置（Phase 6+）
    #[serde(default)]
    pub rules: SpaceRules,
    /// RFP 上下文（仅 RFP 类型有效）— 向后兼容，从 rules.rounds 推导
    #[serde(default)]
    pub rfp_context: Option<JsonValue>,
    /// 共享上下文（需求描述、约束等）
    pub context: JsonValue,
    /// 对称密钥（平台持有，用于加密存储）
    pub encryption_key: String,
    /// 创建时间戳
    pub created_at: i64,
    /// 更新时间戳
    pub updated_at: i64,
    /// 关闭时间戳
    pub closed_at: Option<i64>,
    /// Buyer Agent ID（per-space 角色）
    #[serde(default)]
    pub buyer_id: Option<String>,
    /// Seller Agent ID（per-space 角色）
    #[serde(default)]
    pub seller_id: Option<String>,
    /// Pending join requests — 仅 ApprovalRequired 模式使用
    /// Vec of (agent_id, requested_at_timestamp)
    #[serde(default)]
    pub pending_join_requests: Vec<(String, i64)>,
    /// 乐观锁版本号：每次 Space 变更递增，用于检测并发修改冲突
    #[serde(default)]
    pub version: u64,
}

impl Space {
    /// 创建新Space
    /// my_role: 创建者在此 space 中的角色 ("buyer" 或 "seller")，默认 "buyer"
    pub fn new(
        name: String,
        creator_id: String,
        invitee_ids: Vec<String>,
        context: JsonValue,
        encryption_key: String,
        my_role: Option<String>,
    ) -> Self {
        let now = Utc::now().timestamp_millis();
        let mut agent_ids = vec![creator_id.clone()];
        agent_ids.extend(invitee_ids);

        let (buyer_id, seller_id) = match my_role.as_deref() {
            Some("seller") => (None, Some(creator_id.clone())),
            _ => (Some(creator_id.clone()), None),
        };

        Self {
            id: Uuid::new_v4().to_string(),
            name,
            creator_id: creator_id.clone(),
            agent_ids,
            joined_agent_ids: vec![creator_id], // creator 创建时即已加入
            status: SpaceStatus::Created,
            space_type: SpaceType::Bilateral,
            rules: SpaceRules::bilateral(),
            rfp_context: None,
            context,
            encryption_key,
            created_at: now,
            updated_at: now,
            closed_at: None,
            buyer_id,
            seller_id,
            pending_join_requests: Vec::new(),
            version: 1,
        }
    }

    /// 创建 RFP Space（creator 固定是 buyer）
    pub fn new_rfp(
        name: String,
        creator_id: String,
        provider_ids: Vec<String>,
        rfp_context: crate::negotiation::RfpContext,
        context: JsonValue,
        encryption_key: String,
    ) -> Self {
        let now = Utc::now().timestamp_millis();
        let mut agent_ids = vec![creator_id.clone()];
        agent_ids.extend(provider_ids);

        let mut rules = SpaceRules::rfp();
        rules.apply_rfp_overrides(
            rfp_context.allowed_rounds,
            rfp_context.evaluation_criteria.clone(),
            rfp_context.deadline,
            rfp_context.share_best_terms,
        );

        Self {
            id: Uuid::new_v4().to_string(),
            name,
            creator_id: creator_id.clone(),
            agent_ids,
            joined_agent_ids: vec![creator_id.clone()],
            status: SpaceStatus::Created,
            space_type: SpaceType::Rfp,
            rules,
            rfp_context: Some(serde_json::to_value(rfp_context).unwrap_or_default()),
            context,
            encryption_key,
            created_at: now,
            updated_at: now,
            closed_at: None,
            buyer_id: Some(creator_id), // RFP creator is always buyer
            seller_id: None,
            pending_join_requests: Vec::new(),
            version: 1,
        }
    }

    /// 递增 version，返回新的 version 值
    pub fn bump_version(&mut self) -> u64 {
        self.version += 1;
        self.updated_at = Utc::now().timestamp_millis();
        self.version
    }
    pub fn all_joined(&self) -> bool {
        self.agent_ids
            .iter()
            .all(|id| self.joined_agent_ids.contains(id))
    }

    /// 激活Space（双方都已加入）
    ///
    /// Only legal from `Created` status.
    pub fn activate(&mut self) -> Result<StatusTransition, String> {
        let from = self.status.clone();
        let target = SpaceStatus::Active;
        if !self.status.can_transition_to(&target) {
            return Err(format!(
                "cannot activate space: current status is {:?}, expected Created",
                self.status
            ));
        }
        self.status = target;
        self.updated_at = Utc::now().timestamp_millis();
        Ok(StatusTransition {
            from,
            to: SpaceStatus::Active,
            at: Utc::now().timestamp_millis(),
            trigger: "all_agents_joined".to_string(),
            agent_id: None,
        })
    }

    /// 关闭Space
    ///
    /// Legal from `Created` or `Active`.
    /// Terminal operation — cannot be reversed.
    pub fn close(&mut self, concluded: bool, trigger: &str, agent_id: Option<&str>) -> Result<StatusTransition, String> {
        let from = self.status.clone();
        let target = if concluded {
            SpaceStatus::Concluded
        } else {
            SpaceStatus::Cancelled
        };
        if !self.status.can_transition_to(&target) {
            return Err(format!(
                "cannot close space: current status is {:?}, which is terminal",
                self.status
            ));
        }
        self.status = target.clone();
        self.closed_at = Some(Utc::now().timestamp_millis());
        self.updated_at = Utc::now().timestamp_millis();
        Ok(StatusTransition {
            from,
            to: target,
            at: Utc::now().timestamp_millis(),
            trigger: trigger.to_string(),
            agent_id: agent_id.map(|s| s.to_string()),
        })
    }

    /// 检查Agent是否是成员
    pub fn is_member(&self, agent_id: &str) -> bool {
        self.agent_ids.contains(&agent_id.to_string())
    }

    /// 获取 agent 在此 space 中的角色
    pub fn get_role(&self, agent_id: &str) -> Option<&str> {
        if self.buyer_id.as_deref() == Some(agent_id) {
            Some("buyer")
        } else if self.seller_id.as_deref() == Some(agent_id) {
            Some("seller")
        } else {
            None
        }
    }

    /// 获取当前轮次（基于消息数量估算）
    pub fn current_round(&self, message_count: u32) -> u32 {
        message_count / self.agent_ids.len() as u32 + 1
    }
}

/// Space消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceMessage {
    /// 唯一标识符
    pub id: String,
    /// 所属Space
    pub space_id: String,
    /// 发送者Agent ID
    pub sender_id: String,
    /// 消息类型
    pub msg_type: MessageType,
    /// 消息内容（明文；旧数据为加密 JSON，读取时自动解密）
    pub content: String,
    /// Unix时间戳（毫秒）
    pub timestamp: i64,
    /// 谈判轮次
    pub round: u32,
    /// 扩展元数据
    pub metadata: Option<JsonValue>,
    /// 消息可见性
    #[serde(default)]
    pub visibility: MessageVisibility,
    /// 接收者 IDs（定向消息时使用）
    #[serde(default)]
    pub recipient_ids: Vec<String>,
}

impl SpaceMessage {
    /// 创建新消息
    pub fn new(
        space_id: String,
        sender_id: String,
        msg_type: MessageType,
        content: String,
        round: u32,
        metadata: Option<JsonValue>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            space_id,
            sender_id,
            msg_type,
            content,
            timestamp: Utc::now().timestamp_millis(),
            round,
            metadata,
            visibility: MessageVisibility::Broadcast,
            recipient_ids: Vec::new(),
        }
    }

    /// 创建定向消息
    pub fn new_directed(
        space_id: String,
        sender_id: String,
        msg_type: MessageType,
        content: String,
        round: u32,
        recipient_ids: Vec<String>,
        metadata: Option<JsonValue>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            space_id,
            sender_id,
            msg_type,
            content,
            timestamp: Utc::now().timestamp_millis(),
            round,
            metadata,
            visibility: MessageVisibility::Directed,
            recipient_ids,
        }
    }

    /// 检查消息是否对指定 Agent 可见
    pub fn is_visible_to(&self, agent_id: &str) -> bool {
        match self.visibility {
            MessageVisibility::Broadcast => true,
            MessageVisibility::Private => self.sender_id == agent_id,
            MessageVisibility::Directed => {
                self.sender_id == agent_id || self.recipient_ids.contains(&agent_id.to_string())
            }
        }
    }
}

/// 加密内容
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedContent {
    /// 密文（Base64）
    pub cipher: String,
    /// 随机nonce（Base64）
    pub nonce: String,
    /// 加密版本，兼容升级
    pub version: u8,
}

impl EncryptedContent {
    /// 创建新的加密内容
    pub fn new(cipher: String, nonce: String) -> Self {
        Self {
            cipher,
            nonce,
            version: 1,
        }
    }
}

/// Space创建请求
#[derive(Debug, Deserialize)]
pub struct CreateSpaceRequest {
    pub name: String,
    pub invitee_ids: Vec<String>,
    pub context: JsonValue,
}

/// Space消息请求
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub msg_type: MessageType,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<JsonValue>,
}

/// 关闭Space请求
#[derive(Debug, Deserialize)]
pub struct CloseSpaceRequest {
    pub conclusion: String, // "concluded" or "cancelled"
    #[serde(default)]
    pub final_terms: Option<JsonValue>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_space_status_legal_transitions() {
        // Created → Active ✅
        assert!(SpaceStatus::Created.can_transition_to(&SpaceStatus::Active));
        // Created → Cancelled ✅
        assert!(SpaceStatus::Created.can_transition_to(&SpaceStatus::Cancelled));
        // Created → Expired ✅
        assert!(SpaceStatus::Created.can_transition_to(&SpaceStatus::Expired));
        // Active → Concluded ✅
        assert!(SpaceStatus::Active.can_transition_to(&SpaceStatus::Concluded));
        // Active → Cancelled ✅
        assert!(SpaceStatus::Active.can_transition_to(&SpaceStatus::Cancelled));
        // Active → Expired ✅
        assert!(SpaceStatus::Active.can_transition_to(&SpaceStatus::Expired));
    }

    #[test]
    fn test_space_status_illegal_transitions() {
        // Terminal states cannot transition to anything
        for terminal in &[SpaceStatus::Concluded, SpaceStatus::Cancelled, SpaceStatus::Expired] {
            for target in &[
                SpaceStatus::Created, SpaceStatus::Active,
                SpaceStatus::Concluded, SpaceStatus::Cancelled, SpaceStatus::Expired,
            ] {
                assert!(!terminal.can_transition_to(target),
                    "terminal {:?} should not transition to {:?}", terminal, target);
            }
        }
        // Created → Created (no-op) should fail
        assert!(!SpaceStatus::Created.can_transition_to(&SpaceStatus::Created));
        // Active → Created (backwards) should fail
        assert!(!SpaceStatus::Active.can_transition_to(&SpaceStatus::Created));
    }

    #[test]
    fn test_space_activate_from_created() {
        let mut space = Space {
            id: "test".into(),
            name: "Test".into(),
            creator_id: "agent-1".into(),
            agent_ids: vec!["agent-1".into(), "agent-2".into()],
            joined_agent_ids: vec!["agent-1".into(), "agent-2".into()],
            status: SpaceStatus::Created,
            space_type: SpaceType::Bilateral,
            rules: Default::default(),
            rfp_context: None,
            context: serde_json::json!({}),
            encryption_key: "key".into(),
            created_at: 0,
            updated_at: 0,
            closed_at: None,
            buyer_id: None,
            seller_id: None,
            pending_join_requests: vec![],
            version: 1,
        };
        assert!(space.activate().is_ok());
        assert_eq!(space.status, SpaceStatus::Active);
    }

    #[test]
    fn test_space_activate_from_active_fails() {
        let mut space = Space {
            id: "test".into(), name: "Test".into(), creator_id: "a".into(),
            agent_ids: vec![], joined_agent_ids: vec![],
            status: SpaceStatus::Active, space_type: SpaceType::Bilateral,
            rules: Default::default(), rfp_context: None,
            context: serde_json::json!({}), encryption_key: "k".into(),
            created_at: 0, updated_at: 0, closed_at: None,
            buyer_id: None, seller_id: None, pending_join_requests: vec![],
            version: 1,
        };
        assert!(space.activate().is_err());
    }

    #[test]
    fn test_space_close_from_active() {
        let mut space = Space {
            id: "test".into(), name: "Test".into(), creator_id: "a".into(),
            agent_ids: vec![], joined_agent_ids: vec![],
            status: SpaceStatus::Active, space_type: SpaceType::Bilateral,
            rules: Default::default(), rfp_context: None,
            context: serde_json::json!({}), encryption_key: "k".into(),
            created_at: 0, updated_at: 0, closed_at: None,
            buyer_id: None, seller_id: None, pending_join_requests: vec![],
            version: 1,
        };
        assert!(space.close(true, "test", None).is_ok());
        assert_eq!(space.status, SpaceStatus::Concluded);
    }

    #[test]
    fn test_space_close_from_terminal_fails() {
        let mut space = Space {
            id: "test".into(), name: "Test".into(), creator_id: "a".into(),
            agent_ids: vec![], joined_agent_ids: vec![],
            status: SpaceStatus::Concluded, space_type: SpaceType::Bilateral,
            rules: Default::default(), rfp_context: None,
            context: serde_json::json!({}), encryption_key: "k".into(),
            created_at: 0, updated_at: 0, closed_at: None,
            buyer_id: None, seller_id: None, pending_join_requests: vec![],
            version: 1,
        };
        assert!(space.close(false, "test", None).is_err());
        assert_eq!(space.status, SpaceStatus::Concluded); // unchanged
    }

    #[test]
    fn test_is_terminal() {
        assert!(!SpaceStatus::Created.is_terminal());
        assert!(!SpaceStatus::Active.is_terminal());
        assert!(SpaceStatus::Concluded.is_terminal());
        assert!(SpaceStatus::Cancelled.is_terminal());
        assert!(SpaceStatus::Expired.is_terminal());
    }
}
