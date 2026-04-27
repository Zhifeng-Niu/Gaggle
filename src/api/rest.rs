//! REST API处理函数

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::agents::{AgentRegistry, RegisterRequest, RegisterResponse, UpdateAgentRequest};
use crate::api::event_queue::EventQueue;
use crate::discovery::{
    DiscoveryProfile, DiscoveryStore, NeedSearchQuery, ProviderSearchQuery, ScoredDiscoveryProfile,
    UpdateProfileRequest,
};
use crate::error::GaggleError;
use crate::execution::ExecutionStore;
use crate::marketplace::{MarketplaceStore, SharePriceRequest};
use crate::negotiation::{Space, SpaceManager};
use crate::reputation::{
    calculator::ReputationCalculator, EventType, Outcome, RateResponse, ReputationDetail,
    ReputationEvent, ReputationStore,
};
use crate::templates;
use crate::users::{
    User, UserLoginRequest, UserLoginResponse, UserRegisterRequest, UserRegisterResponse, UserStore,
};

/// 在线连接元信息
pub struct ConnectionInfo {
    pub tx: broadcast::Sender<String>,
    pub connected_since: i64,
    /// AtomicI64：Ping 更新无需 write lock
    pub last_ping: std::sync::atomic::AtomicI64,
    /// 全局唯一连接 ID（用于精确匹配清理）
    pub conn_id: u64,
}

impl Clone for ConnectionInfo {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            connected_since: self.connected_since,
            last_ping: std::sync::atomic::AtomicI64::new(
                self.last_ping.load(std::sync::atomic::Ordering::Relaxed),
            ),
            conn_id: self.conn_id,
        }
    }
}

/// 在线 WebSocket 连接表：agent_id → Vec<ConnectionInfo>
/// 支持同一个 Agent 多个 WS 连接并存（脚本 + Dashboard 同时在线）
pub type OnlineAgents = Arc<RwLock<HashMap<String, Vec<ConnectionInfo>>>>;

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<AgentRegistry>,
    pub space_manager: Arc<SpaceManager>,
    pub user_store: Arc<UserStore>,
    pub discovery_store: Arc<DiscoveryStore>,
    pub reputation_store: Arc<ReputationStore>,
    pub execution_store: Arc<ExecutionStore>,
    pub marketplace_store: Arc<MarketplaceStore>,
    /// 在线 Agent 连接表
    pub online_agents: OnlineAgents,
    /// 离线事件队列
    pub event_queue: Arc<EventQueue>,
}

