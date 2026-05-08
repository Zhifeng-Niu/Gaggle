//! SubSpace — 父 Space 内的子谈判频道
//!
//! 子空间用于在父 Space 内进行深度 1v1 或多方谈判。
//! 成员必须是父空间成员的子集，消息和提案在子空间内独立路由。

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::rules::SpaceRules;
use super::space::SpaceStatus;

/// 子空间结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubSpace {
    /// 唯一标识符
    pub id: String,
    /// 父 Space ID
    pub parent_space_id: String,
    /// 子空间名称
    pub name: String,
    /// 创建者 Agent ID（必须是父空间成员）
    pub creator_id: String,
    /// 成员 Agent IDs（必须是父空间成员的子集）
    pub agent_ids: Vec<String>,
    /// 子空间规则（可覆盖父空间的部分规则）
    pub rules: SpaceRules,
    /// 子空间状态
    pub status: SpaceStatus,
    /// 创建时间戳
    pub created_at: i64,
    /// 更新时间戳
    pub updated_at: i64,
    /// 关闭时间戳
    pub closed_at: Option<i64>,
}

/// 子空间创建请求
#[derive(Debug, Deserialize)]
pub struct CreateSubSpaceRequest {
    pub name: String,
    /// 要加入子空间的 agent IDs（必须是父空间成员）
    pub agent_ids: Vec<String>,
    /// 可选规则覆盖
    pub rules: Option<super::rules::SpaceRulesOverrides>,
}

impl SubSpace {
    /// 创建子空间
    pub fn new(
        parent_space_id: String,
        creator_id: String,
        req: CreateSubSpaceRequest,
    ) -> Self {
        let now = Utc::now().timestamp_millis();
        let mut rules = SpaceRules::bilateral();
        if let Some(overrides) = req.rules {
            overrides.apply_to(&mut rules);
        }

        // creator 自动加入
        let mut agent_ids = req.agent_ids;
        if !agent_ids.contains(&creator_id) {
            agent_ids.push(creator_id.clone());
        }

        Self {
            id: Uuid::new_v4().to_string(),
            parent_space_id,
            name: req.name,
            creator_id,
            agent_ids,
            rules,
            status: SpaceStatus::Active,
            created_at: now,
            updated_at: now,
            closed_at: None,
        }
    }

    /// 检查 agent 是否是子空间成员
    pub fn is_member(&self, agent_id: &str) -> bool {
        self.agent_ids.contains(&agent_id.to_string())
    }

    /// 关闭子空间 — only legal from Created or Active.
    pub fn close(&mut self, concluded: bool, trigger: &str, closer_id: Option<&str>) -> Result<(), String> {
        let target = if concluded {
            SpaceStatus::Concluded
        } else {
            SpaceStatus::Cancelled
        };
        if !self.status.can_transition_to(&target) {
            return Err(format!(
                "cannot close sub-space: current status is {:?}, which is terminal",
                self.status
            ));
        }
        tracing::info!(
            subspace_id = %self.id, from = ?self.status, to = ?target,
            trigger = %trigger, closer_id = ?closer_id,
            "SubSpace status transition"
        );
        self.status = target;
        self.closed_at = Some(Utc::now().timestamp_millis());
        self.updated_at = Utc::now().timestamp_millis();
        Ok(())
    }
}
