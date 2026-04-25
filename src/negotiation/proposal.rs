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

    /// 接受提案
    pub fn accept(&mut self) {
        self.status = ProposalStatus::Accepted;
        self.updated_at = Utc::now().timestamp_millis();
    }

    /// 拒绝提案
    pub fn reject(&mut self) {
        self.status = ProposalStatus::Rejected;
        self.updated_at = Utc::now().timestamp_millis();
    }

    /// 标记为已被取代
    pub fn supersede(&mut self) {
        self.status = ProposalStatus::Superseded;
        self.updated_at = Utc::now().timestamp_millis();
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