pub async fn register_agent(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), GaggleError> {
    // 可选：如果带 usr_ Bearer token，关联到用户
    let user_id = extract_user_id(&state, &headers).await?;
    let response = state.registry.register(req, user_id).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// 用户注册
pub async fn register_user(
    State(state): State<AppState>,
    Json(req): Json<UserRegisterRequest>,
) -> Result<(StatusCode, Json<UserRegisterResponse>), GaggleError> {
    let response = state.user_store.register(req).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// 用户登录
pub async fn login_user(
    State(state): State<AppState>,
    Json(req): Json<UserLoginRequest>,
) -> Result<Json<UserLoginResponse>, GaggleError> {
    let response = state.user_store.login(req).await?;
    Ok(Json(response))
}

/// 获取当前用户信息
pub async fn get_me(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<User>, GaggleError> {
    let user = extract_user(&state, &headers).await?;
    Ok(Json(user))
}

/// 获取当前用户的 Agent 列表
pub async fn get_my_agents(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<crate::agents::Agent>>, GaggleError> {
    let user = extract_user(&state, &headers).await?;
    let agents = state.registry.list_user_agents(&user.id).await?;
    Ok(Json(agents))
}

/// 获取当前用户所有 Agent 的 Spaces（聚合视图）
pub async fn get_user_spaces(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<Space>>, GaggleError> {
    let user = extract_user(&state, &headers).await?;
    let agents = state.registry.list_user_agents(&user.id).await?;
    let mut all_spaces = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for agent in &agents {
        let spaces = state.space_manager.get_agent_spaces(&agent.id).await?;
        for space in spaces {
            if seen.insert(space.id.clone()) {
                all_spaces.push(space);
            }
        }
    }
    // 按 updated_at 降序
    all_spaces.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(Json(all_spaces))
}

/// 从 Bearer token 提取 user_id（可选，用于 Agent 注册时关联用户）
async fn extract_user_id(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<Option<String>, GaggleError> {
    let api_key = extract_bearer(headers)?;
    if api_key.starts_with("usr_") {
        let user = state
            .user_store
            .get_by_api_key(&api_key)
            .await?
            .ok_or_else(|| GaggleError::Unauthorized("Invalid user API key".to_string()))?;
        Ok(Some(user.id))
    } else {
        Ok(None)
    }
}

/// 从 Bearer token 提取并验证用户（必须）
async fn extract_user(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<User, GaggleError> {
    let api_key = extract_bearer(headers)?;
    if !api_key.starts_with("usr_") {
        return Err(GaggleError::Unauthorized(
            "User API key required".to_string(),
        ));
    }
    state
        .user_store
        .get_by_api_key(&api_key)
        .await?
        .ok_or_else(|| GaggleError::Unauthorized("Invalid user API key".to_string()))
}

/// 从 Authorization header 提取 Bearer token
fn extract_bearer(headers: &axum::http::HeaderMap) -> Result<String, GaggleError> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(|s| s.to_string()))
        .ok_or_else(|| {
            GaggleError::Unauthorized("Missing or invalid Authorization header".to_string())
        })
}

pub async fn get_space(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
) -> Result<Json<Space>, GaggleError> {
    let space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or(GaggleError::SpaceNotFound(space_id))?;
    Ok(Json(space))
}

pub async fn list_agent_spaces(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<Vec<Space>>, GaggleError> {
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
    let limit = query.limit.unwrap_or(200).min(1000);
    let messages = state
        .space_manager
        .get_messages(&space_id, query.after, limit)
        .await?;

    // Phase 7: 如果有认证头，按 visibility 过滤消息
    let agent_id_opt = extract_agent_id(&state, &headers).await.ok();
    let space_opt = state.space_manager.get_space(&space_id).await.ok().flatten();

    let vals: Vec<serde_json::Value> = if let (Some(agent_id), Some(space)) = (&agent_id_opt, &space_opt) {
        let rules = &space.rules;
        messages
            .into_iter()
            .filter(|msg| {
                crate::negotiation::VisibilityEngine::should_deliver_message(rules, msg, agent_id, &space)
            })
            .map(|msg| serde_json::to_value(&msg).unwrap_or_default())
            .collect()
    } else {
        // 无认证或无 space 信息：返回所有消息（向后兼容）
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
    Path(space_id): Path<String>,
) -> Result<Json<Vec<crate::negotiation::Proposal>>, GaggleError> {
    let proposals = state.space_manager.get_space_proposals(&space_id).await?;
    Ok(Json(proposals))
}

/// GET /spaces/:space_id/members
pub async fn get_space_members(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
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
    Path(space_id): Path<String>,
    Json(req): Json<EvidenceRequest>,
) -> Result<Json<EvidenceResponse>, GaggleError> {
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

// ==================== Templates API ====================

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

// ==================== Market API ====================

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
    let agent = extract_agent(&state, &headers).await?;
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

// ==================== Discovery API ====================

/// 搜索 Provider（带信誉加权排序）
pub async fn search_providers(
    State(state): State<AppState>,
    Query(query): Query<ProviderSearchQuery>,
) -> Result<Json<Vec<ScoredDiscoveryProfile>>, GaggleError> {
    let profiles = state.discovery_store.search_providers(&query).await?;

    // 批量获取信誉摘要
    let agent_ids: Vec<String> = profiles.iter().map(|p| p.agent_id.clone()).collect();
    let reputation_map = state
        .reputation_store
        .get_batch_summaries(agent_ids)
        .await
        .unwrap_or_default();

    // 计算综合得分
    let query_text = query.query.as_deref().unwrap_or("");
    let mut scored: Vec<ScoredDiscoveryProfile> = profiles
        .into_iter()
        .map(|profile| {
            let rep = reputation_map.get(&profile.agent_id);
            let reputation_score = rep.map(|r| r.reputation_score / 100.0).unwrap_or(0.0);

            // 能力匹配度：skills 重叠比例
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

            // 关键词匹配度
            let keyword_relevance = if query_text.is_empty() {
                0.5
            } else {
                let lower = query_text.to_lowercase();
                let mut matches = 0;
                let mut total = 0;

                total += 1; // name
                if profile.display_name.to_lowercase().contains(&lower) {
                    matches += 1;
                }
                total += 1; // description
                if profile
                    .description
                    .as_ref()
                    .map_or(false, |d| d.to_lowercase().contains(&lower))
                {
                    matches += 1;
                }
                total += 1; // skills
                if profile
                    .skills
                    .iter()
                    .any(|s| s.to_lowercase().contains(&lower))
                {
                    matches += 1;
                }
                total += 1; // category
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

    // 按综合得分降序排序
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
    Path(agent_id): Path<String>,
) -> Result<Json<DiscoveryProfile>, GaggleError> {
    let profile = state
        .discovery_store
        .get_profile(&agent_id)
        .await?
        .ok_or_else(|| {
            GaggleError::NotFound(format!("Provider profile not found: {}", agent_id))
        })?;
    Ok(Json(profile))
}

/// Provider 更新自己的 Discovery Profile（需要 gag_ agent key 鉴权）
pub async fn update_provider_profile(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<Json<DiscoveryProfile>, GaggleError> {
    // 验证 Agent API Key（必须是 gag_ 开头）
    let api_key = extract_bearer(&headers)?;
    if !api_key.starts_with("gag_") {
        return Err(GaggleError::Unauthorized(
            "Agent API key required (gag_*)".to_string(),
        ));
    }

    // 获取 Agent 信息
    let agent = state
        .registry
        .get_by_api_key(&api_key)
        .await?
        .ok_or_else(|| GaggleError::Unauthorized("Invalid agent API key".to_string()))?;

    // 更新或创建 Discovery Profile
    let profile = state.discovery_store.upsert_profile(&agent.id, req).await?;

    Ok(Json(profile))
}

// ==================== Need Broadcast API ====================

/// POST /api/v1/needs — 发布需求
pub async fn publish_need(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<crate::discovery::PublishNeedRequest>,
) -> Result<(StatusCode, Json<crate::discovery::Need>), GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let need = state.discovery_store.publish_need(&agent.id, req).await?;

    // 匹配 Provider
    let matched_providers = state.discovery_store.find_matching_providers(&need).await?;
    let matched_count = matched_providers.len() as i32;
    if matched_count > 0 {
        let _ = state.discovery_store.update_matched_count(&need.id, matched_count).await;
    }

    // 向 creator 发送 NeedPublished 事件
    let published_msg = serde_json::to_string(&super::ws::WsOutgoing::NeedPublished {
        need_id: need.id.clone(),
        payload: super::ws::NeedPublishedPayload {
            need: crate::discovery::Need {
                matched_provider_count: matched_count,
                ..need.clone()
            },
            matched_provider_count: matched_count,
        },
    })?;
    let _ = super::ws::push_event(&state, &agent.id, "need_published", &published_msg).await;

    // 向每个匹配的 Provider 发送 NeedMatched 事件
    for provider in &matched_providers {
        let matched_msg = serde_json::to_string(&super::ws::WsOutgoing::NeedMatched {
            need_id: need.id.clone(),
            payload: super::ws::NeedMatchedPayload {
                need: crate::discovery::Need {
                    matched_provider_count: matched_count,
                    ..need.clone()
                },
            },
        })?;
        let _ = super::ws::push_event(&state, &provider.agent_id, "need_matched", &matched_msg).await;
    }

    Ok((StatusCode::CREATED, Json(crate::discovery::Need {
        matched_provider_count: matched_count,
        ..need
    })))
}

/// GET /api/v1/needs — 搜索需求
pub async fn search_needs(
    State(state): State<AppState>,
    Query(query): Query<NeedSearchQuery>,
) -> Result<Json<crate::discovery::PaginatedResult<crate::discovery::Need>>, GaggleError> {
    let result = state.discovery_store.search_needs(&query).await?;
    Ok(Json(result))
}

/// GET /api/v1/needs/:need_id — 获取需求详情
pub async fn get_need(
    State(state): State<AppState>,
    Path(need_id): Path<String>,
) -> Result<Json<crate::discovery::Need>, GaggleError> {
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
    let agent = extract_agent(&state, &headers).await?;

    // 验证调用者是 creator
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

    // 通知 creator
    let cancel_msg = serde_json::to_string(&super::ws::WsOutgoing::NeedCancelled {
        need_id: need_id.clone(),
    })?;
    let _ = super::ws::push_event(&state, &agent.id, "need_cancelled", &cancel_msg).await;

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
    let agent = extract_agent(&state, &headers).await?;
    let needs = state.discovery_store.get_my_needs(&agent.id).await?;
    Ok(Json(needs))
}

// ==================== Reputation API ====================

/// 获取 Agent 的信誉详情
pub async fn get_agent_reputation(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<ReputationDetail>, GaggleError> {
    let calculator = ReputationCalculator::new(state.reputation_store.clone());
    let (summary, recent_events) = calculator.get_detail(&agent_id, 10).await?;

    Ok(Json(ReputationDetail {
        summary,
        recent_events,
    }))
}

/// 为 Space 结束后评分（需要 usr_ 或 gag_ key 鉴权）
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
    // 验证 API Key（usr_ 或 gag_ 开头）
    let api_key = extract_bearer(&headers)?;
    if !api_key.starts_with("usr_") && !api_key.starts_with("gag_") {
        return Err(GaggleError::Unauthorized(
            "User or Agent API key required (usr_* or gag_*)".to_string(),
        ));
    }

    // 验证评分范围
    ReputationEvent::validate_rating(req.rating)?;

    // 检查是否已经评分过
    let already_rated = state
        .reputation_store
        .has_rating_for_space(&req.agent_id, &req.space_id)
        .await?;
    if already_rated {
        return Err(GaggleError::ValidationError(
            "Already rated for this space".to_string(),
        ));
    }

    // 创建信誉事件
    let event = ReputationEvent::new(
        req.agent_id.clone(),
        req.space_id,
        req.event_type,
        req.outcome,
        req.rating,
        req.counterparty_id,
    );

    state.reputation_store.create_event(event.clone()).await?;

    // 重新计算信誉评分
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

/// GET /api/v1/agents/:agent_id/status
/// 查询 Agent 在线状态
pub async fn get_agent_status(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let online = state.online_agents.read().await;
    let info = online.get(&agent_id);

    let response = if let Some(conns) = info {
        if conns.is_empty() {
            serde_json::json!({
                "agent_id": agent_id,
                "online": false,
            })
        } else {
            serde_json::json!({
                "agent_id": agent_id,
                "online": true,
                "connection_count": conns.len(),
                "connected_since": conns.first().map(|c| c.connected_since),
                "last_ping": conns.iter().map(|c| c.last_ping.load(std::sync::atomic::Ordering::Relaxed)).max(),
            })
        }
    } else {
        serde_json::json!({
            "agent_id": agent_id,
            "online": false,
        })
    };

    Ok(Json(response))
}

// ==================== Agent CRUD ====================

/// Agent 公开信息（不含 api_key / api_secret_hash）
#[derive(Debug, serde::Serialize)]
pub struct AgentPublic {
    pub id: String,
    pub agent_type: crate::agents::AgentType,
    pub name: String,
    pub metadata: serde_json::Value,
    pub public_key: Option<String>,
    pub created_at: i64,
    pub user_id: Option<String>,
    pub disabled_at: Option<i64>,
    pub organization: Option<String>,
    pub callback_url: Option<String>,
    pub online: bool,
}

impl AgentPublic {
    fn from_agent(agent: &crate::agents::Agent, online: bool) -> Self {
        Self {
            id: agent.id.clone(),
            agent_type: agent.agent_type.clone(),
            name: agent.name.clone(),
            metadata: agent.metadata.clone(),
            public_key: agent.public_key.clone(),
            created_at: agent.created_at,
            user_id: agent.user_id.clone(),
            disabled_at: agent.disabled_at,
            organization: agent.organization.clone(),
            callback_url: agent.callback_url.clone(),
            online,
        }
    }
}

/// GET /api/v1/agents/:agent_id — 获取单个 Agent 公开信息
pub async fn get_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentPublic>, GaggleError> {
    let agent = state
        .registry
        .get_by_id(&agent_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Agent not found: {}", agent_id)))?;

    if agent.disabled_at.is_some() {
        return Err(GaggleError::NotFound(format!("Agent not found: {}", agent_id)));
    }

    let online = state.online_agents.read().await.contains_key(&agent_id);
    Ok(Json(AgentPublic::from_agent(&agent, online)))
}

/// 鉴权辅助：验证操作者是否有权操作指定 Agent
/// 支持 usr_（资源所有者）和 gag_（Agent 自身）两种方式
async fn verify_agent_ownership(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    agent_id: &str,
) -> Result<crate::agents::Agent, GaggleError> {
    let api_key = extract_bearer(headers)?;
    let agent = state
        .registry
        .get_by_id(agent_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Agent not found: {}", agent_id)))?;

    if agent.disabled_at.is_some() {
        return Err(GaggleError::ValidationError("Agent is disabled".to_string()));
    }

    if api_key.starts_with("gag_") {
        // Agent 操作自身
        if agent.api_key != api_key {
            return Err(GaggleError::Forbidden(
                "Not your agent".to_string(),
            ));
        }
    } else if api_key.starts_with("usr_") {
        // 用户操作自己的 Agent
        let user = state
            .user_store
            .get_by_api_key(&api_key)
            .await?
            .ok_or_else(|| GaggleError::Unauthorized("Invalid user API key".to_string()))?;
        match &agent.user_id {
            Some(uid) if uid == &user.id => {}
            _ => {
                return Err(GaggleError::Forbidden(
                    "Not your agent".to_string(),
                ));
            }
        }
    } else {
        return Err(GaggleError::Unauthorized(
            "User or Agent API key required".to_string(),
        ));
    }

    Ok(agent)
}

/// DELETE /api/v1/agents/:agent_id — 软删除 Agent
pub async fn delete_agent(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), GaggleError> {
    verify_agent_ownership(&state, &headers, &agent_id).await?;

    let agent = state.registry.disable(&agent_id).await?;

    // 断开所有 WebSocket 连接
    let mut online = state.online_agents.write().await;
    if let Some(conns) = online.remove(&agent_id) {
        for conn in conns {
            let _ = conn.tx.send(
                serde_json::json!({"type": "agent_disabled", "agent_id": agent_id}).to_string(),
            );
        }
    }

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "Agent disabled",
            "agent_id": agent.id,
            "disabled_at": agent.disabled_at,
        })),
    ))
}

/// PUT /api/v1/agents/:agent_id — 更新 Agent 信息
#[derive(Debug, serde::Deserialize)]
pub struct UpdateAgentWithIdRequest {
    pub agent_id: String,
    pub name: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub organization: Option<String>,
    pub callback_url: Option<String>,
}

pub async fn update_agent(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdateAgentWithIdRequest>,
) -> Result<(StatusCode, Json<AgentPublic>), GaggleError> {
    verify_agent_ownership(&state, &headers, &req.agent_id).await?;

    let update_req = UpdateAgentRequest {
        name: req.name,
        metadata: req.metadata,
        organization: req.organization,
        callback_url: req.callback_url,
    };
    let agent = state.registry.update(&req.agent_id, &update_req).await?;
    let online = state.online_agents.read().await.contains_key(&req.agent_id);
    Ok((StatusCode::OK, Json(AgentPublic::from_agent(&agent, online))))
}

// ==================== REST Write Operations ====================

/// 鉴权辅助：从 Bearer token 提取并验证 Agent（gag_ key）
async fn extract_agent(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<crate::agents::Agent, GaggleError> {
    let api_key = extract_bearer(headers)?;
    if !api_key.starts_with("gag_") {
        return Err(GaggleError::Unauthorized(
            "Agent API key required (gag_*)".to_string(),
        ));
    }
    state
        .registry
        .get_by_api_key(&api_key)
        .await?
        .ok_or_else(|| GaggleError::Unauthorized("Invalid agent API key".to_string()))
}

/// 轻量鉴权：只提取 agent_id（不验证完整 agent），失败返回 None
async fn extract_agent_id(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<String, GaggleError> {
    let agent = extract_agent(state, headers).await?;
    Ok(agent.id)
}

/// POST /api/v1/spaces — 创建双边谈判 Space
#[derive(Debug, Deserialize)]
pub struct RestCreateSpaceRequest {
    pub name: String,
    #[serde(default)]
    pub invitee_ids: Vec<String>,
    pub context: serde_json::Value,
    /// 可选：覆盖默认 SpaceRules
    #[serde(default)]
    pub rules: Option<crate::negotiation::rules::SpaceRulesOverrides>,
}

pub async fn rest_create_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RestCreateSpaceRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let create_req = crate::negotiation::CreateSpaceRequest {
        name: req.name,
        invitee_ids: req.invitee_ids.clone(),
        context: req.context,
    };
    let space = state
        .space_manager
        .create_space_with_rules(&agent, create_req, None, req.rules)
        .await?;
    super::ws::broadcast_space_created(&state, &agent, &space, &req.invitee_ids).await?;
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
    let agent = extract_agent(&state, &headers).await?;
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
    super::ws::broadcast_rfp_created(&state, &agent, &space, &req.provider_ids).await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(&space)?)))
}

/// POST /api/v1/spaces/:space_id/join — 加入 Space
pub async fn rest_join_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let space = state.space_manager.join_space(&agent, &space_id).await?;
    super::ws::broadcast_space_joined(&state, &space, &agent.id).await?;
    Ok(Json(serde_json::to_value(&space)?))
}

/// POST /api/v1/spaces/:space_id/join-approve — 审批加入请求
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
    let approver = extract_agent(&state, &headers).await?;
    let space = state
        .space_manager
        .approve_join_request(&approver.id, &space_id, &req.agent_id)
        .await?;
    super::ws::broadcast_space_joined(&state, &space, &req.agent_id).await?;
    Ok(Json(serde_json::to_value(&space)?))
}

/// POST /api/v1/spaces/:space_id/join-reject — 拒绝加入请求
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
    let rejector = extract_agent(&state, &headers).await?;
    let space = state
        .space_manager
        .reject_join_request(&rejector.id, &space_id, &req.agent_id)
        .await?;
    Ok(Json(serde_json::to_value(&space)?))
}

/// POST /api/v1/spaces/:space_id/leave — Agent 离开 Space
pub async fn rest_leave_space(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let space = state.space_manager.leave_space(&agent, &space_id).await?;
    super::ws::broadcast_space_left(&state, &space, &agent.id).await?;
    Ok(Json(serde_json::to_value(&space)?))
}

// ── Phase 13: Rules API ──────────────────────────────

/// GET /api/v1/spaces/:space_id/rules — 查看当前规则
pub async fn rest_get_rules(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
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
    let agent = extract_agent(&state, &headers).await?;
    let mut space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or_else(|| GaggleError::SpaceNotFound(space_id.clone()))?;

    // 权限检查：需要 can_change_rules
    let role = space.get_role(&agent.id).unwrap_or("member");
    if !space.rules.role_can_change_rules(role) {
        return Err(GaggleError::Forbidden(
            "Your role does not have permission to change rules".to_string(),
        ));
    }

    // 应用覆盖
    overrides.apply_to(&mut space.rules);
    space.updated_at = chrono::Utc::now().timestamp_millis();
    state.space_manager.persist_space(&space)?;
    state.space_manager.update_cache(&space).await;

    // 广播 rules_changed 事件
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

/// GET /api/v1/spaces/:space_id/rules/transitions — 查看规则演化计划
pub async fn rest_get_transitions(
    State(state): State<AppState>,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or_else(|| GaggleError::SpaceNotFound(space_id))?;
    Ok(Json(serde_json::to_value(&space.rules.transitions)?))
}

/// POST /api/v1/spaces/:space_id/messages — 发送消息
#[derive(Debug, Deserialize)]
pub struct RestSendMessageRequest {
    #[serde(default)]
    pub msg_type: Option<String>,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// 将字符串消息类型解析为 Gaggle MessageType
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
    let agent = extract_agent(&state, &headers).await?;
    let msg_type = parse_msg_type(req.msg_type.as_deref().unwrap_or("text"));
    let send_req = crate::negotiation::SendMessageRequest {
        msg_type,
        content: req.content,
        metadata: req.metadata,
    };
    let message = state.space_manager.send_message(&agent, &space_id, send_req).await?;
    super::ws::broadcast_new_message(&state, &space_id, &message).await?;
    Ok(Json(serde_json::to_value(&message)?))
}

/// POST /api/v1/spaces/:space_id/proposals — 提交提案
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
    let agent = extract_agent(&state, &headers).await?;
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
    super::ws::broadcast_new_proposal(&state, &space_id, &proposal).await?;
    Ok(Json(serde_json::to_value(&proposal)?))
}

/// POST /api/v1/spaces/:space_id/proposals/:proposal_id/respond — 响应提案
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
    let agent = extract_agent(&state, &headers).await?;
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

    // 广播提案更新
    let update_msg = serde_json::to_string(&super::ws::WsOutgoing::ProposalUpdate {
        space_id: space_id.clone(),
        payload: super::ws::ProposalUpdatePayload {
            proposal_id: updated_proposal.id.clone(),
            status: updated_proposal.status.as_str().to_string(),
            action: "responded".to_string(),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(update_msg.clone());
    }

    // 如果有反提案，也广播
    if let Some(counter) = counter_proposal {
        let counter_msg = serde_json::to_string(&super::ws::WsOutgoing::NewProposal {
            space_id: space_id.clone(),
            payload: super::ws::NewProposalPayload { proposal: counter },
        })?;
        if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
            let _ = tx.send(counter_msg);
        }
    }

    Ok(Json(serde_json::to_value(&updated_proposal)?))
}

/// POST /api/v1/spaces/:space_id/close — 关闭 Space
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
    let agent = extract_agent(&state, &headers).await?;
    let conclusion = req.conclusion.clone();
    let final_terms = req.final_terms.clone();
    let close_req = crate::negotiation::CloseSpaceRequest {
        conclusion: req.conclusion,
        final_terms: req.final_terms,
    };
    let space = state.space_manager.close_space(&agent, &space_id, close_req).await?;
    super::ws::broadcast_space_closed(&state, &space_id, &conclusion).await?;

    // ── Phase 4 自动化：concluded 时自动创建合同 + 证据上链 + 信誉记录 ──
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
        let messages = state.space_manager.get_messages(&space_id, None, 1000).await.unwrap_or_default();
        let session_hash = crate::solana::evidence::compute_session_hash(&messages);
        let _ = crate::solana::evidence::submit_evidence(
            &space_id,
            &crate::solana::evidence::EvidenceType::SessionHash,
            &session_hash,
        );

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

                            // 广播 ContractCreated
                            let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::ContractCreated {
                                space_id: space_id.clone(),
                                payload: super::ws::ContractCreatedPayload {
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
    let agent = extract_agent(&state, &headers).await?;
    state.space_manager.hard_delete_space(&agent, &space_id).await?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "deleted": true,
            "space_id": space_id,
        })),
    ))
}

