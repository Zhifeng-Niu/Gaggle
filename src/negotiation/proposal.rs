//! 结构化提案模型 - RFP 谈判系统核心

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// 提案类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalType {
    /// 初始提案
    Initial,
    /// 反提案（还价）
    Counter,
    /// 最终报价
    BestAndFinal,
}

impl ProposalType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProposalType::Initial => "initial",
            ProposalType::Counter => "counter",
            ProposalType::BestAndFinal => "best_and_final",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "initial" => Self::Initial,
            "counter" => Self::Counter,
            "best_and_final" => Self::BestAndFinal,
            _ => Self::Initial,
        }
    }
}

/// 提案状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    /// 待处理
    Pending,
    /// 已接受
    Accepted,
    /// 已拒绝
    Rejected,
    /// 已被取代（有新的提案替换）
    Superseded,
}

impl ProposalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProposalStatus::Pending => "pending",
            ProposalStatus::Accepted => "accepted",
            ProposalStatus::Rejected => "rejected",
            ProposalStatus::Superseded => "superseded",
        }
    }

    /// Check whether transition from self to target is legal.
    ///
    /// Legal transitions:
    ///   Pending   → Accepted | Rejected | Superseded
    ///   Accepted  → (terminal)
    ///   Rejected  → (terminal)
    ///   Superseded → (terminal)
    pub fn can_transition_to(&self, target: &ProposalStatus) -> bool {
        matches!(
            (self, target),
            (ProposalStatus::Pending, ProposalStatus::Accepted)
                | (ProposalStatus::Pending, ProposalStatus::Rejected)
                | (ProposalStatus::Pending, ProposalStatus::Superseded)
        )
    }

    /// Returns true for terminal states that cannot transition further.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ProposalStatus::Accepted
                | ProposalStatus::Rejected
                | ProposalStatus::Superseded
        )
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "accepted" => Self::Accepted,
            "rejected" => Self::Rejected,
            "superseded" => Self::Superseded,
            _ => Self::Pending,
        }
    }
}

/// 提案维度 - 多维度报价
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProposalDimensions {
    /// 价格（USD 或其他单位）
    pub price: Option<f64>,
    /// 交付周期（天数）
    pub timeline_days: Option<f64>,
    /// 质量等级（1-5 分或 tier 名称）
    pub quality_tier: Option<String>,
    /// 其他条款（JSON 格式，灵活扩展）
    pub terms: Option<JsonValue>,
}

impl ProposalDimensions {
    /// 创建新的提案维度
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置价格
    pub fn with_price(mut self, price: f64) -> Self {
        self.price = Some(price);
        self
    }

    /// 设置交付周期
    pub fn with_timeline(mut self, days: f64) -> Self {
        self.timeline_days = Some(days);
        self
    }

    /// 设置质量等级
    pub fn with_quality(mut self, tier: impl Into<String>) -> Self {
        self.quality_tier = Some(tier.into());
        self
    }

    /// 设置其他条款
    pub fn with_terms(mut self, terms: JsonValue) -> Self {
        self.terms = Some(terms);
        self
    }
}

/// 结构化提案
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    /// 唯一标识符
    pub id: String,
    /// 所属 Space ID
    pub space_id: String,
    /// 提案者 Agent ID
    pub sender_id: String,
    /// 提案类型
    pub proposal_type: ProposalType,
    /// 提案维度
    pub dimensions: ProposalDimensions,
    /// 谈判轮次
    pub round: u32,
    /// 提案状态
    pub status: ProposalStatus,
    /// 父提案 ID（反提案时引用原提案）
    pub parent_proposal_id: Option<String>,
    /// 创建时间戳
    pub created_at: i64,
    /// 更新时间戳
    pub updated_at: i64,
}

impl Proposal {
    /// 创建新提案
    pub fn new(
        space_id: String,
        sender_id: String,
        proposal_type: ProposalType,
        dimensions: ProposalDimensions,
        round: u32,
        parent_proposal_id: Option<String>,
    ) -> Self {
        let now = Utc::now().timestamp_millis();

        Self {
            id: Uuid::new_v4().to_string(),
            space_id,
            sender_id,
            proposal_type,
            dimensions,
            round,
            status: ProposalStatus::Pending,
            parent_proposal_id,
            created_at: now,
            updated_at: now,
        }
    }

    /// 接受提案 — only legal from Pending.
    pub fn accept(&mut self) -> Result<(), String> {
        let target = ProposalStatus::Accepted;
        if !self.status.can_transition_to(&target) {
            return Err(format!(
                "cannot accept proposal: current status is {:?}, expected Pending",
                self.status
            ));
        }
        self.status = target;
        self.updated_at = Utc::now().timestamp_millis();
        Ok(())
    }

