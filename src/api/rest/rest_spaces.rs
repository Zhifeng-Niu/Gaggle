//! Space lifecycle endpoints — create, join, message, proposal, close, evaluate, rounds

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use crate::error::GaggleError;
use crate::negotiation::Space;
use crate::reputation::{EventType, Outcome, ReputationEvent};

use super::AppState;

// ── Space read endpoints ──

pub async fn get_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<Space>, GaggleError> {
    let agent_id = super::extract_agent_id(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent_id).await?;

    let space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or(GaggleError::SpaceNotFound(space_id))?;
    Ok(Json(space))
}

pub async fn list_agent_spaces(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<Vec<Space>>, GaggleError> {
    let caller_id = super::extract_agent_id(&state, &headers).await?;
    if caller_id != agent_id {
        return Err(GaggleError::Forbidden("Cannot access other agent's spaces".to_string()));
    }

    let spaces = state.space_manager.get_agent_spaces(&agent_id).await?;
    Ok(Json(spaces))
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    after: Option<i64>,
    limit: Option<u32>,
}

pub async fn get_space_messages(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Query(query): Query<MessagesQuery>,
) -> Result<Json<Vec<serde_json::Value>>, GaggleError> {
    let agent_id = super::extract_agent_id(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent_id).await?;

    let limit = query.limit.unwrap_or(200).min(1000);
    let messages = state
        .space_manager
        .get_messages(&space_id, query.after, limit)
        .await?;

    let space_opt = state.space_manager.get_space(&space_id).await.ok().flatten();

    let vals: Vec<serde_json::Value> = if let Some(space) = &space_opt {
        let rules = &space.rules;
        messages
            .into_iter()
            .filter(|msg| {
                crate::negotiation::VisibilityEngine::should_deliver_message(rules, msg, &agent_id, space)
            })
            .map(|msg| serde_json::to_value(&msg).unwrap_or_default())
            .collect()
    } else {
        messages
            .into_iter()
            .map(|msg| serde_json::to_value(&msg).unwrap_or_default())
            .collect()
    };
    Ok(Json(vals))
}

/// GET /spaces/:space_id/proposals
pub async fn get_space_proposals(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<Vec<crate::negotiation::Proposal>>, GaggleError> {
    let agent_id = super::extract_agent_id(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent_id).await?;

    let proposals = state.space_manager.get_space_proposals(&space_id).await?;
    Ok(Json(proposals))
}

/// GET /spaces/:space_id/members
pub async fn get_space_members(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent_id = super::extract_agent_id(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent_id).await?;

    let space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or(GaggleError::SpaceNotFound(space_id))?;

    Ok(Json(serde_json::json!({
        "space_id": space.id,
        "status": space.status,
        "agent_ids": space.agent_ids,
        "joined_agent_ids": space.joined_agent_ids,
        "creator_id": space.creator_id,
    })))
}

// ── Space write endpoints ──

#[derive(Debug, Deserialize)]
pub struct RestCreateSpaceRequest {
    pub name: String,
    #[serde(default)]
    pub invitee_ids: Vec<String>,
    pub context: serde_json::Value,
    #[serde(default)]
    pub rules: Option<crate::negotiation::rules::SpaceRulesOverrides>,
}

pub async fn rest_create_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RestCreateSpaceRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let create_req = crate::negotiation::CreateSpaceRequest {
        name: req.name,
        invitee_ids: req.invitee_ids.clone(),
        context: req.context,
    };
    let space = state
        .space_manager
        .create_space_with_rules(&agent, create_req, None, req.rules)
        .await?;
    super::super::ws::broadcast_space_created(&state, &agent, &space, &req.invitee_ids).await?;

    let _ = state.trace_store.log_action(
        &space.id,
        &agent.id,
        crate::api::trace::AuditAction::SpaceCreated,
        Some("space"),
        Some(&space.id),
        Some(serde_json::json!({"name": space.name, "type": format!("{:?}", space.space_type)})),
    ).await;

    Ok((StatusCode::CREATED, Json(serde_json::to_value(&space)?)))
}

/// POST /api/v1/spaces/rfp — 创建多方 RFP Space
#[derive(Debug, Deserialize)]
pub struct RestCreateRfpRequest {
    pub name: String,
    #[serde(default)]
    pub provider_ids: Vec<String>,
    pub allowed_rounds: Option<u32>,
    pub evaluation_criteria: Option<Vec<String>>,
    pub deadline: Option<i64>,
    pub share_best_terms: Option<bool>,
    #[serde(default)]
    pub context: serde_json::Value,
}

pub async fn rest_create_rfp(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RestCreateRfpRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let rfp_context = crate::negotiation::RfpContext {
        allowed_rounds: req.allowed_rounds,
        evaluation_criteria: req.evaluation_criteria,
        deadline: req.deadline,
        share_best_terms: req.share_best_terms,
    };
    let create_req = crate::negotiation::CreateRfpRequest {
        name: req.name,
        provider_ids: req.provider_ids.clone(),
        rfp_context,
        context: req.context,
    };
    let space = state.space_manager.create_rfp(&agent, create_req).await?;
    super::super::ws::broadcast_rfp_created(&state, &agent, &space, &req.provider_ids).await?;

    let _ = state.trace_store.log_action(
        &space.id,
        &agent.id,
        crate::api::trace::AuditAction::RfpCreated,
        Some("space"),
        Some(&space.id),
        Some(serde_json::json!({"name": space.name, "providers": req.provider_ids.len()})),
    ).await;

    Ok((StatusCode::CREATED, Json(serde_json::to_value(&space)?)))
}

/// POST /api/v1/spaces/:space_id/join
pub async fn rest_join_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let space = state.space_manager.join_space(&agent, &space_id).await?;
    super::super::ws::broadcast_space_joined(&state, &space, &agent.id).await?;

    let _ = state.trace_store.log_action(
        &space_id,
        &agent.id,
        crate::api::trace::AuditAction::SpaceJoined,
        Some("space"),
        Some(&space_id),
        Some(serde_json::json!({"agent_id": agent.id})),
    ).await;

    Ok(Json(serde_json::to_value(&space)?))
}

