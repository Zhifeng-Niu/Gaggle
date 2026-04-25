//! Agent类型定义

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Agent类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Consumer,
    Provider,
}

/// Agent主体结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    /// 唯一标识符
    pub id: String,
    /// Agent类型
    pub agent_type: AgentType,
    /// 显示名称
    pub name: String,
    /// API Key（仅创建时返回一次）
    pub api_key: String,
    /// API Secret的Hash（用于鉴权）
    pub api_secret_hash: String,
    /// 可选：Solana公钥
    pub public_key: Option<String>,
    /// 扩展元数据
    pub metadata: JsonValue,
    /// 创建时间戳
    pub created_at: i64,
    /// 所属用户ID（None = legacy agent，向后兼容）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// 软删除时间戳（None = 正常）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_at: Option<i64>,
    /// 所属组织/公司
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    /// Webhook 回调地址（离线时唤醒 Agent）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
}

/// 注册请求
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub agent_type: AgentType,
    pub name: String,
    #[serde(default)]
    pub metadata: JsonValue,
    /// 可选：Solana公钥
    #[serde(default)]
    pub public_key: Option<String>,
    /// 所属组织/公司
    #[serde(default)]
    pub organization: Option<String>,
    /// Webhook 回调地址
    #[serde(default)]
    pub callback_url: Option<String>,
}

/// 注册响应（包含敏感信息，仅返回一次）
#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub id: String,
    pub agent_type: AgentType,
    pub name: String,
    pub api_key: String,
    pub api_secret: String,
    pub created_at: i64,
    pub organization: Option<String>,
}

/// Agent 更新请求
#[derive(Debug, Deserialize)]
pub struct UpdateAgentRequest {
    pub name: Option<String>,
    pub metadata: Option<JsonValue>,
    pub organization: Option<String>,
    pub callback_url: Option<String>,
}

/// Provider扩展信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderProfile {
    /// Agent ID
    pub agent_id: String,
    /// Team成员（如果是Agent Team）
    #[serde(default)]
    pub team: Vec<String>,
    /// 技能清单
    #[serde(default)]
    pub skills: Vec<String>,
    /// 定价模式
    #[serde(default)]
    pub pricing_model: PricingModel,
}

/// 定价模式
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PricingModel {
    /// 固定价格服务
    Fixed {
        #[serde(default)]
        services: Vec<PricedService>,
    },
    /// 需要谈判
    #[default]
    Negotiated,
    /// 自定义规则
    Custom(String),
}

/// 定价服务项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricedService {
    pub name: String,
    pub description: Option<String>,
    pub price: f64,
    pub currency: Option<String>,
}
