//! 路由定义

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use super::rest::AppState;
use super::{health, openclaw, ws};

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // 健康检查
        .route("/health", get(health::health_check))
        // 用户账户
        .route("/api/v1/users/register", post(rest::register_user))
        .route("/api/v1/users/login", post(rest::login_user))
        .route("/api/v1/users/me", get(rest::get_me))
        .route("/api/v1/users/me/agents", get(rest::get_my_agents))
        .route("/api/v1/user/spaces", get(rest::get_user_spaces))
        // Agent 注册 + CRUD
        .route("/api/v1/agents/register", post(rest::register_agent))
        .route("/api/v1/agents/:agent_id", get(rest::get_agent))
        .route(
            "/api/v1/agents/:agent_id/disable",
            post(rest::delete_agent),
        )
        .route(
            "/api/v1/agents/update",
            post(rest::update_agent),
        )
        // Space
        .route("/api/v1/spaces/:space_id", get(rest::get_space))
        .route(
            "/api/v1/agents/:agent_id/spaces",
            get(rest::list_agent_spaces),
        )
        .route(
            "/api/v1/spaces/:space_id/messages",
            get(rest::get_space_messages),
        )
        .route(
            "/api/v1/spaces/:space_id/proposals",
            get(rest::get_space_proposals),
        )
        .route(
            "/api/v1/spaces/:space_id/members",
            get(rest::get_space_members),
        )
        .route(
            "/api/v1/spaces/:space_id/evidence",
            post(rest::submit_evidence),
        )
        // Provider Discovery
        .route("/api/v1/providers/search", get(rest::search_providers))
        .route(
            "/api/v1/providers/:agent_id/profile",
            get(rest::get_provider_profile),
        )
        .route(
            "/api/v1/providers/me/profile",
            axum::routing::put(rest::update_provider_profile),
        )
        // Reputation
        .route(
            "/api/v1/agents/:agent_id/reputation",
            get(rest::get_agent_reputation),
        )
        .route("/api/v1/spaces/:space_id/rate", post(rest::rate_agent))
        // Agent Status
        .route(
            "/api/v1/agents/:agent_id/status",
            get(rest::get_agent_status),
        )
        // WebSocket — Gaggle 原生（带 token 鉴权）
        .route("/ws/v1/agents/:agent_id", get(ws::websocket_handler))
        // WebSocket — OpenClaw 兼容 Gateway
        .route("/ws/v1/gateway", get(openclaw::gateway_handler))
        .with_state(state)
        .layer(cors)
}

mod rest {
    pub use super::super::rest::*;
}