#[derive(Debug, Deserialize)]
pub struct RestJoinApproveRequest {
    pub agent_id: String,
}

pub async fn rest_join_approve(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestJoinApproveRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let approver = super::extract_agent(&state, &headers).await?;
    let space = state.space_manager.approve_join_request(&approver.id, &space_id, &req.agent_id).await?;
    super::super::ws::broadcast_space_joined(&state, &space, &req.agent_id).await?;
    Ok(Json(serde_json::to_value(&space)?))
}

#[derive(Debug, Deserialize)]
pub struct RestJoinRejectRequest {
    pub agent_id: String,
}

pub async fn rest_join_reject(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestJoinRejectRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let rejector = super::extract_agent(&state, &headers).await?;
    let space = state.space_manager.reject_join_request(&rejector.id, &space_id, &req.agent_id).await?;
    Ok(Json(serde_json::to_value(&space)?))
}

/// POST /api/v1/spaces/:space_id/leave
pub async fn rest_leave_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    state.space_manager.leave_space(&agent, &space_id).await?;
    Ok(Json(serde_json::json!({"status": "left", "space_id": space_id})))
}

// ── Rules ──

/// GET /api/v1/spaces/:space_id/rules
pub async fn rest_get_rules(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent_id = super::extract_agent_id(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent_id).await?;
    let space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or_else(|| GaggleError::SpaceNotFound(space_id))?;
    Ok(Json(serde_json::to_value(&space.rules)?))
}

