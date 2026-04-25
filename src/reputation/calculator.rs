//! 信誉评分计算器

use super::store::ReputationStore;
use super::types::{Outcome, ReputationEvent, ReputationSummary};
use crate::error::GaggleError;
use chrono::Utc;
use std::sync::Arc;

/// 30天时间窗口（秒）
const RECENCY_WINDOW_DAYS: i64 = 30;
const RECENCY_WINDOW_SECONDS: i64 = RECENCY_WINDOW_DAYS * 24 * 60 * 60;

pub struct ReputationCalculator {
    store: Arc<ReputationStore>,
}

impl ReputationCalculator {
    pub fn new(store: Arc<ReputationStore>) -> Self {
        Self { store }
    }

    /// 重新计算并更新指定 Agent 的信誉摘要
    pub async fn recalculate(&self, agent_id: &str) -> Result<ReputationSummary, GaggleError> {
        let events: Vec<ReputationEvent> = self.store.get_all_agent_events(agent_id).await?;

        if events.is_empty() {
            // 没有事件时，返回默认摘要
            let summary = ReputationSummary {
                agent_id: agent_id.to_string(),
                total_negotiations: 0,
                successful: 0,
                avg_rating: None,
                fulfillment_rate: 0.0,
                reputation_score: 0.0,
                last_updated: Utc::now().timestamp(),
            };
            self.store.upsert_summary(&summary).await?;
            return Ok(summary);
        }

        // 计算各项指标
        let total = events.len() as i32;
        let successful = events
            .iter()
            .filter(|e| e.outcome == Outcome::Success)
            .count() as i32;

        // 成功率 = successful / total
        let success_rate = if total > 0 {
            successful as f64 / total as f64
        } else {
            0.0
        };

        // 履约率 = (concluded + partial) / total
        let fulfillment_count = events
            .iter()
            .filter(|e| e.event_type != super::types::EventType::Breach)
            .count() as i32;
        let fulfillment_rate = if total > 0 {
            fulfillment_count as f64 / total as f64
        } else {
            0.0
        };

        // 平均评分 (仅统计有评分的事件)
        let ratings: Vec<i32> = events.iter().filter_map(|e| e.rating).collect();
        let avg_rating = if !ratings.is_empty() {
            let sum: i32 = ratings.iter().sum();
            Some(sum as f64 / ratings.len() as f64)
        } else {
            None
        };

        // 计算最近性权重
        let now = Utc::now().timestamp();
        let recency_weight = self.calculate_recency_weight(&events, now);

        // 综合评分公式：
        // score = (0.40 * fulfillment_rate + 0.25 * (avg_rating/5.0) + 0.20 * success_rate + 0.15 * recency_weight) * 100
        let avg_rating_normalized = avg_rating.unwrap_or(0.0) / 5.0;
        let score = (0.40 * fulfillment_rate
            + 0.25 * avg_rating_normalized
            + 0.20 * success_rate
            + 0.15 * recency_weight)
            * 100.0;

        // 限制在 0-100 范围内
        let score = score.clamp(0.0, 100.0);

        let summary = ReputationSummary {
            agent_id: agent_id.to_string(),
            total_negotiations: total,
            successful,
            avg_rating,
            fulfillment_rate,
            reputation_score: score,
            last_updated: now,
        };

        self.store.upsert_summary(&summary).await?;
        Ok(summary)
    }

    /// 计算最近性权重
    /// 最近30天的事件权重更高，线性衰减
    /// 最新事件权重为 1.0，30天前的事件权重为 0.0
    fn calculate_recency_weight(&self, events: &[ReputationEvent], now: i64) -> f64 {
        if events.is_empty() {
            return 0.0;
        }

        // 获取最近30天内的事件
        let cutoff = now - RECENCY_WINDOW_SECONDS;
        let recent_events: Vec<&ReputationEvent> =
            events.iter().filter(|e| e.created_at >= cutoff).collect();

        if recent_events.is_empty() {
            return 0.0;
        }

        // 计算加权平均
        let mut total_weight = 0.0;
        let mut sum_weighted_outcomes = 0.0;

        for event in recent_events {
            // 计算时间权重：越新权重越高
            let days_ago = ((now - event.created_at) as f64) / (24.0 * 60.0 * 60.0);
            let time_weight = 1.0 - (days_ago / RECENCY_WINDOW_DAYS as f64);
            let time_weight = time_weight.max(0.0);

            // 结果权重：成功 > 部分 > 失败
            let outcome_weight = match event.outcome {
                Outcome::Success => 1.0,
                Outcome::Partial => 0.5,
                Outcome::Failure => 0.0,
            };

            let combined_weight = time_weight * outcome_weight;
            total_weight += combined_weight;
            sum_weighted_outcomes += combined_weight * outcome_weight;
        }

        if total_weight > 0.0 {
            sum_weighted_outcomes / total_weight
        } else {
            0.0
        }
    }

    /// 获取 Agent 的信誉详情（摘要 + 最近事件）
    pub async fn get_detail(
        &self,
        agent_id: &str,
        recent_limit: usize,
    ) -> Result<(ReputationSummary, Vec<ReputationEvent>), GaggleError> {
        // 尝试获取现有摘要
        let summary = match self.store.get_summary(agent_id).await? {
            Some(s) => s,
            None => {
                // 如果没有摘要，重新计算
                self.recalculate(agent_id).await?
            }
        };

        let recent_events = self.store.get_agent_events(agent_id, recent_limit).await?;

        Ok((summary, recent_events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recency_weight_calculation() {
        let calculator = ReputationCalculator {
            store: Arc::new(ReputationStore::new(":memory:").unwrap()),
        };

        let now = Utc::now().timestamp();

        // 测试：今天的事件应该有接近 1.0 的权重
        let recent_event = ReputationEvent {
            id: "1".to_string(),
            agent_id: "agent1".to_string(),
            space_id: "space1".to_string(),
            event_type: super::types::EventType::Concluded,
            outcome: Outcome::Success,
            rating: Some(5),
            counterparty_id: "other".to_string(),
            created_at: now,
        };

        let weight = calculator.calculate_recency_weight(&[recent_event], now);
        assert!(weight > 0.9, "Recent event should have high weight");

        // 测试：30天前的事件应该有接近 0.0 的权重
        let old_event = ReputationEvent {
            created_at: now - RECENCY_WINDOW_SECONDS,
            ..recent_event
        };

        let weight = calculator.calculate_recency_weight(&[old_event], now);
        assert!(weight < 0.1, "Old event should have low weight");
    }
}
