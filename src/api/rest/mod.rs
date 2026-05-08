//! REST API — shared types, auth helpers, and domain module re-exports

mod rest_agents;
mod rest_spaces;
mod rest_market;
mod rest_institution;

// Re-export everything so `use crate::api::rest::*` still works unchanged
pub use rest_agents::*;
pub use rest_spaces::*;
pub use rest_market::*;
pub use rest_institution::*;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::agents::AgentRegistry;
use crate::api::event_queue::EventQueue;
use crate::discovery::DiscoveryStore;
use crate::error::GaggleError;
use crate::execution::ExecutionStore;
use crate::marketplace::MarketplaceStore;
use crate::negotiation::SpaceManager;
use crate::reputation::ReputationStore;
use crate::users::UserStore;

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
    pub shared_state_manager: Arc<crate::negotiation::SharedStateManager>,
    pub user_store: Arc<UserStore>,
    pub discovery_store: Arc<DiscoveryStore>,
    pub reputation_store: Arc<ReputationStore>,
    pub execution_store: Arc<ExecutionStore>,
    pub marketplace_store: Arc<MarketplaceStore>,
    /// 在线 Agent 连接表
    pub online_agents: OnlineAgents,
    /// 离线事件队列
    pub event_queue: Arc<EventQueue>,
    /// Space 审计追踪
    pub trace_store: Arc<crate::api::trace::TraceStore>,
}

// ── Auth helpers (shared across all REST sub-modules) ──

/// 从 Bearer token 提取并验证用户（必须）
pub(crate) async fn extract_user(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<crate::users::User, GaggleError> {
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

/// 灵活 auth：接受 agent token (gag_) 或 user token (usr_)
/// 用于需要认证但不需要特定身份的端点（如搜索、浏览）
pub(crate) async fn extract_any_auth(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<String, GaggleError> {
    let token = extract_bearer(headers)?;
    if token.starts_with("gag_") {
        state
            .registry
            .get_by_api_key(&token)
            .await?
            .ok_or_else(|| GaggleError::Unauthorized("Invalid agent API key".to_string()))?;
        Ok(token)
    } else if token.starts_with("usr_") {
        state
            .user_store
            .get_by_api_key(&token)
            .await?
            .ok_or_else(|| GaggleError::Unauthorized("Invalid user API key".to_string()))?;
        Ok(token)
    } else {
        Err(GaggleError::Unauthorized("Invalid token format".to_string()))
    }
}

/// 从 Authorization header 提取 Bearer token
pub(crate) fn extract_bearer(headers: &axum::http::HeaderMap) -> Result<String, GaggleError> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").map(|s| s.to_string()))
        .ok_or_else(|| {
            GaggleError::Unauthorized("Missing or invalid Authorization header".to_string())
        })
}

/// 鉴权辅助：从 Bearer token 提取并验证 Agent（gag_ key）
pub(crate) async fn extract_agent(
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

/// 轻量鉴权：只提取 agent_id（不验证完整 agent）
pub(crate) async fn extract_agent_id(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<String, GaggleError> {
    let agent = extract_agent(state, headers).await?;
    Ok(agent.id)
}

/// Verify that the agent is a member of the space
pub(crate) async fn verify_space_member(
    state: &AppState,
    space_id: &str,
    agent_id: &str,
) -> Result<(), GaggleError> {
    let space = state
        .space_manager
        .get_space(space_id)
        .await?
        .ok_or_else(|| GaggleError::SpaceNotFound(space_id.to_string()))?;

    if !space.agent_ids.contains(&agent_id.to_string()) {
        return Err(GaggleError::Forbidden(
            "Agent is not a member of this space".to_string(),
        ));
    }
    Ok(())
}

/// 鉴权辅助：验证操作者是否有权操作指定 Agent
/// 支持 usr_（资源所有者）和 gag_（Agent 自身）两种方式
pub(crate) async fn verify_agent_ownership(
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