/// PUT /api/v1/spaces/:space_id/rules — 更新规则（需权限）
pub async fn rest_update_rules(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(overrides): Json<crate::negotiation::rules::SpaceRulesOverrides>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let mut space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or_else(|| GaggleError::SpaceNotFound(space_id.clone()))?;

    let role = space.get_role(&agent.id).unwrap_or("member");
    if !space.rules.role_can_change_rules(role) {
        return Err(GaggleError::Forbidden(
            "Your role does not have permission to change rules".to_string(),
        ));
    }

    overrides.apply_to(&mut space.rules);
    space.updated_at = chrono::Utc::now().timestamp_millis();
    state.space_manager.persist_space(&space)?;
    state.space_manager.update_cache(&space).await;

    let broadcast_msg = serde_json::json!({
        "type": "rules_changed",
        "space_id": space_id,
        "payload": {
            "space_id": space_id,
            "rules": space.rules
        }
    });
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(serde_json::to_string(&broadcast_msg)?);
    }

    Ok(Json(serde_json::to_value(&space.rules)?))
}

/// GET /api/v1/spaces/:space_id/rules/transitions
pub async fn rest_get_transitions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent_id = super::extract_agent_id(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent_id).await?;
    let space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or_else(|| GaggleError::SpaceNotFound(space_id))?;
    Ok(Json(serde_json::to_value(&space.rules.transitions)?))
}

// ── Messages & Proposals ──

#[derive(Debug, Deserialize)]
pub struct RestSendMessageRequest {
    #[serde(default)]
    pub msg_type: Option<String>,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub proposal: Option<super::super::ws::InlineProposal>,
}

fn parse_msg_type(s: &str) -> crate::negotiation::MessageType {
    use crate::negotiation::message::MessageType;
    match s {
        "proposal" => MessageType::Proposal,
        "counter_proposal" => MessageType::CounterProposal,
        "acceptance" => MessageType::Acceptance,
        "rejection" => MessageType::Rejection,
        "withdrawal" => MessageType::Withdrawal,
        "system" => MessageType::System,
        "attachment" => MessageType::Attachment,
        _ => MessageType::Text,
    }
}

pub async fn rest_send_message(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestSendMessageRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    const MAX_CONTENT_LEN: usize = 32 * 1024;
    if req.content.len() > MAX_CONTENT_LEN {
        return Err(GaggleError::ValidationError(format!(
            "message content exceeds {} bytes (got {})",
            MAX_CONTENT_LEN, req.content.len()
        )));
    }

    let msg_type = parse_msg_type(req.msg_type.as_deref().unwrap_or("text"));
    let send_req = crate::negotiation::SendMessageRequest {
        msg_type: msg_type.clone(),
        content: req.content,
        metadata: req.metadata,
    };
    let message = state.space_manager.send_message(&agent, &space_id, send_req).await?;

    let mut proposal_obj: Option<crate::negotiation::Proposal> = None;
    if let Some(inline) = req.proposal {
        let ptype = match inline.proposal_type.as_deref() {
            Some("counter") => crate::negotiation::ProposalType::Counter,
            Some("best_and_final") => crate::negotiation::ProposalType::BestAndFinal,
            _ => crate::negotiation::ProposalType::Initial,
        };
        let prop_req = crate::negotiation::SubmitProposalRequest {
            proposal_type: ptype,
            dimensions: inline.dimensions,
            parent_proposal_id: inline.parent_proposal_id,
        };
        let proposal = state.space_manager.submit_proposal(&agent, &space_id, prop_req).await?;
        proposal_obj = Some(proposal);
    }

    if matches!(msg_type, crate::negotiation::MessageType::Acceptance | crate::negotiation::MessageType::Rejection) {
        if let Some(ref meta) = message.metadata {
            if let Some(pid) = meta.get("proposal_id").and_then(|v| v.as_str()) {
                let action = if matches!(msg_type, crate::negotiation::MessageType::Acceptance) {
                    crate::negotiation::ProposalResponseAction::Accept
                } else {
                    crate::negotiation::ProposalResponseAction::Reject
                };
                let resp_req = crate::negotiation::RespondToProposalRequest {
                    proposal_id: pid.to_string(),
                    action,
                    counter_dimensions: None,
                };
                let _ = state.space_manager.respond_to_proposal(&agent, &space_id, resp_req).await;
            }
        }
    }

    super::super::ws::broadcast_new_message_with_proposal(&state, &space_id, &message, proposal_obj.as_ref()).await?;

    if let Some(ref proposal) = proposal_obj {
        super::super::ws::broadcast_new_proposal(&state, &space_id, proposal).await?;
    }

    let _ = state.trace_store.log_action(
        &space_id,
        &agent.id,
        crate::api::trace::AuditAction::MessageSent,
        Some("message"),
        Some(&message.id),
        Some(serde_json::json!({"msg_type": format!("{:?}", msg_type), "content_len": message.content.len()})),
    ).await;

    Ok(Json(serde_json::to_value(&message)?))
}

