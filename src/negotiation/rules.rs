//! SpaceRules — 统一规则引擎
//!
//! 替代 SpaceType 枚举，用可配置规则驱动 Space 行为。
//! 提供 bilateral() 和 rfp() 模板方法实现向后兼容。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::space::SpaceType;

// ── 枚举类型 ──────────────────────────────────────────

/// 消息可见性规则
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VisibilityRules {
    /// 所有成员看到所有消息
    All,
    /// Buyer 看所有消息，Provider 只看到自己的和 buyer 的
    BuyerSeesAll,
    /// 仅发送者和指定接收者看到消息（私密配对）
    PrivatePairs,
    /// 自定义可见性规则
    Custom(Vec<VisibilityRule>),
}

impl Default for VisibilityRules {
    fn default() -> Self {
        Self::All
    }
}

impl VisibilityRules {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::BuyerSeesAll => "buyer_sees_all",
            Self::PrivatePairs => "private_pairs",
            Self::Custom(_) => "custom",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "all" => Self::All,
            "buyer_sees_all" => Self::BuyerSeesAll,
            "private_pairs" => Self::PrivatePairs,
            _ => Self::All,
        }
    }
}

/// 自定义可见性规则
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisibilityRule {
    /// 源角色
    pub from_role: String,
    /// 目标角色列表
    pub to_roles: Vec<String>,
    /// 消息类型（None = 所有类型）
    pub message_type: Option<String>,
}

/// 成员锁定条件
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LockCondition {
    /// 第一份提案提交后锁定
    OnFirstProposal,
    /// Space 成交/取消时解锁
    OnConclude,
    /// 需要手动解锁（对方同意）
    Manual,
    /// 不锁定，随时可离开
    Never,
}

impl Default for LockCondition {
    fn default() -> Self {
        Self::Never
    }
}

impl LockCondition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OnFirstProposal => "on_first_proposal",
            Self::OnConclude => "on_conclude",
            Self::Manual => "manual",
            Self::Never => "never",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "on_first_proposal" => Self::OnFirstProposal,
            "on_conclude" => Self::OnConclude,
            "manual" => Self::Manual,
            "never" => Self::Never,
            _ => Self::Never,
        }
    }
}

/// 条款揭示模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RevealMode {
    /// 所有提案对所有成员可见
    Open,
    /// 只揭示最优条款（匿名）
    BestOnly,
    /// 封闭提交，直到截止才揭示
    Sealed,
    /// 渐进揭示（每轮揭示更多信息）
    Progressive,
}

impl Default for RevealMode {
    fn default() -> Self {
        Self::Open
    }
}

impl RevealMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::BestOnly => "best_only",
            Self::Sealed => "sealed",
            Self::Progressive => "progressive",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "open" => Self::Open,
            "best_only" => Self::BestOnly,
            "sealed" => Self::Sealed,
            "progressive" => Self::Progressive,
            _ => Self::Open,
        }
    }
}

/// 加入策略
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JoinPolicy {
    /// 仅受邀者可加入（默认）
    InviteOnly,
    /// 任何已注册 Agent 可直接加入
    Open,
    /// Agent 申请加入，创建者审批
    ApprovalRequired,
}

impl Default for JoinPolicy {
    fn default() -> Self {
        Self::InviteOnly
    }
}

impl JoinPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InviteOnly => "invite_only",
            Self::Open => "open",
            Self::ApprovalRequired => "approval_required",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "invite_only" => Self::InviteOnly,
            "open" => Self::Open,
            "approval_required" => Self::ApprovalRequired,
            _ => Self::InviteOnly,
        }
    }
}

// ── 结构体类型 ──────────────────────────────────────

/// 角色配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoleConfig {
    pub can_send_messages: bool,
    pub can_propose: bool,
    pub can_respond: bool,
    pub can_invite: bool,
    pub can_close: bool,
    pub can_evaluate: bool,
    /// Phase 13: 是否可修改空间规则
    #[serde(default)]
    pub can_change_rules: bool,
}

impl RoleConfig {
    /// Buyer 默认权限
    pub fn buyer() -> Self {
        Self {
            can_send_messages: true,
            can_propose: true,
            can_respond: true,
            can_invite: false,
            can_close: true,
            can_evaluate: true,
            can_change_rules: true,
        }
    }

    /// Provider/Seller 默认权限
    pub fn provider() -> Self {
        Self {
            can_send_messages: true,
            can_propose: true,
            can_respond: true,
            can_invite: false,
            can_close: false,
            can_evaluate: false,
            can_change_rules: false,
        }
    }

    /// Observer 默认权限（只读）
    pub fn observer() -> Self {
        Self {
            can_send_messages: false,
            can_propose: false,
            can_respond: false,
            can_invite: false,
            can_close: false,
            can_evaluate: false,
            can_change_rules: false,
        }
    }
}

/// 轮次管理配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoundConfig {
    /// 最大轮次
    pub max_rounds: u32,
    /// 轮次截止时间（Unix 时间戳毫秒）
    pub deadline: Option<i64>,
    /// 是否自动推进轮次
    pub auto_advance: bool,
    /// 评估标准列表
    pub evaluation_criteria: Option<Vec<String>>,
    /// 是否匿名分享最优条款
    pub share_best_terms: bool,
}