// ==================== Phase 3: 评估 & 轮次 ====================

/// POST /api/v1/spaces/:space_id/proposals/evaluate — 加权评估提案
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
    let _agent = extract_agent(&state, &headers).await?;

    let weights = req.weights.unwrap_or_default();
    let response = state
        .space_manager
        .evaluate_proposals(&space_id, &weights)
        .await?;

    Ok(Json(serde_json::to_value(&response)?))
}

/// GET /api/v1/spaces/:space_id/rounds — 获取轮次信息
pub async fn rest_get_rounds(
    State(state): State<AppState>,
    _headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let round_info = state.space_manager.get_round_info(&space_id).await?;
    Ok(Json(serde_json::to_value(&round_info)?))
}

/// POST /api/v1/spaces/:space_id/rounds/advance — 推进轮次
pub async fn rest_advance_round(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let round_info = state
        .space_manager
        .advance_round(&agent, &space_id)
        .await?;

    // 广播轮次推进事件
    let msg = serde_json::to_string(&super::ws::WsOutgoing::RoundAdvanced {
        space_id: space_id.clone(),
        payload: super::ws::RoundAdvancedPayload {
            new_round: round_info.current_round,
            round_status: round_info.round_status.as_str().to_string(),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(msg);
    }

    Ok(Json(serde_json::to_value(&round_info)?))
}

/// POST /api/v1/needs/:need_id/create-rfp — 从 Need 创建 RFP Space
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
    let agent = extract_agent(&state, &headers).await?;

    // 获取 Need
    let need = state
        .discovery_store
        .get_need(&need_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Need not found: {}", need_id)))?;

    // 验证调用者是 creator
    if need.creator_id != agent.id {
        return Err(GaggleError::Forbidden(
            "Only the need creator can create RFP".to_string(),
        ));
    }

    // 验证 Need 状态
    if need.status != crate::discovery::NeedStatus::Open {
        return Err(GaggleError::ValidationError(
            format!("Need is not open (status: {:?})", need.status),
        ));
    }

    // 创建 RFP Context
    let rfp_context = crate::negotiation::RfpContext {
        allowed_rounds: req.allowed_rounds,
        evaluation_criteria: req.evaluation_criteria,
        deadline: req.deadline,
        share_best_terms: req.share_best_terms,
    };

    // 用 Need 信息构建 Space context
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

    // 更新 Need 状态为 Matched
    state
        .discovery_store
        .update_need_status(&need_id, &crate::discovery::NeedStatus::Matched)
        .await?;

    // 广播 RFP 创建
    super::ws::broadcast_rfp_created(&state, &agent, &space, &req.provider_ids).await?;

    Ok(Json(serde_json::json!({
        "space": space,
        "need_status": "matched",
    })))
}

// ==================== Phase 4: 执行引擎 — 合同 & 里程碑 ====================

/// POST /api/v1/spaces/:space_id/contract — 从已成交 Space 创建合同
pub async fn rest_create_contract(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
    Json(req): Json<crate::execution::CreateContractRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;

    let space = state
        .space_manager
        .get_space(&space_id)
        .await?
        .ok_or_else(|| GaggleError::SpaceNotFound(space_id.clone()))?;

    // 验证调用者是 space 成员
    if space.buyer_id.as_deref() != Some(&agent.id) && space.seller_id.as_deref() != Some(&agent.id) {
        return Err(GaggleError::Forbidden("Not a member of this space".to_string()));
    }

    // 检查是否已有合同
    if let Some(existing) = state.execution_store.get_contract_by_space(&space_id).await? {
        return Err(GaggleError::ValidationError(
            format!("Contract already exists for this space: {}", existing.id),
        ));
    }

    // Space 没有 final_terms 字段 — 用空 JSON
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

    // 广播 ContractCreated
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::ContractCreated {
        space_id: space_id.clone(),
        payload: super::ws::ContractCreatedPayload {
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

/// GET /api/v1/contracts/:contract_id — 获取合同详情
pub async fn rest_get_contract(
    State(state): State<AppState>,
    _headers: axum::http::HeaderMap,
    Path(contract_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let contract = state
        .execution_store
        .get_contract(&contract_id)
        .await?
        .ok_or_else(|| GaggleError::NotFound(format!("Contract not found: {}", contract_id)))?;
    Ok(Json(serde_json::to_value(&contract)?))
}

/// GET /api/v1/agents/:agent_id/contracts — 获取 Agent 的所有合同
pub async fn rest_get_agent_contracts(
    State(state): State<AppState>,
    _headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let contracts = state.execution_store.get_agent_contracts(&agent_id).await?;
    Ok(Json(serde_json::to_value(&contracts)?))
}

/// POST /api/v1/contracts/:contract_id/milestones/:milestone_id/submit — Provider 提交交付物
pub async fn rest_submit_milestone(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((contract_id, milestone_id)): Path<(String, String)>,
    Json(req): Json<crate::execution::SubmitMilestoneRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;

    // 验证 agent 是 seller
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

    // 广播 MilestoneSubmitted
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::MilestoneSubmitted {
        contract_id: contract_id.clone(),
        payload: super::ws::MilestoneSubmittedPayload {
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

/// POST /api/v1/contracts/:contract_id/milestones/:milestone_id/accept — Consumer 验收/拒绝
pub async fn rest_accept_milestone(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((contract_id, milestone_id)): Path<(String, String)>,
    Json(req): Json<crate::execution::AcceptMilestoneRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;

    // 验证 agent 是 buyer
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

    // 广播 MilestoneAccepted
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::MilestoneAccepted {
        contract_id: contract_id.clone(),
        payload: super::ws::MilestoneAcceptedPayload {
            milestone_id: milestone.id.clone(),
            milestone_title: milestone.title.clone(),
            accepted: req.accepted,
            accepted_at: milestone.accepted_at.unwrap_or(0),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&contract.space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    // 检查合同是否完成 — 重新查询获取最新状态
    let updated_contract = state.execution_store.get_contract(&contract_id).await?;
    if let Some(ref c) = updated_contract {
        if c.status == crate::execution::ContractStatus::Completed {
            let completed_msg = serde_json::to_string(&super::ws::WsOutgoing::ContractCompleted {
                contract_id: contract_id.clone(),
            })?;
            if let Some(tx) = state.space_manager.get_broadcast_tx(&c.space_id).await {
                let _ = tx.send(completed_msg);
            }
        }
    }

    Ok(Json(serde_json::to_value(&milestone)?))
}

/// POST /api/v1/contracts/:contract_id/dispute — 发起争议
pub async fn rest_dispute_contract(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(contract_id): Path<String>,
    Json(_req): Json<crate::execution::DisputeContractRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;

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

// ── Phase 9: SubSpace REST API ──────────────────────────

/// POST /api/v1/spaces/:space_id/subspaces — 创建子空间
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
    let agent = extract_agent(&state, &headers).await?;

    let sub = state.space_manager.create_subspace(
        &space_id,
        &agent.id,
        crate::negotiation::subspace::CreateSubSpaceRequest {
            name: req.name,
            agent_ids: req.agent_ids,
            rules: req.rules,
        },
    ).await?;

    // 广播到父空间的 channel
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::SubSpaceCreated {
        parent_space_id: space_id.clone(),
        sub_space: sub.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&sub)?))
}

/// GET /api/v1/spaces/:space_id/subspaces — 列出子空间
pub async fn rest_list_subspaces(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = extract_agent(&state, &headers).await?;

    let subs = state.space_manager.list_subspaces(&space_id).await?;
    Ok(Json(serde_json::json!({ "subspaces": subs })))
}

/// GET /api/v1/subspaces/:sub_space_id — 获取子空间详情
pub async fn rest_get_subspace(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = extract_agent(&state, &headers).await?;

    let sub = state.space_manager.get_subspace(&sub_space_id).await?
        .ok_or_else(|| GaggleError::NotFound(format!("Sub-space not found: {}", sub_space_id)))?;
    Ok(Json(serde_json::to_value(&sub)?))
}

/// POST /api/v1/subspaces/:sub_space_id/messages — 子空间发消息
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
    let agent = extract_agent(&state, &headers).await?;

    let msg = state.space_manager.send_subspace_message(
        &sub_space_id,
        &agent.id,
        req.msg_type,
        &req.content,
        req.metadata,
    ).await?;

    // 广播到子空间 channel
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::SubSpaceMessage {
        sub_space_id: sub_space_id.clone(),
        message: msg.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_subspace_broadcast_tx(&sub_space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&msg)?))
}

/// GET /api/v1/subspaces/:sub_space_id/messages — 获取子空间消息
pub async fn rest_get_subspace_messages(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = extract_agent(&state, &headers).await?;

    let messages = state.space_manager.get_subspace_messages(&sub_space_id).await?;
    Ok(Json(serde_json::json!({ "messages": messages })))
}

/// POST /api/v1/subspaces/:sub_space_id/proposals — 子空间提提案
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
    let agent = extract_agent(&state, &headers).await?;

    let proposal = state.space_manager.submit_subspace_proposal(
        &sub_space_id,
        &agent.id,
        req.proposal_type,
        req.dimensions,
    ).await?;

    // 广播到子空间 channel
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::SubSpaceProposal {
        sub_space_id: sub_space_id.clone(),
        proposal: proposal.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_subspace_broadcast_tx(&sub_space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&proposal)?))
}

/// GET /api/v1/subspaces/:sub_space_id/proposals — 获取子空间提案
pub async fn rest_get_subspace_proposals(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = extract_agent(&state, &headers).await?;

    let proposals = state.space_manager.get_subspace_proposals(&sub_space_id).await?;
    Ok(Json(serde_json::json!({ "proposals": proposals })))
}

/// POST /api/v1/subspaces/:sub_space_id/close — 关闭子空间
#[derive(Debug, Deserialize)]
pub struct RestCloseSubSpaceRequest {
    pub conclusion: String, // "concluded" or "cancelled"
}

pub async fn rest_close_subspace(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(sub_space_id): Path<String>,
    Json(req): Json<RestCloseSubSpaceRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let concluded = req.conclusion == "concluded";

    let sub = state.space_manager.close_subspace(&sub_space_id, &agent.id, concluded).await?;

    // 广播到子空间和父空间
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::SubSpaceClosed {
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

// ── Phase 10: Coalition REST API ────────────────────────

/// POST /api/v1/spaces/:space_id/coalitions — 创建联盟
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
    let agent = extract_agent(&state, &headers).await?;

    let coalition = state.space_manager.create_coalition(
        &space_id,
        &agent.id,
        crate::negotiation::coalition::CreateCoalitionRequest {
            name: req.name,
            member_ids: req.member_ids,
            stance: req.stance,
        },
    ).await?;

    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::CoalitionCreated {
        space_id: space_id.clone(),
        coalition: coalition.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&coalition)?))
}

/// GET /api/v1/spaces/:space_id/coalitions — 列出联盟
pub async fn rest_list_coalitions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = extract_agent(&state, &headers).await?;
    let coalitions = state.space_manager.list_coalitions(&space_id).await?;
    Ok(Json(serde_json::json!({ "coalitions": coalitions })))
}

/// GET /api/v1/coalitions/:coalition_id — 获取联盟详情
pub async fn rest_get_coalition(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.get_coalition(&coalition_id).await?
        .ok_or_else(|| GaggleError::NotFound(format!("Coalition not found: {}", coalition_id)))?;
    Ok(Json(serde_json::to_value(&coalition)?))
}

/// POST /api/v1/coalitions/:coalition_id/join — 加入联盟
pub async fn rest_join_coalition(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.join_coalition(&coalition_id, &agent.id).await?;
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::CoalitionMemberJoined {
        coalition_id: coalition_id.clone(),
        agent_id: agent.id.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&coalition.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&coalition)?))
}

/// POST /api/v1/coalitions/:coalition_id/leave — 离开联盟
pub async fn rest_leave_coalition(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.leave_coalition(&coalition_id, &agent.id).await?;
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::CoalitionMemberLeft {
        coalition_id: coalition_id.clone(),
        agent_id: agent.id.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&coalition.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&coalition)?))
}