#[derive(Debug, Deserialize)]
pub struct RestSubmitProposalRequest {
    pub proposal_type: String,
    pub dimensions: crate::negotiation::ProposalDimensions,
    #[serde(default)]
    pub parent_proposal_id: Option<String>,
}

pub async fn rest_submit_proposal(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestSubmitProposalRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let proposal_type = match req.proposal_type.as_str() {
        "counter" => crate::negotiation::ProposalType::Counter,
        "best_and_final" => crate::negotiation::ProposalType::BestAndFinal,
        _ => crate::negotiation::ProposalType::Initial,
    };
    let submit_req = crate::negotiation::SubmitProposalRequest {
        proposal_type,
        dimensions: req.dimensions,
        parent_proposal_id: req.parent_proposal_id,
    };
    let proposal = state.space_manager.submit_proposal(&agent, &space_id, submit_req).await?;
    super::super::ws::broadcast_new_proposal(&state, &space_id, &proposal).await?;

    let _ = state.trace_store.log_action(
        &space_id,
        &agent.id,
        crate::api::trace::AuditAction::ProposalSubmitted,
        Some("proposal"),
        Some(&proposal.id),
        Some(serde_json::json!({"proposal_type": format!("{:?}", proposal.proposal_type), "status": format!("{:?}", proposal.status)})),
    ).await;

    Ok(Json(serde_json::to_value(&proposal)?))
}

#[derive(Debug, Deserialize)]
pub struct RestRespondProposalRequest {
    pub action: String,
    #[serde(default)]
    pub counter_dimensions: Option<crate::negotiation::ProposalDimensions>,
}

