//! Agent CRUD + Status endpoints

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use crate::agents::{RegisterRequest, RegisterResponse, UpdateAgentRequest};
use crate::error::GaggleError;
use crate::negotiation::Space;
use crate::users::{
    User, UserLoginRequest, UserLoginResponse, UserRegisterRequest, UserRegisterResponse,
};

use super::{extract_user, AppState};

// ── IP-level registration rate limit ─────────────────────────
use std::sync::LazyLock;
use dashmap::DashMap;

/// Tracks (registration_count, window_start_timestamp_ms) per IP.
const IP_REG_LIMIT: u32 = 5;      // max registrations per IP per window
const IP_REG_WINDOW_SECS: u64 = 3600; // 1 hour

static IP_REG_COUNTERS: LazyLock<DashMap<String, (u32, i64)>> = LazyLock::new(DashMap::new);

fn check_ip_reg_limit(ip: &str) -> Result<(), GaggleError> {
    let now = chrono::Utc::now().timestamp_millis();
    let window_ms = (IP_REG_WINDOW_SECS * 1000) as i64;

    IP_REG_COUNTERS.entry(ip.to_string()).and_modify(|(count, start)| {
        if now - *start > window_ms {
            *count = 1;
            *start = now;
        } else {
            *count += 1;
        }
    }).or_insert((1, now));

    if let Some(entry) = IP_REG_COUNTERS.get(ip) {
        let (count, start) = entry.value();
        if now - *start <= window_ms && *count > IP_REG_LIMIT {
            return Err(GaggleError::ValidationError(
                format!("Registration rate limit exceeded for this IP (max {IP_REG_LIMIT}/hour)")
            ));
        }
    }
    Ok(())
}

fn extract_client_ip(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("unknown").trim())
        .unwrap_or("unknown")
        .to_string()
}

// ── User endpoints ──

pub async fn register_user(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UserRegisterRequest>,
) -> Result<(StatusCode, Json<UserRegisterResponse>), GaggleError> {
    let ip = extract_client_ip(&headers);
    check_ip_reg_limit(&ip)?;
    let response = state.user_store.register(req).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn login_user(
    State(state): State<AppState>,
    Json(req): Json<UserLoginRequest>,
) -> Result<Json<UserLoginResponse>, GaggleError> {
    let response = state.user_store.login(req).await?;
    Ok(Json(response))
}

pub async fn get_me(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<User>, GaggleError> {
    let user = extract_user(&state, &headers).await?;
    Ok(Json(user))
}

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
    all_spaces.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(Json(all_spaces))
}

// ── Agent registration + CRUD ──

pub async fn register_agent(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), GaggleError> {
    let user = extract_user(&state, &headers).await?;

    let max_agents_per_user: usize = std::env::var("MAX_AGENTS_PER_USER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let agent_count = state.user_store.count_agents(&user.id).await?;
    if agent_count >= max_agents_per_user {
        return Err(GaggleError::Forbidden(format!(
            "Agent limit reached (max {})",
            max_agents_per_user
        )));
    }

    let response = state.registry.register(req, Some(user.id)).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// Agent 公开信息（不含 api_key / api_secret_hash / callback_url / user_id）
#[derive(Debug, serde::Serialize)]
pub struct AgentPublic {
    pub id: String,
    pub agent_type: crate::agents::AgentType,
    pub name: String,
    pub metadata: serde_json::Value,
    pub public_key: Option<String>,
    pub created_at: i64,
    pub disabled_at: Option<i64>,
    pub organization: Option<String>,
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
            disabled_at: agent.disabled_at,
            organization: agent.organization.clone(),
            online,
        }
    }
}

/// GET /api/v1/agents/:agent_id — 获取单个 Agent 公开信息
pub async fn get_agent(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentPublic>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;

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

/// DELETE /api/v1/agents/:agent_id — 软删除 Agent
pub async fn delete_agent(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), GaggleError> {
    super::verify_agent_ownership(&state, &headers, &agent_id).await?;

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
    super::verify_agent_ownership(&state, &headers, &req.agent_id).await?;

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

/// GET /api/v1/agents/:agent_id/status — 查询 Agent 在线状态
pub async fn get_agent_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>, GaggleError> {
    let _ = super::extract_any_auth(&state, &headers).await?;
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
