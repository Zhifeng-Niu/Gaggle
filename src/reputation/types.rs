//! 信誉系统类型定义

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 信誉事件类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    /// 谈判达成
    Concluded,
    /// 谈判取消
    Cancelled,
    /// 违约
    Breach,
}

/// 事件结果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// 成功
    Success,
    /// 部分完成
    Partial,
    /// 失败
    Failure,
}

/// 信誉事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationEvent {
    pub id: String,
    pub agent_id: String,
    pub space_id: String,
    pub event_type: EventType,
    pub outcome: Outcome,
    pub rating: Option<i32>, // 1-5
    pub counterparty_id: String,
    pub created_at: i64,
}

/// 信誉摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationSummary {
    pub agent_id: String,
    pub total_negotiations: i32,
    pub successful: i32,
    pub avg_rating: Option<f64>,
    pub fulfillment_rate: f64,
    pub reputation_score: f64,
    pub last_updated: i64,
}

/// 创建信誉事件请求
#[derive(Debug, Deserialize)]
pub struct CreateEventRequest {
    pub agent_id: String,
    pub space_id: String,
    pub event_type: EventType,
    pub outcome: Outcome,
    pub rating: Option<i32>,
    pub counterparty_id: String,
}

/// 评分响应
#[derive(Debug, Serialize)]
pub struct RateResponse {
    pub event_id: String,
    pub agent_id: String,
    pub new_reputation_score: f64,
}

/// 信誉详情响应
#[derive(Debug, Serialize)]
pub struct ReputationDetail {
    pub summary: ReputationSummary,
    pub recent_events: Vec<ReputationEvent>,
}

impl ReputationEvent {
    pub fn new(
        agent_id: String,
        space_id: String,
        event_type: EventType,
        outcome: Outcome,
        rating: Option<i32>,
        counterparty_id: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            agent_id,
            space_id,
            event_type,
            outcome,
            rating,
            counterparty_id,
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    /// 验证评分范围
    pub fn validate_rating(rating: Option<i32>) -> Result<(), crate::error::GaggleError> {
        if let Some(r) = rating {
            if !(1..=5).contains(&r) {
                return Err(crate::error::GaggleError::ValidationError(
                    "Rating must be between 1 and 5".to_string(),
                ));
            }
        }
        Ok(())
    }
}
