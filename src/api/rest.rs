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
    DiscoveryProfile, DiscoveryStore, ProviderSearchQuery, UpdateProfileRequest,
};
use crate::error::GaggleError;
use crate::negotiation::{Space, SpaceManager, SpaceMessage};
use crate::reputation::{
    calculator::ReputationCalculator, EventType, Outcome, RateResponse, ReputationDetail,
    ReputationEvent, ReputationStore,
};
use crate::users::{
    User, UserLoginRequest, UserLoginResponse, UserRegisterRequest, UserRegisterResponse, UserStore,
};

/// 在线连接元信息
#[derive(Clone)]
pub struct ConnectionInfo {
    pub tx: broadcast::Sender<String>,
    pub connected_since: i64,
    pub last_ping: i64,
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
    Path(space_id): Path<String>,
    Query(query): Query<MessagesQuery>,
) -> Result<Json<Vec<SpaceMessage>>, GaggleError> {
    let limit = query.limit.unwrap_or(50).min(100);
    let messages = state
        .space_manager
        .get_messages(&space_id, query.after, limit)
        .await?;
    Ok(Json(messages))
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

// ==================== Discovery API ====================

/// 搜索 Provider
pub async fn search_providers(
    State(state): State<AppState>,
    Query(query): Query<ProviderSearchQuery>,
) -> Result<Json<Vec<DiscoveryProfile>>, GaggleError> {
    let profiles = state.discovery_store.search_providers(&query).await?;
    Ok(Json(profiles))
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
                "last_ping": conns.iter().map(|c| c.last_ping).max(),
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
