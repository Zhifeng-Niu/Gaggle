//! 市场价格信息中心类型定义

use serde::{Deserialize, Serialize};

/// 市场价格汇总
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketPrice {
    pub id: String,
    pub category: String,
    pub service_type: String,
    pub avg_price: f64,
    pub min_price: f64,
    pub max_price: f64,
    pub sample_count: i32,
    pub period: String, // "7d", "30d", "all"
    pub updated_at: i64,
}

/// 市场价格贡献
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketContribution {
    pub id: String,
    pub contributor_id: String,
    pub category: String,
    pub service_type: String,
    pub price: f64,
    pub description: Option<String>,
    pub anonymous: bool,
    pub created_at: i64,
}

/// 手动贡献价格请求
#[derive(Debug, Deserialize)]
pub struct SharePriceRequest {
    pub category: String,
    pub service_type: String,
    pub price: f64,
    pub description: Option<String>,
    #[serde(default)]
    pub anonymous: bool,
}
