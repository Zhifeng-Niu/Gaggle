//! Recruitment — 谈判进行中的外部 Agent 招募
//!
//! 允许有邀请权限的 Space 成员招募外部 Agent 加入谈判。
//! 受 SpaceRules 的 can_invite、max_participants、join_policy 约束。

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 招募请求状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RecruitmentStatus {
    /// 等待响应
    Pending,
    /// 已接受
    Accepted,
    /// 已拒绝
    Rejected,
    /// 已过期
    Expired,
}

impl RecruitmentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "accepted" => Self::Accepted,
            "rejected" => Self::Rejected,
            "expired" => Self::Expired,
            _ => Self::Pending,
        }
    }
}

/// 招募请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecruitmentRequest {
    /// 唯一标识符
    pub id: String,
    /// 所属 Space ID
    pub space_id: String,
    /// 发起招募的 Agent ID
    pub recruiter_id: String,
    /// 被招募的 Agent ID
    pub target_id: String,
    /// 招募角色
    pub role: String,
    /// 招募说明
    pub pitch: String,
    /// 状态
    pub status: RecruitmentStatus,
    /// 创建时间
    pub created_at: i64,
}

/// 创建招募请求
#[derive(Debug, Deserialize)]
pub struct CreateRecruitmentRequest {
    pub target_id: String,
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(default)]
    pub pitch: String,
}

fn default_role() -> String {
    "participant".to_string()
}

impl RecruitmentRequest {
    /// 创建新招募请求
    pub fn new(space_id: String, recruiter_id: String, req: CreateRecruitmentRequest) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            space_id,
            recruiter_id,
            target_id: req.target_id,
            role: req.role,
            pitch: req.pitch,
            status: RecruitmentStatus::Pending,
            created_at: Utc::now().timestamp_millis(),
        }
    }

    /// 接受招募 — only legal from Pending.
    pub fn accept(&mut self) -> Result<(), String> {
        if self.status != RecruitmentStatus::Pending {
            return Err(format!(
                "cannot accept recruitment: current status is {:?}, expected Pending",
                self.status
            ));
        }
        self.status = RecruitmentStatus::Accepted;
        Ok(())
    }

    /// 拒绝招募 — only legal from Pending.
    pub fn reject(&mut self) -> Result<(), String> {
        if self.status != RecruitmentStatus::Pending {
            return Err(format!(
                "cannot reject recruitment: current status is {:?}, expected Pending",
                self.status
            ));
        }
        self.status = RecruitmentStatus::Rejected;
        Ok(())
    }
}
