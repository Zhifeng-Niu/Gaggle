//! Institutional endpoints — Contracts, SubSpaces, Coalitions, Delegations,
//! Recruitment, Shared Reality State, Trace/Observability, Queue Stats

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;

use crate::error::GaggleError;
use crate::negotiation::SetStateRequest;

use super::AppState;

// ══════════════════════════════════════════════════
// Phase 4: Execution — Contracts & Milestones
// ══════════════════════════════════════════════════

pub async fn rest_create_contract(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<crate::execution::CreateContractRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or_else(|| GaggleError::SpaceNotFound(space_id.clone()))?;

    if space.buyer_id.as_deref() != Some(&agent.id) && space.seller_id.as_deref() != Some(&agent.id) {
        return Err(GaggleError::Forbidden("Not a member of this space".to_string()));
    }

    if let Some(existing) = state.execution_store.get_contract_by_space(&space_id).await? {
        return Err(GaggleError::ValidationError(
            format!("Contract already exists for this space: {}", existing.id),
        ));
    }

    let terms = serde_json::json!({});
    let contract = state
        .execution_store
        .create_contract(
            &space_id,
            space.buyer_id.as_deref().unwrap_or(""),
            space.seller_id.as_deref().unwrap_or(""),
            terms,
            &req,
            None,
        )
        .await?;

    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::ContractCreated {
        space_id: space_id.clone(),
        payload: super::super::ws::ContractCreatedPayload {
            contract_id: contract.id.clone(),
            buyer_id: contract.buyer_id.clone(),
            seller_id: contract.seller_id.clone(),
            milestone_count: contract.milestones.len(),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&contract)?))
}

