//! Coalition — Space 内的自组织联盟
//!
//! Agent 可在 Space 内形成联盟，对内协调立场，对外统一谈判。
//! 联盟自动创建内部子空间用于成员间协调。

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 联盟状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CoalitionStatus {
    /// 活跃中
    Active,
    /// 已解散
    Disbanded,
}

impl CoalitionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disbanded => "disbanded",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "disbanded" => Self::Disbanded,
            _ => Self::Active,
        }
    }
}

/// 联盟结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coalition {
    /// 唯一标识符
    pub id: String,
    /// 所属 Space ID
    pub space_id: String,
    /// 联盟名称
    pub name: String,
    /// 领导者 Agent ID
    pub leader_id: String,
    /// 成员 Agent IDs
    pub member_ids: Vec<String>,
    /// 统一立场/出价范围（JSON）
    #[serde(default)]
    pub stance: Option<serde_json::Value>,
    /// 关联的内部子空间 ID（联盟内部协调用）
    pub internal_space_id: String,
    /// 状态
    pub status: CoalitionStatus,
    /// 创建时间戳
    pub created_at: i64,
    /// 更新时间戳
    pub updated_at: i64,
}

/// 创建联盟请求
#[derive(Debug, Deserialize)]
pub struct CreateCoalitionRequest {
    pub name: String,
    /// 初始成员 Agent IDs（必须是同一 Space 的成员）
    pub member_ids: Vec<String>,
    /// 初始立场
    #[serde(default)]
    pub stance: Option<serde_json::Value>,
}

/// 更新联盟立场请求
#[derive(Debug, Deserialize)]
pub struct UpdateStanceRequest {
    pub stance: serde_json::Value,
}

impl Coalition {
    /// 创建联盟（internal_space_id 由 SpaceManager 设置）
    pub fn new(
        space_id: String,
        leader_id: String,
        req: CreateCoalitionRequest,
        internal_space_id: String,
    ) -> Self {
        let now = Utc::now().timestamp_millis();

        // leader 自动是成员
        let mut member_ids = req.member_ids;
        if !member_ids.contains(&leader_id) {
            member_ids.push(leader_id.clone());
        }

        Self {
            id: Uuid::new_v4().to_string(),
            space_id,
            name: req.name,
            leader_id,
            member_ids,
            stance: req.stance,
            internal_space_id,
            status: CoalitionStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }

    /// 检查 agent 是否是成员
    pub fn is_member(&self, agent_id: &str) -> bool {
        self.member_ids.contains(&agent_id.to_string())
    }

    /// 检查 agent 是否是领导者
    pub fn is_leader(&self, agent_id: &str) -> bool {
        self.leader_id == agent_id
    }

    /// 添加成员
    pub fn add_member(&mut self, agent_id: &str) {
        if !self.member_ids.contains(&agent_id.to_string()) {
            self.member_ids.push(agent_id.to_string());
            self.updated_at = Utc::now().timestamp_millis();
        }
    }

    /// 移除成员（leader 不能离开，只能解散）
    pub fn remove_member(&mut self, agent_id: &str) -> bool {
        if agent_id == self.leader_id {
            return false;
        }
        let before = self.member_ids.len();
        self.member_ids.retain(|id| id != agent_id);
        if self.member_ids.len() < before {
            self.updated_at = Utc::now().timestamp_millis();
        }
        self.member_ids.len() < before
    }

    /// 解散联盟
    pub fn disband(&mut self) {
        self.status = CoalitionStatus::Disbanded;
        self.updated_at = Utc::now().timestamp_millis();
    }
}