pub async fn rest_respond_to_proposal(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((space_id, proposal_id)): Path<(String, String)>,
    Json(req): Json<RestRespondProposalRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let action = match req.action.as_str() {
        "accept" => crate::negotiation::ProposalResponseAction::Accept,
        "reject" => crate::negotiation::ProposalResponseAction::Reject,
        "counter" => crate::negotiation::ProposalResponseAction::Counter,
        _ => return Err(GaggleError::ValidationError(format!("Invalid action: {}", req.action))),
    };
    let respond_req = crate::negotiation::RespondToProposalRequest {
        proposal_id: proposal_id.clone(),
        action,
        counter_dimensions: req.counter_dimensions,
    };
    let (updated_proposal, counter_proposal) = state.space_manager.respond_to_proposal(&agent, &space_id, respond_req).await?;

    let update_msg = serde_json::to_string(&super::super::ws::WsOutgoing::ProposalUpdate {
        space_id: space_id.clone(),
        payload: super::super::ws::ProposalUpdatePayload {
            proposal_id: updated_proposal.id.clone(),
            status: updated_proposal.status.as_str().to_string(),
            action: "responded".to_string(),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(update_msg.clone());
    }

    if let Some(counter) = counter_proposal {
        let counter_msg = serde_json::to_string(&super::super::ws::WsOutgoing::NewProposal {
            space_id: space_id.clone(),
            payload: super::super::ws::NewProposalPayload { proposal: counter },
        })?;
        if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
            let _ = tx.send(counter_msg);
        }
    }

    let _ = state.trace_store.log_action(
        &space_id,
        &agent.id,
        crate::api::trace::AuditAction::ProposalResponded,
        Some("proposal"),
        Some(&proposal_id),
        Some(serde_json::json!({"action": req.action, "new_status": updated_proposal.status.as_str()})),
    ).await;

    Ok(Json(serde_json::to_value(&updated_proposal)?))
}

// ── Close & Delete ──

#[derive(Debug, Deserialize)]
pub struct RestCloseSpaceRequest {
    pub conclusion: String,
    #[serde(default)]
    pub final_terms: Option<serde_json::Value>,
}

pub async fn rest_close_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestCloseSpaceRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let conclusion = req.conclusion.clone();
    let final_terms = req.final_terms.clone();
    let close_req = crate::negotiation::CloseSpaceRequest {
        conclusion: req.conclusion,
        final_terms: req.final_terms,
    };
    let space = state.space_manager.close_space(&agent, &space_id, close_req).await?;
    super::super::ws::broadcast_space_closed(&state, &space_id, &conclusion).await?;

    let _ = state.trace_store.log_action(
        &space_id,
        &agent.id,
        crate::api::trace::AuditAction::SpaceClosed,
        Some("space"),
        Some(&space_id),
        Some(serde_json::json!({"conclusion": conclusion, "status": format!("{:?}", space.status)})),
    ).await;

    // Phase 4 自动化：concluded 时自动创建合同 + 证据上链 + 信誉记录
    let mut contract_info: Option<serde_json::Value> = None;

    if conclusion == "concluded" {
        let seller_id = space.seller_id.clone().unwrap_or_default();
        let buyer_id = space.buyer_id.clone().unwrap_or_default();

        // 1. 自动记录信誉事件
        if !seller_id.is_empty() && !buyer_id.is_empty() {
            let _ = state.reputation_store.create_event(ReputationEvent {
                id: format!("rev_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
                agent_id: seller_id.clone(),
                space_id: space_id.clone(),
                event_type: EventType::Concluded,
                outcome: Outcome::Success,
                rating: None,
                counterparty_id: buyer_id.clone(),
                created_at: chrono::Utc::now().timestamp(),
            }).await;
        }

        // 2. 自动证据上链
        #[cfg(feature = "solana")]
        {
            let messages = state.space_manager.get_messages(&space_id, None, 1000).await.unwrap_or_default();
            let session_hash = crate::solana::evidence::compute_session_hash(&messages);
            let _ = crate::solana::evidence::submit_evidence(
                &space_id,
                &crate::solana::evidence::EvidenceType::SessionHash,
                &session_hash,
            );
        }

        // 3. 如果 final_terms 包含 milestones，自动创建合同
        if let Some(ref terms) = final_terms {
            let milestones_json = terms.get("milestones");
            if let Some(milestones_arr) = milestones_json.and_then(|v| v.as_array()) {
                let milestone_reqs: Vec<crate::execution::CreateMilestoneRequest> = milestones_arr
                    .iter()
                    .filter_map(|m| {
                        Some(crate::execution::CreateMilestoneRequest {
                            title: m.get("title")?.as_str()?.to_string(),
                            description: m.get("description").and_then(|v| v.as_str()).map(String::from),
                            amount: m.get("amount").and_then(|v| v.as_f64()),
                            due_date: m.get("due_date").and_then(|v| v.as_i64()),
                        })
                    })
                    .collect();

                if !milestone_reqs.is_empty() {
                    let create_req = crate::execution::CreateContractRequest {
                        milestones: milestone_reqs,
                    };
                    match state.execution_store.create_contract(
                        &space_id,
                        &buyer_id,
                        &seller_id,
                        terms.clone(),
                        &create_req,
                        None,
                    ).await {
                        Ok(contract) => {
                            let cid = contract.id.clone();
                            contract_info = Some(serde_json::to_value(&contract).ok()).flatten();

                            let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::ContractCreated {
                                space_id: space_id.clone(),
                                payload: super::super::ws::ContractCreatedPayload {
                                    contract_id: cid,
                                    buyer_id: buyer_id.clone(),
                                    seller_id: seller_id.clone(),
                                    milestone_count: contract.milestones.len(),
                                },
                            }).unwrap_or_default();
                            if !broadcast_msg.is_empty() {
                                if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
                                    let _ = tx.send(broadcast_msg);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Auto-contract creation failed for space {}: {}", space_id, e);
                        }
                    }
                }
            }
        }

        // Phase 5: 自动提取市场价格数据
        if let Some(ref terms) = final_terms {
            let price = terms
                .get("price")
                .or_else(|| terms.get("total_amount"))
                .and_then(|v| v.as_f64());
            if let Some(price) = price {
                let category = space
                    .context
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("general");
                let _ = state
                    .marketplace_store
                    .record_price_from_space(&space_id, category, price)
                    .await;
            }
        }
    }

    let mut response = serde_json::to_value(&space)?;
    if let Some(ci) = contract_info {
        response["auto_contract"] = ci;
    }
    Ok(Json(response))
}

/// DELETE /api/v1/spaces/:space_id — 硬删除 Space（仅创建者）
pub async fn delete_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    state.space_manager.hard_delete_space(&agent, &space_id).await?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "deleted": true,
            "space_id": space_id,
        })),
    ))
}

