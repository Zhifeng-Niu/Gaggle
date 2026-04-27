//! Visibility Engine — 按规则过滤消息投递
//!
//! Phase 7: 根据 SpaceRules.visibility 决定哪些 agent 应收到哪些消息。
//! 核心原则："过程私密，结果公开" — 消息按 visibility 过滤，但状态变更始终广播。

use crate::negotiation::rules::{SpaceRules, VisibilityRules};
use crate::negotiation::space::{Space, SpaceMessage};

/// 可见性引擎 — 无状态，纯函数判断
pub struct VisibilityEngine;

impl VisibilityEngine {
    /// 判断一条消息是否应对指定 agent 投递
    ///
    /// 参数:
    /// - rules: 当前 Space 的规则
    /// - message: 待投递消息
    /// - recipient_id: 目标接收者
    /// - space: 所属 Space（用于查 buyer/seller 角色）
    pub fn should_deliver_message(
        rules: &SpaceRules,
        message: &SpaceMessage,
        recipient_id: &str,
        space: &Space,
    ) -> bool {
        // 发送者始终能看到自己的消息
        if message.sender_id == recipient_id {
            return true;
        }

        match &rules.visibility {
            VisibilityRules::All => {
                // 所有成员看到所有消息（当前默认行为）
                true
            }
            VisibilityRules::BuyerSeesAll => {
                // Buyer 看所有消息
                // Provider/Seller 只看到自己的 + buyer 发的
                let recipient_role = space.get_role(recipient_id);
                match recipient_role {
                    Some("buyer") => true,
                    _ => {
                        // 非 buyer 只看到 buyer 发的消息和自己的
                        space.buyer_id.as_deref() == Some(&message.sender_id)
                    }
                }
            }
            VisibilityRules::PrivatePairs => {
                // 仅 sender + recipient_ids 收到
                message.recipient_ids.contains(&recipient_id.to_string())
            }
            VisibilityRules::Custom(rules_list) => {
                // 按自定义规则执行
                let sender_role = space.get_role(&message.sender_id).unwrap_or("member");
                let recipient_role = space.get_role(recipient_id).unwrap_or("member");

                for rule in rules_list {
                    if rule.from_role == sender_role {
                        // 检查消息类型过滤
                        if let Some(ref msg_type) = rule.message_type {
                            if message.msg_type.as_str() != msg_type.as_str() {
                                continue;
                            }
                        }
                        if rule.to_roles.contains(&recipient_role.to_string()) {
                            return true;
                        }
                    }
                }
                false
            }
        }
    }

    /// 判断一条消息是否应对指定 agent 投递（从 WS broadcast JSON 反序列化）
    ///
    /// 此方法用于 WS 端的 broadcast channel 接收过滤，
    /// 避免在广播路径上需要完整反序列化。
    pub fn should_deliver_json(
        rules: &SpaceRules,
        sender_id: &str,
        recipient_ids: &[String],
        recipient_id: &str,
        space: &Space,
    ) -> bool {
        if sender_id == recipient_id {
            return true;
        }

        match &rules.visibility {
            VisibilityRules::All => true,
            VisibilityRules::BuyerSeesAll => {
                let recipient_role = space.get_role(recipient_id);
                match recipient_role {
                    Some("buyer") => true,
                    _ => space.buyer_id.as_deref() == Some(sender_id),
                }
            }
            VisibilityRules::PrivatePairs => {
                recipient_ids.contains(&recipient_id.to_string())
            }
            VisibilityRules::Custom(_) => {
                // Custom 需要完整消息才能判断，默认允许投递
                // 实际过滤在 REST 查询时进行
                true
            }
        }
    }

    /// 获取应收到消息的 agent 列表
    pub fn get_recipients(
        rules: &SpaceRules,
        message: &SpaceMessage,
        space: &Space,
    ) -> Vec<String> {
        space
            .joined_agent_ids
            .iter()
            .filter(|id| Self::should_deliver_message(rules, message, id, space))
            .cloned()
            .collect()
    }

