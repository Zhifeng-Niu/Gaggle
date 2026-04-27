//! Delegation — Agent 间的权限委托
//!
//! Agent A 可授权 Agent B 在特定 Space 中代表自己行动。
//! 通过 DelegationScope 控制权限范围，支持过期和撤销。

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 委托权限范围
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationScope {
    /// 全权谈判
    FullNegotiation,
    /// 只能提案
    ProposeOnly,
    /// 只能应答
    RespondOnly,
    /// 只能观察和建议（不能正式提案/应答）
    ObserveAndAdvise,
}

impl DelegationScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FullNegotiation => "full_negotiation",
            Self::ProposeOnly => "propose_only",
            Self::RespondOnly => "respond_only",
            Self::ObserveAndAdvise => "observe_and_advise",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "full_negotiation" => Self::FullNegotiation,
            "propose_only" => Self::ProposeOnly,
            "respond_only" => Self::RespondOnly,
            "observe_and_advise" => Self::ObserveAndAdvise,
            _ => Self::ObserveAndAdvise,
        }
    }

    /// 检查是否有提案权限
    pub fn can_propose(&self) -> bool {
        matches!(self, Self::FullNegotiation | Self::ProposeOnly)
    }

    /// 检查是否有应答权限
    pub fn can_respond(&self) -> bool {
        matches!(self, Self::FullNegotiation | Self::RespondOnly)
    }
}

/// 委托状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DelegationStatus {
    /// 活跃中
    Active,
    /// 已撤销
    Revoked,
    /// 已过期
    Expired,
}

impl DelegationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "revoked" => Self::Revoked,
            "expired" => Self::Expired,
            _ => Self::Active,
        }
    }
}

/// 委托结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    /// 唯一标识符
    pub id: String,
    /// 委托人 Agent ID
    pub delegator_id: String,
    /// 代理人 Agent ID
    pub delegate_id: String,
    /// 所属 Space ID
    pub space_id: String,
    /// 权限范围
    pub scope: DelegationScope,
    /// 过期时间（Unix 毫秒），None = 永不过期
    pub expires_at: Option<i64>,
    /// 状态
    pub status: DelegationStatus,
    /// 创建时间
    pub created_at: i64,
}

/// 创建委托请求
#[derive(Debug, Deserialize)]
pub struct CreateDelegationRequest {
    /// 代理人 Agent ID
    pub delegate_id: String,
    /// Space ID
    pub space_id: String,
    /// 权限范围
    pub scope: DelegationScope,
    /// 过期时间（可选）
    pub expires_at: Option<i64>,
}

impl Delegation {
    /// 创建新委托
    pub fn new(delegator_id: String, req: CreateDelegationRequest) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            delegator_id,
            delegate_id: req.delegate_id,
            space_id: req.space_id,
            scope: req.scope,
            expires_at: req.expires_at,
            status: DelegationStatus::Active,
            created_at: Utc::now().timestamp_millis(),
        }
    }

    /// 检查委托是否仍然有效
    pub fn is_valid(&self) -> bool {
        if self.status != DelegationStatus::Active {
            return false;
        }
        if let Some(expires) = self.expires_at {
            let now = Utc::now().timestamp_millis();
            return expires > now;
        }
        true
    }

    /// 撤销委托
    pub fn revoke(&mut self) {
        self.status = DelegationStatus::Revoked;
    }

    /// 检查过期
    pub fn check_expiry(&mut self) -> bool {
        if let Some(expires) = self.expires_at {
            if expires <= Utc::now().timestamp_millis() {
                self.status = DelegationStatus::Expired;
                return true;
            }
        }
        false
    }
}