pub async fn rest_get_contract(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(contract_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let contract = state
        .execution_store
        .get_contract(&contract_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Contract not found: {}", contract_id)))?;
    if contract.buyer_id != agent.id && contract.seller_id != agent.id {
        return Err(GaggleError::Forbidden("Not a party to this contract".to_string()));
    }
    Ok(Json(serde_json::to_value(&contract)?))
}

pub async fn rest_get_agent_contracts(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    if agent.id != agent_id {
        return Err(GaggleError::Forbidden("Can only view own contracts".to_string()));
    }
    let contracts = state.execution_store.get_agent_contracts(&agent_id).await?;
    Ok(Json(serde_json::to_value(&contracts)?))
}

pub async fn rest_submit_milestone(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((contract_id, milestone_id)): Path<(String, String)>,
    Json(req): Json<crate::execution::SubmitMilestoneRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let contract = state
        .execution_store
        .get_contract(&contract_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Contract not found: {}", contract_id)))?;

    if contract.seller_id != agent.id {
        return Err(GaggleError::Forbidden("Only the seller can submit milestones".to_string()));
    }

    let milestone = state
        .execution_store
        .submit_milestone(&milestone_id, &req.deliverable_url)
        .await?;

    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::MilestoneSubmitted {
        contract_id: contract_id.clone(),
        payload: super::super::ws::MilestoneSubmittedPayload {
            milestone_id: milestone.id.clone(),
            milestone_title: milestone.title.clone(),
            deliverable_url: milestone.deliverable_url.clone().unwrap_or_default(),
            submitted_at: milestone.submitted_at.unwrap_or(0),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&contract.space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&milestone)?))
}

pub async fn rest_accept_milestone(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((contract_id, milestone_id)): Path<(String, String)>,
    Json(req): Json<crate::execution::AcceptMilestoneRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let contract = state
        .execution_store
        .get_contract(&contract_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Contract not found: {}", contract_id)))?;

    if contract.buyer_id != agent.id {
        return Err(GaggleError::Forbidden("Only the buyer can accept milestones".to_string()));
    }

    let milestone = state
        .execution_store
        .accept_milestone(&milestone_id, req.accepted)
        .await?;

    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::MilestoneAccepted {
        contract_id: contract_id.clone(),
        payload: super::super::ws::MilestoneAcceptedPayload {
            milestone_id: milestone.id.clone(),
            milestone_title: milestone.title.clone(),
            accepted: req.accepted,
            accepted_at: milestone.accepted_at.unwrap_or(0),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&contract.space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    let updated_contract = state.execution_store.get_contract(&contract_id).await?;
    if let Some(ref c) = updated_contract {
        if c.status == crate::execution::ContractStatus::Completed {
            let completed_msg = serde_json::to_string(&super::super::ws::WsOutgoing::ContractCompleted {
                contract_id: contract_id.clone(),
            })?;
            if let Some(tx) = state.space_manager.get_broadcast_tx(&c.space_id).await {
                let _ = tx.send(completed_msg);
            }
        }
    }

    Ok(Json(serde_json::to_value(&milestone)?))
}

pub async fn rest_dispute_contract(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(contract_id): Path<String>,
    Json(_req): Json<crate::execution::DisputeContractRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let contract = state
        .execution_store
        .get_contract(&contract_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Contract not found: {}", contract_id)))?;

    if contract.buyer_id != agent.id && contract.seller_id != agent.id {
        return Err(GaggleError::Forbidden("Not a party to this contract".to_string()));
    }

    let updated = state.execution_store.dispute_contract(&contract_id).await?;
    Ok(Json(serde_json::to_value(&updated)?))
}

// ══════════════════════════════════════════════════
// Phase 9: SubSpace
// ══════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct RestCreateSubSpaceRequest {
    pub name: String,
    #[serde(default)]
    pub agent_ids: Vec<String>,
    pub rules: Option<crate::negotiation::SpaceRulesOverrides>,
}

pub async fn rest_create_subspace(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestCreateSubSpaceRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let sub = state.space_manager.create_subspace(
        &space_id,
        &agent.id,
        crate::negotiation::subspace::CreateSubSpaceRequest {
            name: req.name,
            agent_ids: req.agent_ids,
            rules: req.rules,
        },
    ).await?;

    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::SubSpaceCreated {
        parent_space_id: space_id.clone(),
        sub_space: sub.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&sub)?))
}

pub async fn rest_list_subspaces(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;
    let subs = state.space_manager.list_subspaces(&space_id).await?;
    Ok(Json(serde_json::json!({ "subspaces": subs })))
}

pub async fn rest_get_subspace(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;
    let sub = state.space_manager.get_subspace(&sub_space_id).await?
        .ok_or_else(|| GaggleError::NotFound(format!("Sub-space not found: {}", sub_space_id)))?;
    Ok(Json(serde_json::to_value(&sub)?))
}

#[derive(Debug, Deserialize)]
pub struct RestSubSpaceMessageRequest {
    pub msg_type: crate::negotiation::MessageType,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

pub async fn rest_subspace_send_message(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
    Json(req): Json<RestSubSpaceMessageRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let msg = state.space_manager.send_subspace_message(
        &sub_space_id,
        &agent.id,
        req.msg_type,
        &req.content,
        req.metadata,
    ).await?;

    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::SubSpaceMessage {
        sub_space_id: sub_space_id.clone(),
        message: msg.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_subspace_broadcast_tx(&sub_space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&msg)?))
}

pub async fn rest_get_subspace_messages(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;
    let messages = state.space_manager.get_subspace_messages(&sub_space_id).await?;
    Ok(Json(serde_json::json!({ "messages": messages })))
}

#[derive(Debug, Deserialize)]
pub struct RestSubSpaceProposalRequest {
    pub proposal_type: crate::negotiation::ProposalType,
    pub dimensions: crate::negotiation::ProposalDimensions,
}

pub async fn rest_subspace_submit_proposal(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
    Json(req): Json<RestSubSpaceProposalRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let proposal = state.space_manager.submit_subspace_proposal(
        &sub_space_id,
        &agent.id,
        req.proposal_type,
        req.dimensions,
    ).await?;

    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::SubSpaceProposal {
        sub_space_id: sub_space_id.clone(),
        proposal: proposal.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_subspace_broadcast_tx(&sub_space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&proposal)?))
}

pub async fn rest_get_subspace_proposals(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;
    let proposals = state.space_manager.get_subspace_proposals(&sub_space_id).await?;
    Ok(Json(serde_json::json!({ "proposals": proposals })))
}

#[derive(Debug, Deserialize)]
pub struct RestCloseSubSpaceRequest {
    pub conclusion: String,
}

pub async fn rest_close_subspace(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
    Json(req): Json<RestCloseSubSpaceRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let concluded = req.conclusion == "concluded";

    let sub = state.space_manager.close_subspace(&sub_space_id, &agent.id, concluded).await?;

    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::SubSpaceClosed {
        sub_space_id: sub_space_id.clone(),
        parent_space_id: sub.parent_space_id.clone(),
        status: sub.status.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_subspace_broadcast_tx(&sub_space_id).await {
        let _ = tx.send(broadcast_msg.clone());
    }
    if let Some(tx) = state.space_manager.get_broadcast_tx(&sub.parent_space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&sub)?))
}

// ══════════════════════════════════════════════════
// Phase 10: Coalitions
// ══════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct RestCreateCoalitionRequest {
    pub name: String,
    #[serde(default)]
    pub member_ids: Vec<String>,
    pub stance: Option<serde_json::Value>,
}