    /// 拒绝提案 — only legal from Pending.
    pub fn reject(&mut self) -> Result<(), String> {
        let target = ProposalStatus::Rejected;
        if !self.status.can_transition_to(&target) {
            return Err(format!(
                "cannot reject proposal: current status is {:?}, expected Pending",
                self.status
            ));
        }
        self.status = target;
        self.updated_at = Utc::now().timestamp_millis();
        Ok(())
    }

    /// 标记为已被取代 — only legal from Pending.
    pub fn supersede(&mut self) -> Result<(), String> {
        let target = ProposalStatus::Superseded;
        if !self.status.can_transition_to(&target) {
            return Err(format!(
                "cannot supersede proposal: current status is {:?}, expected Pending",
                self.status
            ));
        }
        self.status = target;
        self.updated_at = Utc::now().timestamp_millis();
        Ok(())
    }

    /// 检查提案是否待处理
    pub fn is_pending(&self) -> bool {
        self.status == ProposalStatus::Pending
    }

    /// 检查提案是否已结束（接受/拒绝/被取代）
    pub fn is_concluded(&self) -> bool {
        matches!(
            self.status,
            ProposalStatus::Accepted | ProposalStatus::Rejected | ProposalStatus::Superseded
        )
    }
}

/// 提交提案请求
#[derive(Debug, Deserialize)]
pub struct SubmitProposalRequest {
    pub proposal_type: ProposalType,
    pub dimensions: ProposalDimensions,
    pub parent_proposal_id: Option<String>,
}

/// 回复提案请求
#[derive(Debug, Deserialize)]
pub struct RespondToProposalRequest {
    pub proposal_id: String,
    pub action: ProposalResponseAction,
    pub counter_dimensions: Option<ProposalDimensions>,
}

/// 提案响应动作
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalResponseAction {
    /// 接受
    Accept,
    /// 拒绝
    Reject,
    /// 反提案
    Counter,
}

/// RFP 上下文 - 消费者定义的 RFP 参数
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RfpContext {
    /// 允许的最大轮次
    pub allowed_rounds: Option<u32>,
    /// 评估标准列表（如 ["price", "timeline", "quality"]）
    pub evaluation_criteria: Option<Vec<String>>,
    /// 截止时间（Unix 时间戳）
    pub deadline: Option<i64>,
    /// 是否分享最优条款（匿名）
    pub share_best_terms: Option<bool>,
}

impl RfpContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_rounds(mut self, rounds: u32) -> Self {
        self.allowed_rounds = Some(rounds);
        self
    }

    pub fn with_criteria(mut self, criteria: Vec<String>) -> Self {
        self.evaluation_criteria = Some(criteria);
        self
    }

    pub fn with_deadline(mut self, deadline: i64) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_share_best(mut self, share: bool) -> Self {
        self.share_best_terms = Some(share);
        self
    }
}

/// 创建 RFP 请求
#[derive(Debug, Deserialize)]
pub struct CreateRfpRequest {
    pub name: String,
    pub provider_ids: Vec<String>,
    pub rfp_context: RfpContext,
    #[serde(default)]
    pub context: JsonValue,
}

/// 分享最优条款请求
#[derive(Debug, Deserialize)]
pub struct ShareBestTermsRequest {
    /// 匿名最优条件（不透露具体 Provider）
    pub best_dimensions: ProposalDimensions,
}

/// 最优条款分享事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestTermsShared {
    pub space_id: String,
    pub best_dimensions: ProposalDimensions,
    pub shared_at: i64,
}

// ── 加权评估 ──────────────────────────────────────

/// 加权评估配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationWeights {
    /// 价格权重（0.0-1.0）
    pub price: f64,
    /// 交付周期权重（0.0-1.0）
    pub timeline: f64,
    /// 质量权重（0.0-1.0）
    pub quality: f64,
}

impl Default for EvaluationWeights {
    fn default() -> Self {
        Self {
            price: 0.4,
            timeline: 0.3,
            quality: 0.3,
        }
    }
}

impl EvaluationWeights {
    /// 验证权重总和是否接近 1.0
    pub fn is_valid(&self) -> bool {
        let sum = self.price + self.timeline + self.quality;
        (sum - 1.0).abs() < 0.01
    }
}

/// 单维度评分
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionScores {
    /// 价格评分（0.0-1.0，越低越好）
    pub price_score: f64,
    /// 周期评分（0.0-1.0，越短越好）
    pub timeline_score: f64,
    /// 质量评分（0.0-1.0）
    pub quality_score: f64,
}

/// 提案评分结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalScore {
    pub proposal_id: String,
    pub provider_id: String,
    pub weighted_score: f64,
    pub dimension_scores: DimensionScores,
}

