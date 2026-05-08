//! Market + Discovery + Needs + Reputation + Templates + Evidence

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;

use crate::discovery::{
    DiscoveryProfile, NeedSearchQuery, ProviderSearchQuery, ScoredDiscoveryProfile,
    UpdateProfileRequest,
};
use crate::error::GaggleError;
use crate::marketplace::SharePriceRequest;
use crate::reputation::{
    calculator::ReputationCalculator, EventType, Outcome, RateResponse, ReputationDetail,
    ReputationEvent,
};
use crate::templates;

use super::AppState;

// ── Evidence ──

#[derive(Debug, Deserialize)]
pub struct EvidenceRequest {
    pub evidence_type: String,
    pub hash: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
pub struct EvidenceResponse {
    pub tx_signature: String,
    pub slot: u64,
    pub confirmed_at: i64,
}

pub async fn submit_evidence(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<EvidenceRequest>,
) -> Result<Json<EvidenceResponse>, GaggleError> {
    let agent_id = super::extract_agent_id(&state, &headers).await?;
    super::verify_space_member(&state, &space_id, &agent_id).await?;
    state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or_else(|| GaggleError::SpaceNotFound(space_id.clone()))?;

    let tx_signature = format!(
        "simulated_tx_{}_{}_{}",
        space_id,
        req.evidence_type,
        Utc::now().timestamp_millis()
    );

    Ok(Json(EvidenceResponse {
        tx_signature,
        slot: 123456789,
        confirmed_at: Utc::now().timestamp_millis(),
    }))
}

// ── Templates ──

/// GET /api/v1/templates — 列出所有模板
pub async fn list_templates(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Vec<templates::AgentTemplate>>, GaggleError> {
    let category = params.get("category").map(|s| s.as_str());
    Ok(Json(templates::list_templates(category)))
}

/// GET /api/v1/templates/:template_id — 获取单个模板
pub async fn get_template(
    Path(template_id): Path<String>,
) -> Result<Json<templates::AgentTemplate>, GaggleError> {
    templates::get_template(&template_id)
        .map(Json)
        .ok_or_else(|| GaggleError::NotFound(format!("Template not found: {}", template_id)))
}

// ── Market ──

#[derive(Debug, Deserialize)]
pub struct MarketQueryParams {
    pub period: Option<String>,
}

/// GET /api/v1/market — 获取全部市场价格
pub async fn get_all_market_prices(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::marketplace::MarketPrice>>, GaggleError> {
    let prices = state.marketplace_store.get_all_market_prices().await?;
    Ok(Json(prices))
}

/// GET /api/v1/market/:category — 获取分类市场价格
pub async fn get_market_prices(
    State(state): State<AppState>,
    Path(category): Path<String>,
) -> Result<Json<Vec<crate::marketplace::MarketPrice>>, GaggleError> {
    let prices = state.marketplace_store.get_market_prices(&category).await?;
    Ok(Json(prices))
}

/// POST /api/v1/market/share — 手动贡献价格
pub async fn share_market_price(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<SharePriceRequest>,
) -> Result<Json<crate::marketplace::MarketContribution>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let contribution = state.marketplace_store.share_price(&agent.id, req).await?;
    Ok(Json(contribution))
}

/// GET /api/v1/market/:category/contributions — 获取最近贡献
pub async fn get_market_contributions(
    State(state): State<AppState>,
    Path(category): Path<String>,
) -> Result<Json<Vec<crate::marketplace::MarketContribution>>, GaggleError> {
    let contributions = state.marketplace_store.get_recent_contributions(&category, 20).await?;
    Ok(Json(contributions))
}

// ── Discovery ──

/// 搜索 Provider（带信誉加权排序）
pub async fn search_providers(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ProviderSearchQuery>,
) -> Result<Json<Vec<ScoredDiscoveryProfile>>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;
    let profiles = state.discovery_store.search_providers(&query).await?;

    let agent_ids: Vec<String> = profiles.iter().map(|p| p.agent_id.clone()).collect();
    let reputation_map = state
        .reputation_store
        .get_batch_summaries(agent_ids)
        .await
        .unwrap_or_default();

    let query_text = query.query.as_deref().unwrap_or("");
    let mut scored: Vec<ScoredDiscoveryProfile> = profiles
        .into_iter()
        .map(|profile| {
            let rep = reputation_map.get(&profile.agent_id);
            let reputation_score = rep.map(|r| r.reputation_score / 100.0).unwrap_or(0.0);

            let capability_match = if let Some(ref query_skills) = query.skills {
                let query_set: Vec<&str> = query_skills.split(',').collect();
                let matched = profile
                    .skills
                    .iter()
                    .filter(|s| query_set.iter().any(|q| q.trim().eq_ignore_ascii_case(s)))
                    .count();
                if query_set.is_empty() {
                    0.5
                } else {
                    matched as f64 / query_set.len() as f64
                }
            } else {
                0.5
            };

            let keyword_relevance = if query_text.is_empty() {
                0.5
            } else {
                let lower = query_text.to_lowercase();
                let mut matches = 0;
                let mut total = 0;

                total += 1;
                if profile.display_name.to_lowercase().contains(&lower) {
                    matches += 1;
                }
                total += 1;
                if profile
                    .description
                    .as_ref()
                    .map_or(false, |d| d.to_lowercase().contains(&lower))
                {
                    matches += 1;
                }
                total += 1;
                if profile
                    .skills
                    .iter()
                    .any(|s| s.to_lowercase().contains(&lower))
                {
                    matches += 1;
                }
                total += 1;
                if profile
                    .capabilities
                    .category
                    .to_lowercase()
                    .contains(&lower)
                {
                    matches += 1;
                }
                matches as f64 / total as f64
            };

            let final_score =
                0.4 * reputation_score + 0.3 * capability_match + 0.3 * keyword_relevance;

            ScoredDiscoveryProfile {
                profile,
                reputation_score,
                capability_match,
                final_score,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.final_score
            .partial_cmp(&a.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(Json(scored))
}

/// 获取 Provider Discovery Profile 详情
pub async fn get_provider_profile(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<DiscoveryProfile>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;
    let profile = state
        .discovery_store
        .get_profile(&agent_id)
        .await?
        .ok_or_else(|| {
            GaggleError::NotFound(format!("Provider profile not found: {}", agent_id))
        })?;
    Ok(Json(profile))
}

/// Provider 更新自己的 Discovery Profile
pub async fn update_provider_profile(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<Json<DiscoveryProfile>, GaggleError> {
    let api_key = super::extract_bearer(&headers)?;
    if !api_key.starts_with("gag_") {
        return Err(GaggleError::Unauthorized(
            "Agent API key required (gag_*)".to_string(),
        ));
    }

    let agent = state
        .registry
        .get_by_api_key(&api_key)
        .await?
        .ok_or_else(|| GaggleError::Unauthorized("Invalid agent API key".to_string()))?;

    let profile = state.discovery_store.upsert_profile(&agent.id, req).await?;
    Ok(Json(profile))
}

// ── Need Broadcast ──

/// POST /api/v1/needs — 发布需求
pub async fn publish_need(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<crate::discovery::PublishNeedRequest>,
) -> Result<(StatusCode, Json<crate::discovery::Need>), GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let need = state.discovery_store.publish_need(&agent.id, req).await?;

    let matched_providers = state.discovery_store.find_matching_providers(&need).await?;
    let matched_count = matched_providers.len() as i32;
    if matched_count > 0 {
        let _ = state.discovery_store.update_matched_count(&need.id, matched_count).await;
    }

    let published_msg = serde_json::to_string(&super::super::ws::WsOutgoing::NeedPublished {
        need_id: need.id.clone(),
        payload: super::super::ws::NeedPublishedPayload {
            need: crate::discovery::Need {
                matched_provider_count: matched_count,
                ..need.clone()
            },
            matched_provider_count: matched_count,
        },
    })?;
    let _ = super::super::ws::push_event(&state, &agent.id, "need_published", &published_msg).await;

    for provider in &matched_providers {
        let matched_msg = serde_json::to_string(&super::super::ws::WsOutgoing::NeedMatched {
            need_id: need.id.clone(),
            payload: super::super::ws::NeedMatchedPayload {
                need: crate::discovery::Need {
                    matched_provider_count: matched_count,
                    ..need.clone()
                },
            },
        })?;
        let _ = super::super::ws::push_event(&state, &provider.agent_id, "need_matched", &matched_msg).await;
    }

    Ok((StatusCode::CREATED, Json(crate::discovery::Need {
        matched_provider_count: matched_count,
        ..need
    })))
}

/// GET /api/v1/needs — 搜索需求
pub async fn search_needs(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<NeedSearchQuery>,
) -> Result<Json<crate::discovery::PaginatedResult<crate::discovery::Need>>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;
    let result = state.discovery_store.search_needs(&query).await?;
    Ok(Json(result))
}

/// GET /api/v1/needs/:need_id — 获取需求详情
pub async fn get_need(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(need_id): Path<String>,
) -> Result<Json<crate::discovery::Need>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;
    let need = state
        .discovery_store
        .get_need(&need_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Need not found: {}", need_id)))?;
    Ok(Json(need))
}

/// POST /api/v1/needs/:need_id/cancel — 取消需求
pub async fn cancel_need(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(need_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;

    let need = state
        .discovery_store
        .get_need(&need_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Need not found: {}", need_id)))?;

    if need.creator_id != agent.id {
        return Err(GaggleError::Forbidden(
            "Only the creator can cancel this need".to_string(),
        ));
    }

    state
        .discovery_store
        .update_need_status(&need_id, &crate::discovery::NeedStatus::Cancelled)
        .await?;

    let cancel_msg = serde_json::to_string(&super::super::ws::WsOutgoing::NeedCancelled {
        need_id: need_id.clone(),
    })?;
    let _ = super::super::ws::push_event(&state, &agent.id, "need_cancelled", &cancel_msg).await;

    Ok(Json(serde_json::json!({
        "status": "cancelled",
        "need_id": need_id,
    })))
}

/// GET /api/v1/needs/my — 获取我的需求
pub async fn get_my_needs(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<crate::discovery::Need>>, GaggleError> {
    let agent = super::extract_agent(&state, &headers).await?;
    let needs = state.discovery_store.get_my_needs(&agent.id).await?;
    Ok(Json(needs))
}

// ── Reputation ──

/// 获取 Agent 的信誉详情
pub async fn get_agent_reputation(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<ReputationDetail>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;
    let calculator = ReputationCalculator::new(state.reputation_store.clone());
    let (summary, recent_events) = calculator.get_detail(&agent_id, 10).await?;

    Ok(Json(ReputationDetail {
        summary,
        recent_events,
    }))
}

/// 为 Space 结束后评分
#[derive(Debug, Deserialize)]
pub struct RateRequest {
    pub agent_id: String,
    pub space_id: String,
    pub event_type: EventType,
    pub outcome: Outcome,
    pub rating: Option<i32>,
    pub counterparty_id: String,
}

pub async fn rate_agent(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RateRequest>,
) -> Result<(StatusCode, Json<RateResponse>), GaggleError> {
    let api_key = super::extract_bearer(&headers)?;
    if !api_key.starts_with("usr_") && !api_key.starts_with("gag_") {
        return Err(GaggleError::Unauthorized(
            "User or Agent API key required (usr_* or gag_*)".to_string(),
        ));
    }

    ReputationEvent::validate_rating(req.rating)?;

    let already_rated = state
        .reputation_store
        .has_rating_for_space(&req.agent_id, &req.space_id)
        .await?;
    if already_rated {
        return Err(GaggleError::ValidationError(
            "Already rated for this space".to_string(),
        ));
    }

    let event = ReputationEvent::new(
        req.agent_id.clone(),
        req.space_id,
        req.event_type,
        req.outcome,
        req.rating,
        req.counterparty_id,
    );

    state.reputation_store.create_event(event.clone()).await?;

    let calculator = ReputationCalculator::new(state.reputation_store.clone());
    let summary = calculator.recalculate(&req.agent_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(RateResponse {
            event_id: event.id,
            agent_id: req.agent_id,
            new_reputation_score: summary.reputation_score,
        }),
    ))
}