pub async fn rest_create_coalition(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestCreateCoalitionRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let coalition = state.space_manager.create_coalition(
        &space_id,
        &agent.id,
        crate::negotiation::coalition::CreateCoalitionRequest {
            name: req.name,
            member_ids: req.member_ids,
            stance: req.stance,
        },
    ).await?;

    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::CoalitionCreated {
        space_id: space_id.clone(),
        coalition: coalition.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&coalition)?))
}

pub async fn rest_list_coalitions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;
    let coalitions = state.space_manager.list_coalitions(&space_id).await?;
    Ok(Json(serde_json::json!({ "coalitions": coalitions })))
}

pub async fn rest_get_coalition(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.get_coalition(&coalition_id).await?
        .ok_or_else(|| GaggleError::NotFound(format!("Coalition not found: {}", coalition_id)))?;
    Ok(Json(serde_json::to_value(&coalition)?))
}

pub async fn rest_join_coalition(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.join_coalition(&coalition_id, &agent.id).await?;
    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::CoalitionMemberJoined {
        coalition_id: coalition_id.clone(),
        agent_id: agent.id.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&coalition.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&coalition)?))
}

pub async fn rest_leave_coalition(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.leave_coalition(&coalition_id, &agent.id).await?;
    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::CoalitionMemberLeft {
        coalition_id: coalition_id.clone(),
        agent_id: agent.id.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&coalition.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&coalition)?))
}

pub async fn rest_update_coalition_stance(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
    Json(req): Json<crate::negotiation::UpdateStanceRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.update_coalition_stance(
        &coalition_id, &agent.id, req.stance,
    ).await?;
    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::CoalitionStanceUpdated {
        coalition_id: coalition_id.clone(),
        stance: coalition.stance.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&coalition.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&coalition)?))
}

pub async fn rest_disband_coalition(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.disband_coalition(&coalition_id, &agent.id).await?;
    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::CoalitionDisbanded {
        coalition_id: coalition_id.clone(),
        space_id: coalition.space_id.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&coalition.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&coalition)?))
}

// ══════════════════════════════════════════════════
// Phase 11: Delegations
// ══════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct RestCreateDelegationRequest {
    pub delegate_id: String,
    pub scope: crate::negotiation::DelegationScope,
    pub expires_at: Option<i64>,
}

pub async fn rest_create_delegation(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestCreateDelegationRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let delegation = state.space_manager.create_delegation(
        &agent.id,
        crate::negotiation::delegation::CreateDelegationRequest {
            delegate_id: req.delegate_id,
            space_id: space_id.clone(),
            scope: req.scope,
            expires_at: req.expires_at,
        },
    ).await?;
    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::DelegationCreated {
        delegation: delegation.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&delegation)?))
}

pub async fn rest_list_delegations(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;
    let delegations = state.space_manager.list_delegations(&space_id).await?;
    Ok(Json(serde_json::json!({ "delegations": delegations })))
}

pub async fn rest_revoke_delegation(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(delegation_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let delegation = state.space_manager.revoke_delegation(&delegation_id, &agent.id).await?;
    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::DelegationRevoked {
        delegation_id: delegation.id.clone(),
        space_id: delegation.space_id.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&delegation.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&delegation)?))
}

pub async fn rest_list_agent_delegations(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;
    let delegations = state.space_manager.list_delegations_by_delegator(&agent_id).await?;
    Ok(Json(serde_json::json!({ "delegations": delegations })))
}

// ══════════════════════════════════════════════════
// Phase 12: Recruitment
// ══════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct RestCreateRecruitmentRequest {
    pub target_id: String,
    #[serde(default = "default_recruit_role")]
    pub role: String,
    #[serde(default)]
    pub pitch: String,
}

fn default_recruit_role() -> String {
    "participant".to_string()
}