/// PUT /api/v1/coalitions/:coalition_id/stance — 更新联盟立场
pub async fn rest_update_coalition_stance(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
    Json(req): Json<crate::negotiation::UpdateStanceRequest>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.update_coalition_stance(
        &coalition_id, &agent.id, req.stance,
    ).await?;
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::CoalitionStanceUpdated {
        coalition_id: coalition_id.clone(),
        stance: coalition.stance.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&coalition.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&coalition)?))
}

/// POST /api/v1/coalitions/:coalition_id/disband — 解散联盟
pub async fn rest_disband_coalition(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(coalition_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let coalition = state.space_manager.disband_coalition(&coalition_id, &agent.id).await?;
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::CoalitionDisbanded {
        coalition_id: coalition_id.clone(),
        space_id: coalition.space_id.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&coalition.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&coalition)?))
}

// ── Phase 11: Delegation REST API ───────────────────────

/// POST /api/v1/spaces/:space_id/delegations — 创建委托
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
    let agent = extract_agent(&state, &headers).await?;
    let delegation = state.space_manager.create_delegation(
        &agent.id,
        crate::negotiation::delegation::CreateDelegationRequest {
            delegate_id: req.delegate_id,
            space_id: space_id.clone(),
            scope: req.scope,
            expires_at: req.expires_at,
        },
    ).await?;
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::DelegationCreated {
        delegation: delegation.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&delegation)?))
}