/// 评估请求
#[derive(Debug, Deserialize)]
pub struct EvaluateRequest {
    #[serde(default)]
    pub weights: EvaluationWeights,
}

/// 评估响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateResponse {
    pub scores: Vec<ProposalScore>,
    pub sorted_by: String,
}

// ── 轮次状态 ──────────────────────────────────────

/// 轮次状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RoundStatus {
    /// 接受提案中
    Open,
    /// 轮次已关闭
    Closed,
    /// 轮次已过期
    Expired,
}

impl RoundStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoundStatus::Open => "open",
            RoundStatus::Closed => "closed",
            RoundStatus::Expired => "expired",
        }
    }
}

/// 轮次信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundInfo {
    pub current_round: u32,
    pub allowed_rounds: Option<u32>,
    pub round_status: RoundStatus,
    pub round_deadline: Option<i64>,
}

/// 质量等级 → 评分映射
pub fn quality_tier_score(tier: &str) -> f64 {
    match tier.to_lowercase().as_str() {
        "premium" | "5" | "5.0" => 1.0,
        "high" | "4" | "4.0" => 0.85,
        "standard" | "3" | "3.0" => 0.7,
        "basic" | "2" | "2.0" => 0.4,
        "economy" | "1" | "1.0" => 0.2,
        _ => 0.5, // 未知等级给中间分
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposal_status_legal_transitions() {
        // Pending → Accepted ✅
        assert!(ProposalStatus::Pending.can_transition_to(&ProposalStatus::Accepted));
        // Pending → Rejected ✅
        assert!(ProposalStatus::Pending.can_transition_to(&ProposalStatus::Rejected));
        // Pending → Superseded ✅
        assert!(ProposalStatus::Pending.can_transition_to(&ProposalStatus::Superseded));
    }

    #[test]
    fn test_proposal_status_illegal_transitions() {
        // Terminal states cannot transition to anything
        for terminal in &[ProposalStatus::Accepted, ProposalStatus::Rejected, ProposalStatus::Superseded] {
            for target in &[
                ProposalStatus::Pending, ProposalStatus::Accepted,
                ProposalStatus::Rejected, ProposalStatus::Superseded,
            ] {
                assert!(!terminal.can_transition_to(target),
                    "terminal {:?} should not transition to {:?}", terminal, target);
            }
        }
        // Pending → Pending (no-op) should fail
        assert!(!ProposalStatus::Pending.can_transition_to(&ProposalStatus::Pending));
    }

    #[test]
    fn test_proposal_accept_from_pending() {
        let mut p = Proposal {
            id: "p1".into(), space_id: "s1".into(), sender_id: "a1".into(),
            proposal_type: ProposalType::Initial, dimensions: Default::default(),
            round: 1, status: ProposalStatus::Pending, parent_proposal_id: None,
            created_at: 0, updated_at: 0,
        };
        assert!(p.accept().is_ok());
        assert_eq!(p.status, ProposalStatus::Accepted);
    }

    #[test]
    fn test_proposal_accept_from_accepted_fails() {
        let mut p = Proposal {
            id: "p1".into(), space_id: "s1".into(), sender_id: "a1".into(),
            proposal_type: ProposalType::Initial, dimensions: Default::default(),
            round: 1, status: ProposalStatus::Accepted, parent_proposal_id: None,
            created_at: 0, updated_at: 0,
        };
        assert!(p.accept().is_err());
        assert_eq!(p.status, ProposalStatus::Accepted); // unchanged
    }

    #[test]
    fn test_proposal_reject_from_pending() {
        let mut p = Proposal {
            id: "p1".into(), space_id: "s1".into(), sender_id: "a1".into(),
            proposal_type: ProposalType::Initial, dimensions: Default::default(),
            round: 1, status: ProposalStatus::Pending, parent_proposal_id: None,
            created_at: 0, updated_at: 0,
        };
        assert!(p.reject().is_ok());
        assert_eq!(p.status, ProposalStatus::Rejected);
    }

    #[test]
    fn test_proposal_supersede_from_rejected_fails() {
        let mut p = Proposal {
            id: "p1".into(), space_id: "s1".into(), sender_id: "a1".into(),
            proposal_type: ProposalType::Initial, dimensions: Default::default(),
            round: 1, status: ProposalStatus::Rejected, parent_proposal_id: None,
            created_at: 0, updated_at: 0,
        };
        assert!(p.supersede().is_err());
        assert_eq!(p.status, ProposalStatus::Rejected); // unchanged
    }

    #[test]
    fn test_proposal_is_terminal() {
        assert!(!ProposalStatus::Pending.is_terminal());
        assert!(ProposalStatus::Accepted.is_terminal());
        assert!(ProposalStatus::Rejected.is_terminal());
        assert!(ProposalStatus::Superseded.is_terminal());
    }
}