// ── Phase 13: 自适应规则 ───────────────────────────────

/// 规则变更触发条件
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleTrigger {
    /// Space 激活时触发
    OnSpaceActivated,
    /// 第一份提案提交时触发
    OnFirstProposal,
    /// 轮次推进到指定轮时触发
    OnRoundAdvance { round: u32 },
    /// 成员数量达到指定值时触发
    OnMemberCount { count: usize },
    /// 经过指定秒数后触发
    OnTimeElapsed { after_secs: u64 },
    /// 手动触发
    Manual,
}

/// 规则转换：定义触发条件 + 增量规则覆盖
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuleTransition {
    /// 触发条件
    pub trigger: RuleTrigger,
    /// 触发时应用的规则覆盖（增量更新，只改指定字段）
    pub rule_changes: SpaceRulesOverrides,
    /// 是否一次性（触发后从列表中移除）
    #[serde(default = "default_true")]
    pub one_shot: bool,
}

fn default_true() -> bool {
    true
}

// ── 核心规则结构 ──────────────────────────────────────

/// 空间规则配置 — 驱动 Space 的所有行为
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpaceRules {
    /// 消息可见性
    #[serde(default)]
    pub visibility: VisibilityRules,
    /// 哪些角色可以提交提案
    #[serde(default)]
    pub can_propose: Vec<String>,
    /// 成员锁定条件
    #[serde(default)]
    pub lock_condition: LockCondition,
    /// 条款揭示模式
    #[serde(default)]
    pub reveal_mode: RevealMode,
    /// 角色定义
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
    /// 轮次管理（None = 无轮次限制）
    pub rounds: Option<RoundConfig>,
    /// 最大参与者数量（None = 无限制）
    pub max_participants: Option<usize>,
    /// 加入策略
    #[serde(default)]
    pub join_policy: JoinPolicy,
    /// Phase 13: 规则演化计划
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<RuleTransition>,
}

impl Default for SpaceRules {
    fn default() -> Self {
        Self::bilateral()
    }
}

impl SpaceRules {
    /// Bilateral 默认规则（向后兼容 SpaceType::Bilateral）
    ///
    /// - 可见性: All（双方看到所有消息）
    /// - 提案: buyer 和 seller 都可提
    /// - 锁定: 成交前可离开
    /// - 揭示: Open
    /// - 角色: buyer + seller，权限基本对称
    /// - 无轮次管理
    pub fn bilateral() -> Self {
        let mut roles = HashMap::new();
        roles.insert("buyer".to_string(), RoleConfig::buyer());
        roles.insert("seller".to_string(), RoleConfig::provider());

        Self {
            visibility: VisibilityRules::All,
            can_propose: vec!["buyer".to_string(), "seller".to_string()],
            lock_condition: LockCondition::Never,
            reveal_mode: RevealMode::Open,
            roles,
            rounds: None,
            max_participants: Some(2),
            join_policy: JoinPolicy::InviteOnly,
            transitions: Vec::new(),
        }
    }

    /// RFP 默认规则（向后兼容 SpaceType::Rfp）
    ///
    /// - 可见性: All（所有 provider 看到 buyer 消息，彼此提案可见）
    /// - 提案: 仅 provider 可提
    /// - 锁定: 成交前可离开
    /// - 揭示: Open（可用 share_best_terms 切换为 BestOnly）
    /// - 角色: buyer + provider
    /// - 有轮次管理
    pub fn rfp() -> Self {
        let mut roles = HashMap::new();
        roles.insert("buyer".to_string(), RoleConfig::buyer());
        roles.insert(
            "provider".to_string(),
            RoleConfig {
                can_send_messages: true,
                can_propose: true,
                can_respond: true,
                can_invite: false,
                can_close: false,
                can_evaluate: false,
                can_change_rules: false,
            },
        );

        Self {
            visibility: VisibilityRules::All,
            can_propose: vec!["provider".to_string()],
            lock_condition: LockCondition::Never,
            reveal_mode: RevealMode::Open,
            roles,
            rounds: Some(RoundConfig {
                max_rounds: 3,
                deadline: None,
                auto_advance: false,
                evaluation_criteria: None,
                share_best_terms: false,
            }),
            max_participants: None,
            join_policy: JoinPolicy::InviteOnly,
            transitions: Vec::new(),
        }
    }

    /// 从 SpaceType 生成对应默认规则（数据库迁移用）
    pub fn from_space_type(space_type: &SpaceType) -> Self {
        match space_type {
            SpaceType::Bilateral => Self::bilateral(),
            SpaceType::Rfp => Self::rfp(),
        }
    }

    /// 从 SpaceType 字符串生成默认规则
    pub fn from_space_type_str(s: &str) -> Self {
        match s {
            "rfp" => Self::rfp(),
            _ => Self::bilateral(),
        }
    }

