//! Discovery 类型定义

use serde::{Deserialize, Serialize};

/// Provider Discovery Profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryProfile {
    /// Agent ID
    pub agent_id: String,
    /// 显示名称
    pub display_name: String,
    /// 描述
    pub description: Option<String>,
    /// 技能清单 JSON 数组
    pub skills: Vec<String>,
    /// 能力 JSON 对象
    pub capabilities: ProviderCapabilities,
    /// 定价模式
    pub pricing_model: PricingModel,
    /// 可用状态
    pub availability_status: AvailabilityStatus,
    /// 最低价格
    pub min_price: Option<f64>,
    /// 最高价格
    pub max_price: Option<f64>,
    /// 更新时间
    pub updated_at: i64,
}

/// Provider 能力
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// 分类
    pub category: String,
    /// 标签
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 定价模式
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PricingModel {
    Fixed,
    Negotiated,
    Custom(String),
    #[default]
    Unknown,
}

/// 可用状态
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityStatus {
    Available,
    Busy,
    Offline,
    #[default]
    Unknown,
}

/// 创建/更新 Discovery Profile 请求
#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub display_name: String,
    pub description: Option<String>,
    pub skills: Vec<String>,
    pub capabilities: ProviderCapabilities,
    pub pricing_model: PricingModel,
    pub availability_status: AvailabilityStatus,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
}

/// Provider 搜索结果（包含 Agent 基础信息）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSearchResult {
    /// Agent ID
    pub id: String,
    /// Agent 名称
    pub name: String,
    /// Discovery Profile
    pub profile: DiscoveryProfile,
}

/// Provider 搜索查询参数
#[derive(Debug, Deserialize)]
pub struct ProviderSearchQuery {
    /// 技能过滤（逗号分隔或单个）
    pub skills: Option<String>,
    /// 最低价格
    pub min_price: Option<f64>,
    /// 最高价格
    pub max_price: Option<f64>,
    /// 分类过滤
    pub category: Option<String>,
    /// 可用状态过滤
    pub availability: Option<String>,
}