/// GET /api/v1/spaces/:space_id/delegations — 列出空间委托
pub async fn rest_list_delegations(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = extract_agent(&state, &headers).await?;
    let delegations = state.space_manager.list_delegations(&space_id).await?;
    Ok(Json(serde_json::json!({ "delegations": delegations })))
}

/// DELETE /api/v1/delegations/:delegation_id — 撤销委托
pub async fn rest_revoke_delegation(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(delegation_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let delegation = state.space_manager.revoke_delegation(&delegation_id, &agent.id).await?;
    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::DelegationRevoked {
        delegation_id: delegation.id.clone(),
        space_id: delegation.space_id.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&delegation.space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(Json(serde_json::to_value(&delegation)?))
}

/// GET /api/v1/agents/:agent_id/delegations — 列出 agent 的委托（作为委托人）
pub async fn rest_list_agent_delegations(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = extract_agent(&state, &headers).await?;
    let delegations = state.space_manager.list_delegations_by_delegator(&agent_id).await?;
    Ok(Json(serde_json::json!({ "delegations": delegations })))
}

// ── Phase 12: Recruitment REST API ──────────────────────

/// POST /api/v1/spaces/:space_id/recruit — 发起招募
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
    let agent = extract_agent(&state, &headers).await?;
    let recruitment = state.space_manager.create_recruitment(
        &space_id,
        &agent.id,
        crate::negotiation::recruitment::CreateRecruitmentRequest {
            target_id: req.target_id,
            role: req.role,
            pitch: req.pitch,
        },
    ).await?;

    let broadcast_msg = serde_json::to_string(&super::ws::WsOutgoing::RecruitmentCreated {
        recruitment: recruitment.clone(),
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let _ = tx.send(broadcast_msg);
    }

    Ok(Json(serde_json::to_value(&recruitment)?))
}

/// POST /api/v1/spaces/:space_id/recruit/:recruitment_id/accept — 接受招募
pub async fn rest_accept_recruitment(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((space_id, recruitment_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;

    // 1. 获取招募请求
    let mut recruitment = state.space_manager.get_recruitment(&recruitment_id).await?
        .ok_or_else(|| GaggleError::NotFound(format!("Recruitment not found: {}", recruitment_id)))?;

    if recruitment.status != crate::negotiation::RecruitmentStatus::Pending {
        return Err(GaggleError::ValidationError("Recruitment is no longer pending".to_string()));
    }
    if recruitment.target_id != agent.id {
        return Err(GaggleError::Forbidden("Only the target can accept".to_string()));
    }

    // 2. 标记接受
    recruitment.accept();

    // 3. 获取并更新 Space
    let space = state.space_manager.accept_recruitment_join(&recruitment_id, &agent.id, &recruitment).await?;

    // 4. 广播
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
        let msg = serde_json::to_string(&super::ws::WsOutgoing::RecruitmentAccepted {
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

/// POST /api/v1/spaces/:space_id/recruit/:recruitment_id/reject — 拒绝招募
pub async fn rest_reject_recruitment(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path((_space_id, recruitment_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let agent = extract_agent(&state, &headers).await?;
    let recruitment = state.space_manager.reject_recruitment(&recruitment_id, &agent.id).await?;
    Ok(Json(serde_json::to_value(&recruitment)?))
}

/// GET /api/v1/spaces/:space_id/recruitments — 列出招募记录
pub async fn rest_list_recruitments(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(space_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _agent = extract_agent(&state, &headers).await?;
    let recruitments = state.space_manager.list_recruitments(&space_id).await?;
    Ok(Json(serde_json::json!({ "recruitments": recruitments })))
}