    /// 判断一个事件是否为"状态广播"（不受 visibility 限制）
    ///
    /// 状态广播事件始终对所有成员可见：
    /// - proposal_update (accepted/rejected)
    /// - best_terms_shared
    /// - space_status_changed
    /// - space_closed
    /// - round_advanced
    pub fn is_state_broadcast(event_type: &str) -> bool {
        matches!(
            event_type,
            "proposal_update"
                | "best_terms_shared"
                | "space_status_changed"
                | "space_closed"
                | "space_joined"
                | "space_left"
                | "round_advanced"
                | "rules_changed"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::negotiation::message::MessageType;
    use crate::negotiation::space::{MessageVisibility, SpaceStatus};

    fn make_test_space(buyer_id: &str, agent_ids: Vec<&str>) -> Space {
        Space {
            id: "test-space".to_string(),
            name: "Test".to_string(),
            creator_id: buyer_id.to_string(),
            agent_ids: agent_ids.iter().map(|s| s.to_string()).collect(),
            joined_agent_ids: agent_ids.iter().map(|s| s.to_string()).collect(),
            status: SpaceStatus::Active,
            space_type: crate::negotiation::space::SpaceType::Rfp,
            rules: SpaceRules::rfp(),
            rfp_context: None,
            context: serde_json::json!({}),
            encryption_key: "test".to_string(),
            created_at: 0,
            updated_at: 0,
            closed_at: None,
            buyer_id: Some(buyer_id.to_string()),
            seller_id: None,
            pending_join_requests: Vec::new(),
        }
    }

    fn make_message(sender: &str, recipients: Vec<&str>) -> SpaceMessage {
        SpaceMessage {
            id: "msg-1".to_string(),
            space_id: "test-space".to_string(),
            sender_id: sender.to_string(),
            msg_type: MessageType::Text,
            content: "hello".to_string(),
            timestamp: 0,
            round: 1,
            metadata: None,
            visibility: if recipients.is_empty() {
                MessageVisibility::Broadcast
            } else {
                MessageVisibility::Directed
            },
            recipient_ids: recipients.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn test_visibility_all() {
        let rules = SpaceRules::bilateral(); // visibility = All
        let space = make_test_space("buyer1", vec!["buyer1", "seller1"]);

        let msg = make_message("buyer1", vec![]);
        assert!(VisibilityEngine::should_deliver_message(&rules, &msg, "buyer1", &space));
        assert!(VisibilityEngine::should_deliver_message(&rules, &msg, "seller1", &space));
    }

    #[test]
    fn test_visibility_buyer_sees_all() {
        let mut rules = SpaceRules::rfp();
        rules.visibility = VisibilityRules::BuyerSeesAll;
        let space = make_test_space("buyer1", vec!["buyer1", "prov1", "prov2"]);

        // Buyer sees everything
        let msg_from_prov1 = make_message("prov1", vec![]);
        assert!(VisibilityEngine::should_deliver_message(&rules, &msg_from_prov1, "buyer1", &space));

        // prov2 does NOT see prov1's message
        assert!(!VisibilityEngine::should_deliver_message(&rules, &msg_from_prov1, "prov2", &space));

        // prov1 sees own message
        assert!(VisibilityEngine::should_deliver_message(&rules, &msg_from_prov1, "prov1", &space));

        // prov1 sees buyer's message
        let msg_from_buyer = make_message("buyer1", vec![]);
        assert!(VisibilityEngine::should_deliver_message(&rules, &msg_from_buyer, "prov1", &space));
        assert!(VisibilityEngine::should_deliver_message(&rules, &msg_from_buyer, "prov2", &space));
    }

    #[test]
    fn test_visibility_private_pairs() {
        let mut rules = SpaceRules::rfp();
        rules.visibility = VisibilityRules::PrivatePairs;
        let space = make_test_space("buyer1", vec!["buyer1", "prov1", "prov2"]);

        // Directed message buyer → prov1 only
        let msg = make_message("buyer1", vec!["prov1"]);
        assert!(VisibilityEngine::should_deliver_message(&rules, &msg, "buyer1", &space)); // sender
        assert!(VisibilityEngine::should_deliver_message(&rules, &msg, "prov1", &space)); // recipient
        assert!(!VisibilityEngine::should_deliver_message(&rules, &msg, "prov2", &space)); // not recipient
    }

    #[test]
    fn test_state_broadcast_always_visible() {
        assert!(VisibilityEngine::is_state_broadcast("proposal_update"));
        assert!(VisibilityEngine::is_state_broadcast("best_terms_shared"));
        assert!(VisibilityEngine::is_state_broadcast("space_closed"));
        assert!(!VisibilityEngine::is_state_broadcast("new_message"));
    }
}