pub async fn rest_create_recruitment(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<RestCreateRecruitmentRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let recruitment = state.space_manager.create_recruitment(
        &space_id,
        &agent.id,
        crate::negotiation::recruitment::CreateRecruitmentRequest {
            target_id: req.target_id,
            role: req.role,
            pitch: req.pitch,
        },
    ).await?;

    let broadcast_msg = serde_json::to_string(&super::super::ws::WsOutgoing::RecruitmentCreated {
        recruitment: recruitment.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&recruitment)?))
}

pub async fn rest_accept_recruitment(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((space_id, recruitment_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let mut recruitment = state.space_manager.get_recruitment(&recruitment_id).await?
        .ok_or_else(|| GaggleError::NotFound(format!("Recruitment not found: {}", recruitment_id)))?;

    if recruitment.status != crate::negotiation::RecruitmentStatus::Pending {
        return Err(GaggleError::ValidationError("Recruitment is no longer pending".to_string()));
    }
    if recruitment.target_id != agent.id {
        return Err(GaggleError::Forbidden("Only the target can accept".to_string()));
    }

    recruitment.accept().map_err(|e| GaggleError::ValidationError(e))?;

    let space = state.space_manager.accept_recruitment_join(&recruitment_id, &agent.id, &recruitment).await?;

    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let msg = serde_json::to_string(&super::super::ws::WsOutgoing::RecruitmentAccepted {
            space_id: space_id.clone(),
            recruitment_id: recruitment.id.clone(),
            target_id: agent.id.clone(),
        }).unwrap_or_default();
        let _ = tx.send(msg);
    }

    Ok(Json(serde_json::json!({
        "recruitment": recruitment,
        "space_status": space.status,
    })))
}

pub async fn rest_reject_recruitment(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((_space_id, recruitment_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let recruitment = state.space_manager.reject_recruitment(&recruitment_id, &agent.id).await?;
    Ok(Json(serde_json::to_value(&recruitment)?))
}

pub async fn rest_list_recruitments(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = super::extract_agent(&state, &headers).await?;
    let recruitments = state.space_manager.list_recruitments(&space_id).await?;
    Ok(Json(serde_json::json!({ "recruitments": recruitments })))
}

// ══════════════════════════════════════════════════
// Phase 14: Shared Reality Layer
// ══════════════════════════════════════════════════

pub async fn rest_get_shared_state(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;
    let snapshot = state.shared_state_manager.get_snapshot(&space_id).await?;
    // Update agent cursor so Memory Continuity knows this agent is up-to-date
    if snapshot.version > 0 {
        let _ = state.shared_state_manager.update_cursor(&agent.id, &space_id, snapshot.version).await;
    }
    Ok(Json(serde_json::to_value(&snapshot)?))
}

/// GET /api/v1/spaces/:space_id/reality-alignment — Agent reality alignment status
/// Shows which agents are in sync with the current state version.
/// This is the core visualization for "do all agents share the same reality?"
pub async fn rest_get_reality_alignment(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;

    let current_version = state.shared_state_manager.get_version(&space_id).await?;
    let cursors = state
        .shared_state_manager
        .get_space_agent_cursors(&space_id)
        .await?;

    let agents: Vec<serde_json::Value> = cursors
        .into_iter()
        .map(|(aid, ver)| {
            let in_sync = ver >= current_version;
            serde_json::json!({
                "agent_id": aid,
                "last_seen_version": ver,
                "current_version": current_version,
                "in_sync": in_sync,
                "behind": if current_version > ver { current_version - ver } else { 0 }
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "space_id": space_id,
        "current_version": current_version,
        "agents": agents,
        "all_aligned": agents.iter().all(|a| a["in_sync"].as_bool().unwrap_or(false)),
    })))
}

pub async fn rest_get_state_key(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((space_id, key)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;

    let entry = state.shared_state_manager.get_key(&space_id, &key).await?;
    match entry {
        Some(e) => Ok(Json(serde_json::to_value(&e)?)),
        None => Err(GaggleError::NotFound(format!("State key '{}' not found", key))),
    }
}

pub async fn rest_set_state_key(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((space_id, key)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;

    let space = state.space_manager.get_space(&space_id).await?
        .ok_or_else(|| GaggleError::NotFound("Space not found".to_string()))?;
    if space.status.is_terminal() {
        return Err(GaggleError::ValidationError(
            "Cannot modify state of a closed (terminal) space".to_string(),
        ));
    }

    // Rule-validated state transition: check agent's role has write permission
    let agent_role = space.get_role(&agent.id).unwrap_or("member");
    if !space.rules.role_can_write_state(agent_role) {
        return Err(GaggleError::Forbidden(
            format!("Role '{}' does not have permission to write shared state", agent_role),
        ));
    }

    let value = body
        .get("value")
        .cloned()
        .ok_or_else(|| GaggleError::ValidationError("Missing 'value' field".to_string()))?;

    let expected_version = body.get("expected_version").and_then(|v| v.as_u64());
    let idempotency_key = body
        .get("idempotency_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let req = SetStateRequest {
        key,
        value,
        author_id: agent.id.clone(),
        expected_version,
        idempotency_key,
    };

    let result = state.shared_state_manager.set(&space_id, req).await?;

    let payload = serde_json::json!({
        "type": "state_updated",
        "space_id": space_id,
        "payload": {
            "key": result.key,
            "old_value": result.old_value,
            "new_value": result.new_value,
            "version": result.new_version,
            "author_id": agent.id,
            "event_id": result.event_id,
        }
    });
    let _ = crate::api::ws::push_event(
        &state,
        &space_id,
        "state_updated",
        &payload.to_string(),
    ).await;

    let _ = state.trace_store.log_action(
        &space_id,
        &agent.id,
        crate::api::trace::AuditAction::StateSet,
        Some("state_key"),
        Some(&result.key),
        Some(serde_json::json!({
            "key": result.key,
            "old_value": result.old_value,
            "new_value": result.new_value,
            "version": result.new_version,
        })),
    ).await;

    Ok(Json(serde_json::to_value(&result)?))
}

pub async fn rest_delete_state_key(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((space_id, key)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;

    let space = state.space_manager.get_space(&space_id).await?
        .ok_or_else(|| GaggleError::NotFound("Space not found".to_string()))?;
    if space.status.is_terminal() {
        return Err(GaggleError::ValidationError(
            "Cannot modify state of a closed (terminal) space".to_string(),
        ));
    }

    // Rule-validated state transition: check agent's role has write permission
    let agent_role = space.get_role(&agent.id).unwrap_or("member");
    if !space.rules.role_can_write_state(agent_role) {
        return Err(GaggleError::Forbidden(
            format!("Role '{}' does not have permission to write shared state", agent_role),
        ));
    }

    let result = state
        .shared_state_manager
        .delete(&space_id, &key, &agent.id)
        .await?;

    let payload = serde_json::json!({
        "type": "state_updated",
        "space_id": space_id,
        "payload": {
            "key": result.key,
            "old_value": result.old_value,
            "new_value": null,
            "version": result.new_version,
            "author_id": agent.id,
            "event_id": result.event_id,
        }
    });
    let _ = crate::api::ws::push_event(
        &state,
        &space_id,
        "state_updated",
        &payload.to_string(),
    ).await;

    let _ = state.trace_store.log_action(
        &space_id,
        &agent.id,
        crate::api::trace::AuditAction::StateDeleted,
        Some("state_key"),
        Some(&result.key),
        Some(serde_json::json!({
            "key": result.key,
            "old_value": result.old_value,
            "version": result.new_version,
        })),
    ).await;

    Ok(Json(serde_json::to_value(&result)?))
}

/// GET /api/v1/spaces/:space_id/state/verify-chain
/// 验证 Space 的 event hash chain 完整性
pub async fn rest_verify_chain(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;

    let (total, verified, failed, latest_hash) = state
        .shared_state_manager
        .verify_chain(&space_id)
        .await?;

    let integrity = if total == 0 {
        "empty".to_string()
    } else if failed == 0 {
        "intact".to_string()
    } else {
        format!("compromised ({} failures)", failed)
    };

    Ok(Json(serde_json::json!({
        "space_id": space_id,
        "chain_integrity": integrity,
        "total_events": total,
        "verified": verified,
        "failed": failed,
        "latest_hash": latest_hash,
    })))
}

/// GET /api/v1/spaces/:space_id/state/integrity
/// 验证 materialized state 与 event log reconstructed state 一致性
pub async fn rest_state_integrity(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;

    let (consistent, mat_count, rec_count, diffs) = state
        .shared_state_manager
        .verify_state_integrity(&space_id)
        .await?;

    let current_version = state.shared_state_manager.get_version(&space_id).await?;

    Ok(Json(serde_json::json!({
        "space_id": space_id,
        "current_version": current_version,
        "state_integrity": if consistent { "consistent" } else { "divergent" },
        "materialized_entries": mat_count,
        "reconstructed_entries": rec_count,
        "divergences": diffs,
    })))
}

#[derive(Debug, Deserialize)]
pub struct StateEventsQuery {
    limit: Option<usize>,
    before_version: Option<u64>,
}

pub async fn rest_get_state_events(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Query(params): Query<StateEventsQuery>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;

    let limit = params.limit.unwrap_or(50).min(200);
    let result = state
        .shared_state_manager
        .get_events(&space_id, limit, params.before_version)
        .await?;

    // Update cursor to latest version seen (top of results = most recent)
    if let Some(top_event) = result.events.first() {
        let _ = state.shared_state_manager.update_cursor(&agent.id, &space_id, top_event.space_version).await;
    }

    Ok(Json(serde_json::to_value(&result)?))
}

pub async fn rest_reconstruct_state(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((space_id, version)): Path<(String, u64)>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;

    let snapshot = state
        .shared_state_manager
        .reconstruct_at_version(&space_id, version)
        .await?;

    Ok(Json(serde_json::to_value(&snapshot)?))
}

// ══════════════════════════════════════════════════
// Trace / Observability
// ══════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct TraceQuery {
    limit: Option<usize>,
    before_id: Option<i64>,
    action: Option<String>,
    agent: Option<String>,
}

pub async fn rest_get_trace(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Query(query): Query<TraceQuery>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent.id).await?;

    let limit = query.limit.unwrap_or(50).min(200);
    let entries = state
        .trace_store
        .query_trace_ex(
            &space_id,
            limit,
            query.before_id,
            query.action.as_deref(),
            query.agent.as_deref(),
        )
        .await?;

    // Always include stats summary
    let stats = state.trace_store.trace_stats(&space_id).await.unwrap_or_default();

    Ok(Json(serde_json::json!({
        "space_id": space_id,
        "entries": entries,
        "count": entries.len(),
        "stats": stats,
    })))
}

// ══════════════════════════════════════════════════
// Queue Stats
// ══════════════════════════════════════════════════

pub async fn rest_queue_stats(
    State(state): State<AppState>,
) -> Result<Json<crate::api::event_queue::QueueStats>, GaggleError> {
    let stats = state.event_queue.get_stats().await?;
    Ok(Json(stats))
}

/// GET /api/v1/events/dead-letters — 列出 dead letter 事件
#[derive(Debug, serde::Deserialize)]
pub struct ListDeadLettersQuery {
    pub agent_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    50
}

pub async fn rest_list_dead_letters(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ListDeadLettersQuery>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;

    let limit = query.limit.min(200);
    let events = state
        .event_queue
        .list_dead_letters(query.agent_id.as_deref(), limit)
        .await?;

    Ok(Json(serde_json::json!({
        "dead_letters": events,
        "count": events.len(),
    })))
}

/// POST /api/v1/events/dead-letters/:event_id/retry — 重试单个 dead letter
pub async fn rest_retry_dead_letter(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(event_id): Path<i64>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;

    let retried = state.event_queue.retry_dead_letter(event_id).await?;

    if retried {
        Ok(Json(serde_json::json!({
            "message": "Dead letter event queued for retry",
            "event_id": event_id,
        })))
    } else {
        Err(GaggleError::NotFound(format!(
            "Dead letter event not found: {}",
            event_id
        )))
    }
}

/// POST /api/v1/events/dead-letters/cleanup — 清理旧的 dead letter
#[derive(Debug, serde::Deserialize)]
pub struct CleanupDeadLettersRequest {
    #[serde(default = "default_cleanup_days")]
    pub older_than_days: i64,
}

fn default_cleanup_days() -> i64 {
    7
}

pub async fn rest_cleanup_dead_letters(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CleanupDeadLettersRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;

    let deleted = state
        .event_queue
        .cleanup_dead_letters(req.older_than_days)
        .await?;

    Ok(Json(serde_json::json!({
        "message": "Dead letters cleaned up",
        "deleted_count": deleted,
        "older_than_days": req.older_than_days,
    })))
}
