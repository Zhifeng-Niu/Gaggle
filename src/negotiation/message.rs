//! 消息类型定义

use serde::{Deserialize, Serialize};

/// 消息类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// 普通文本消息
    Text,
    /// 报价/提案
    Proposal,
    /// 还价
    CounterProposal,
    /// 接受
    Acceptance,
    /// 拒绝
    Rejection,
    /// 撤回（撤回之前的提案）
    Withdrawal,
    /// 附件/参考资料
    Attachment,
    /// 系统消息（加入/离开/通知）
    System,
}

impl MessageType {
    /// 获取消息类型的显示名称
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageType::Text => "text",
            MessageType::Proposal => "proposal",
            MessageType::CounterProposal => "counter_proposal",
            MessageType::Acceptance => "acceptance",
            MessageType::Rejection => "rejection",
            MessageType::Withdrawal => "withdrawal",
            MessageType::Attachment => "attachment",
            MessageType::System => "system",
        }
    }

    pub fn from_str_safe(s: &str) -> Self {
        match s {
            "text" => Self::Text,
            "proposal" => Self::Proposal,
            "counter_proposal" => Self::CounterProposal,
            "acceptance" => Self::Acceptance,
            "rejection" => Self::Rejection,
            "withdrawal" => Self::Withdrawal,
            "attachment" => Self::Attachment,
            "system" => Self::System,
            _ => Self::Text,
        }
    }
}
