//! Discovery 类型定义

use serde::{Deserialize, Serialize};

/// 标准化 category 列表
pub const STANDARD_CATEGORIES: &[&str] = &[
    "supply_chain",     // 供应链
    "data_analysis",    // 数据分析
    "content_creation", // 内容创作
    "software_dev",     // 软件开发
    "marketing",        // 营销推广
    "finance",          // 金融服务
    "logistics",        // 物流仓储
    "manufacturing",    // 制造加工
    "consulting",       // 咨询服务
    "other",            // 其他
];

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
    /// 通用文本搜索（搜索 display_name, description, skills, category）
    pub query: Option<String>,
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
    /// 排序方式: "relevance" | "price_asc" | "price_desc" | "updated"
    #[serde(default)]
    pub sort_by: Option<String>,
    /// 页码（默认 1）
    #[serde(default)]
    pub page: Option<u32>,
    /// 每页数量（默认 20，最大 100）
    #[serde(default)]
    pub page_size: Option<u32>,
}

/// 带分页的搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResult<T> {
    pub items: Vec<T>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

// ── 需求广播（Need Broadcast）──────────────────────────

/// 需求状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NeedStatus {
    /// 开放中，接受响应
    Open,
    /// 已匹配，进入谈判
    Matched,
    /// 已过期
    Expired,
    /// 已取消
    Cancelled,
}

impl NeedStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            NeedStatus::Open => "open",
            NeedStatus::Matched => "matched",
            NeedStatus::Expired => "expired",
            NeedStatus::Cancelled => "cancelled",
        }
    }
}

/// 需求广播（Need）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Need {
    /// 唯一标识符
    pub id: String,
    /// 创建者 Agent ID（Consumer）
    pub creator_id: String,
    /// 需求标题
    pub title: String,
    /// 需求描述
    pub description: String,
    /// 标准 category
    pub category: String,
    /// 所需技能
    pub required_skills: Vec<String>,
    /// 预算下限
    pub budget_min: Option<f64>,
    /// 预算上限
    pub budget_max: Option<f64>,
    /// 截止时间（Unix timestamp 毫秒）
    pub deadline: Option<i64>,
    /// 当前状态
    pub status: NeedStatus,
    /// 创建时间
    pub created_at: i64,
    /// 更新时间
    pub updated_at: i64,
    /// 匹配到的 Provider 数量
    pub matched_provider_count: i32,
}

/// 发布需求请求
#[derive(Debug, Clone, Deserialize)]
pub struct PublishNeedRequest {
    pub title: String,
    pub description: String,
    pub category: String,
    #[serde(default)]
    pub required_skills: Vec<String>,
    pub budget_min: Option<f64>,
    pub budget_max: Option<f64>,
    pub deadline: Option<i64>,
}

/// 需求搜索查询
#[derive(Debug, Deserialize)]
pub struct NeedSearchQuery {
    pub category: Option<String>,
    pub skills: Option<String>,
    pub query: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_size: Option<u32>,
}

// ── 带信誉评分的搜索结果──────────────────────────

/// 带信誉评分的搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredDiscoveryProfile {
    pub profile: DiscoveryProfile,
    pub reputation_score: f64,
    pub capability_match: f64,
    pub final_score: f64,
}