    /// 推导出等效的 SpaceType（向后兼容）
    pub fn derive_space_type(&self) -> SpaceType {
        if self.rounds.is_some() && self.can_propose.contains(&"provider".to_string()) {
            SpaceType::Rfp
        } else {
            SpaceType::Bilateral
        }
    }

    /// 检查指定角色是否有权限提案
    pub fn role_can_propose(&self, role: &str) -> bool {
        self.can_propose.contains(&role.to_string())
            || self
                .roles
                .get(role)
                .map(|rc| rc.can_propose)
                .unwrap_or(false)
    }

    /// 检查指定角色是否有权限关闭 Space
    pub fn role_can_close(&self, role: &str) -> bool {
        self.roles
            .get(role)
            .map(|rc| rc.can_close)
            .unwrap_or(false)
    }

    /// 检查指定角色是否有权限评估提案
    pub fn role_can_evaluate(&self, role: &str) -> bool {
        self.roles
            .get(role)
            .map(|rc| rc.can_evaluate)
            .unwrap_or(false)
    }

    /// 是否有轮次管理
    pub fn has_rounds(&self) -> bool {
        self.rounds.is_some()
    }

    /// 检查指定角色是否可修改规则
    pub fn role_can_change_rules(&self, role: &str) -> bool {
        self.roles
            .get(role)
            .map(|rc| rc.can_change_rules)
            .unwrap_or(false)
    }

    /// 检查 transitions 中是否有匹配触发条件的规则变更
    /// 返回需要应用的规则覆盖列表（已移除 one_shot 的项）
    pub fn check_transitions(&mut self, trigger: &RuleTrigger) -> Vec<SpaceRulesOverrides> {
        let mut matched = Vec::new();
        let mut i = 0;
        while i < self.transitions.len() {
            if &self.transitions[i].trigger == trigger {
                matched.push(self.transitions[i].rule_changes.clone());
                if self.transitions[i].one_shot {
                    self.transitions.remove(i);
                    continue;
                }
            }
            i += 1;
        }
        matched
    }

    /// 应用 RFP 上下文覆盖到规则（兼容旧 rfp_context 字段）
    pub fn apply_rfp_overrides(
        &mut self,
        allowed_rounds: Option<u32>,
        evaluation_criteria: Option<Vec<String>>,
        deadline: Option<i64>,
        share_best_terms: Option<bool>,
    ) {
        if let Some(rounds) = allowed_rounds {
            if let Some(ref mut rc) = self.rounds {
                rc.max_rounds = rounds;
            } else {
                self.rounds = Some(RoundConfig {
                    max_rounds: rounds,
                    deadline: None,
                    auto_advance: false,
                    evaluation_criteria: None,
                    share_best_terms: false,
                });
            }
        }
        if let Some(criteria) = evaluation_criteria {
            if let Some(ref mut rc) = self.rounds {
                rc.evaluation_criteria = Some(criteria);
            }
        }
        if let Some(dl) = deadline {
            if let Some(ref mut rc) = self.rounds {
                rc.deadline = Some(dl);
            }
        }
        if let Some(share) = share_best_terms {
            if share {
                self.reveal_mode = RevealMode::BestOnly;
            }
            if let Some(ref mut rc) = self.rounds {
                rc.share_best_terms = share;
            }
        }
    }
}

// ── 反序列化兼容 ────────────────────────────────────

/// 用于 API 请求中的可选 rules 字段
/// 如果未提供 rules，则根据请求类型选择默认模板
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpaceRulesOverrides {
    /// 覆盖可见性规则
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<VisibilityRules>,
    /// 覆盖提案权限
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_propose: Option<Vec<String>>,
    /// 覆盖锁定条件
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_condition: Option<LockCondition>,
    /// 覆盖揭示模式
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal_mode: Option<RevealMode>,
    /// 覆盖角色配置
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<HashMap<String, RoleConfig>>,
    /// 覆盖轮次配置
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounds: Option<Option<RoundConfig>>,
    /// 覆盖最大参与者
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_participants: Option<Option<usize>>,
    /// 覆盖加入策略
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_policy: Option<JoinPolicy>,
    /// 覆盖规则演化计划
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transitions: Option<Vec<RuleTransition>>,
}

impl SpaceRulesOverrides {
    /// 将覆盖项应用到基础规则
    pub fn apply_to(&self, base: &mut SpaceRules) {
        if let Some(ref v) = self.visibility {
            base.visibility = v.clone();
        }
        if let Some(ref cp) = self.can_propose {
            base.can_propose = cp.clone();
        }
        if let Some(ref lc) = self.lock_condition {
            base.lock_condition = lc.clone();
        }
        if let Some(ref rm) = self.reveal_mode {
            base.reveal_mode = rm.clone();
        }
        if let Some(ref r) = self.roles {
            base.roles = r.clone();
        }
        if let Some(ref rc) = self.rounds {
            base.rounds = rc.clone();
        }
        if let Some(ref mp) = self.max_participants {
            base.max_participants = mp.clone();
        }
        if let Some(ref jp) = self.join_policy {
            base.join_policy = jp.clone();
        }
        if let Some(ref t) = self.transitions {
            base.transitions = t.clone();
        }
    }
}