// ── Phase 3: Evaluate & Rounds ──

#[derive(Debug, Deserialize)]
pub struct RestEvaluateRequest {
    #[serde(default)]
    pub weights: Option<crate::negotiation::EvaluationWeights>,
}

pub async fn rest_evaluate_proposals(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestEvaluateRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;

    let weights = req.weights.unwrap_or_default();
    let response = state
        .space_manager
        .evaluate_proposals(&space_id, &weights)
        .await?;

    Ok(Json(serde_json::to_value(&response)?))
}

pub async fn rest_get_rounds(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent_id = super::extract_agent_id(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent_id).await?;
    let round_info = state.space_manager.get_round_info(&space_id).await?;
    Ok(Json(serde_json::to_value(&round_info)?))
}

pub async fn rest_advance_round(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let round_info = state
        .space_manager
        .advance_round(&agent, &space_id)
        .await?;

    let msg = serde_json::to_string(&super::super::ws::WsOutgoing::RoundAdvanced {
        space_id: space_id.clone(),
        payload: super::super::ws::RoundAdvancedPayload {
            new_round: round_info.current_round,
            round_status: round_info.round_status.as_str().to_string(),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(msg);
    }

    Ok(Json(serde_json::to_value(&round_info)?))
}

// ── Need → RFP ──

#[derive(Debug, Deserialize)]
pub struct RestNeedToRfpRequest {
    pub provider_ids: Vec<String>,
    pub allowed_rounds: Option<u32>,
    pub evaluation_criteria: Option<Vec<String>>,
    pub deadline: Option<i64>,
    pub share_best_terms: Option<bool>,
}

pub async fn rest_create_rfp_from_need(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(need_id): Path<String>,
    Json(req): Json<RestNeedToRfpRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let need = state
        .discovery_store
        .get_need(&need_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Need not found: {}", need_id)))?;

    if need.creator_id != agent.id {
        return Err(GaggleError::Forbidden(
            "Only the need creator can create RFP".to_string(),
        ));
    }

    if need.status != crate::discovery::NeedStatus::Open {
        return Err(GaggleError::ValidationError(
            format!("Need is not open (status: {:?})", need.status),
        ));
    }

    let rfp_context = crate::negotiation::RfpContext {
        allowed_rounds: req.allowed_rounds,
        evaluation_criteria: req.evaluation_criteria,
        deadline: req.deadline,
        share_best_terms: req.share_best_terms,
    };

    let context = serde_json::json!({
        "need_id": need_id,
        "title": need.title,
        "description": need.description,
        "category": need.category,
        "required_skills": need.required_skills,
        "budget_min": need.budget_min,
        "budget_max": need.budget_max,
    });

    let create_req = crate::negotiation::CreateRfpRequest {
        name: format!("RFP: {}", need.title),
        provider_ids: req.provider_ids.clone(),
        rfp_context,
        context,
    };

    let space = state.space_manager.create_rfp(&agent, create_req).await?;

    state
        .discovery_store
        .update_need_status(&need_id, &crate::discovery::NeedStatus::Matched)
        .await?;

    super::super::ws::broadcast_rfp_created(&state, &agent, &space, &req.provider_ids).await?;

    Ok(Json(serde_json::json!({
        "space": space,
        "need_status": "matched",
    })))
}
