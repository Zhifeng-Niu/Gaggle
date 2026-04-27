//! 执行引擎类型定义

use serde::{Deserialize, Serialize};

/// 合同状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    /// 执行中
    Active,
    /// 全部里程碑完成
    Completed,
    /// 有争议
    Disputed,
    /// 已取消
    Cancelled,
    /// 已过期
    Expired,
}

impl ContractStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Disputed => "disputed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "completed" => Some(Self::Completed),
            "disputed" => Some(Self::Disputed),
            "cancelled" => Some(Self::Cancelled),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

/// 里程碑状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneStatus {
    /// 待交付
    Pending,
    /// Provider 已提交交付物
    Submitted,
    /// Consumer 已验收
    Accepted,
    /// Consumer 拒绝，需重新提交
    Rejected,
    /// 有争议
    Disputed,
}

impl MilestoneStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Submitted => "submitted",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Disputed => "disputed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "submitted" => Some(Self::Submitted),
            "accepted" => Some(Self::Accepted),
            "rejected" => Some(Self::Rejected),
            "disputed" => Some(Self::Disputed),
            _ => None,
        }
    }
}

/// 合同
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub id: String,
    pub space_id: String,
    pub buyer_id: String,
    pub seller_id: String,
    pub terms: serde_json::Value,
    pub milestones: Vec<Milestone>,
    pub status: ContractStatus,
    pub deadline: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 里程碑
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub id: String,
    pub contract_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: MilestoneStatus,
    pub deliverable_url: Option<String>,
    pub amount: Option<f64>,
    pub due_date: Option<i64>,
    pub submitted_at: Option<i64>,
    pub accepted_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 创建合同请求
#[derive(Debug, Deserialize)]
pub struct CreateContractRequest {
    pub milestones: Vec<CreateMilestoneRequest>,
}

/// 创建里程碑请求
#[derive(Debug, Clone, Deserialize)]
pub struct CreateMilestoneRequest {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub due_date: Option<i64>,
}

/// 提交里程碑交付物请求
#[derive(Debug, Deserialize)]
pub struct SubmitMilestoneRequest {
    pub deliverable_url: String,
}

/// 验收里程碑请求
#[derive(Debug, Deserialize)]
pub struct AcceptMilestoneRequest {
    /// true = 接受, false = 拒绝
    pub accepted: bool,
    #[serde(default)]
    pub comment: Option<String>,
}

/// 发起争议请求
#[derive(Debug, Deserialize)]
pub struct DisputeContractRequest {
    pub reason: String,
}
