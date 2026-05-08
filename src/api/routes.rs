//! 路由定义

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use tower_http::cors::{Any, CorsLayer};

use super::middleware::{RateLimitConfig, RateLimitState};
use super::rest::AppState;
use super::{health, openclaw, ws};

pub fn create_router(
    state: AppState,
    rate_limit_rpm: u32,
) -> Router {
    // CORS: 从环境变量读取允许的 origins，默认仍允许所有（开发模式）
    let cors = if let Ok(origins) = std::env::var("CORS_ORIGINS") {
        let parsed: Vec<_> = origins
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if parsed.is_empty() {
            CorsLayer::very_permissive()
        } else {
            CorsLayer::new()
                .allow_origin(parsed)
                .allow_methods(Any)
                .allow_headers([AUTHORIZATION, CONTENT_TYPE])
        }
    } else {
        CorsLayer::very_permissive()
    };

    // 速率限制配置 (RPM -> 60秒窗口)
    // 120 RPM = 2 请求/秒 = 120 请求/60秒
    let max_requests = rate_limit_rpm;
    let window_seconds = 60u64;

    let rate_limit_config = RateLimitConfig {
        max_requests,
        window_seconds,
    };

    let rate_limit_state = RateLimitState::new();

    Router::new()
        // 健康检查（不限流）
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
        // Space — 创建（REST POST）
        .route("/api/v1/spaces", post(rest::rest_create_space))
        .route("/api/v1/spaces/rfp", post(rest::rest_create_rfp))
        // Space — 读取
        .route("/api/v1/spaces/:space_id", get(rest::get_space))
        .route("/api/v1/spaces/:space_id", delete(rest::delete_space))
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
        // Space — 写操作（REST POST）
        .route("/api/v1/spaces/:space_id/join", post(rest::rest_join_space))
        .route("/api/v1/spaces/:space_id/join-approve", post(rest::rest_join_approve))
        .route("/api/v1/spaces/:space_id/join-reject", post(rest::rest_join_reject))
        .route("/api/v1/spaces/:space_id/leave", post(rest::rest_leave_space))
        // Phase 13: Rules API
        .route("/api/v1/spaces/:space_id/rules", get(rest::rest_get_rules))
        .route("/api/v1/spaces/:space_id/rules", put(rest::rest_update_rules))
        .route(
            "/api/v1/spaces/:space_id/rules/transitions",
            get(rest::rest_get_transitions),
        )
        .route("/api/v1/spaces/:space_id/send", post(rest::rest_send_message))
        .route("/api/v1/spaces/:space_id/proposals/submit", post(rest::rest_submit_proposal))
        .route(
            "/api/v1/spaces/:space_id/proposals/:proposal_id/respond",
            post(rest::rest_respond_to_proposal),
        )
        // Phase 3: 评估 & 轮次
        .route(
            "/api/v1/spaces/:space_id/proposals/evaluate",
            post(rest::rest_evaluate_proposals),
        )
        .route(
            "/api/v1/spaces/:space_id/rounds",
            get(rest::rest_get_rounds),
        )
        .route(
            "/api/v1/spaces/:space_id/rounds/advance",
            post(rest::rest_advance_round),
        )
        .route("/api/v1/spaces/:space_id/close", post(rest::rest_close_space))
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
        // Need Broadcast
        .route("/api/v1/needs", post(rest::publish_need))
        .route("/api/v1/needs", get(rest::search_needs))
        .route("/api/v1/needs/my", get(rest::get_my_needs))
        .route("/api/v1/needs/:need_id", get(rest::get_need))
        .route("/api/v1/needs/:need_id/cancel", post(rest::cancel_need))
        .route(
            "/api/v1/needs/:need_id/create-rfp",
            post(rest::rest_create_rfp_from_need),
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
        // Phase 4: 执行引擎 — 合同 & 里程碑
        .route(
            "/api/v1/spaces/:space_id/contract",
            post(rest::rest_create_contract),
        )
        .route(
            "/api/v1/contracts/:contract_id",
            get(rest::rest_get_contract),
        )
        .route(
            "/api/v1/agents/:agent_id/contracts",
            get(rest::rest_get_agent_contracts),
        )
        .route(
            "/api/v1/contracts/:contract_id/milestones/:milestone_id/submit",
            post(rest::rest_submit_milestone),
        )
        .route(
            "/api/v1/contracts/:contract_id/milestones/:milestone_id/accept",
            post(rest::rest_accept_milestone),
        )
        .route(
            "/api/v1/contracts/:contract_id/dispute",
            post(rest::rest_dispute_contract),
        )
        // Phase 5: 模板市场
        .route("/api/v1/templates", get(rest::list_templates))
        .route(
            "/api/v1/templates/:template_id",
            get(rest::get_template),
        )
        // Phase 5: 市场信息中心
        .route("/api/v1/market", get(rest::get_all_market_prices))
        .route("/api/v1/market/:category", get(rest::get_market_prices))
        .route("/api/v1/market/share", post(rest::share_market_price))
        .route(
            "/api/v1/market/:category/contributions",
            get(rest::get_market_contributions),
        )
        // WebSocket — Gaggle 原生（带 token 鉴权）
        .route("/ws/v1/agents/:agent_id", get(ws::websocket_handler))
        // WebSocket — OpenClaw 兼容 Gateway
        .route("/ws/v1/gateway", get(openclaw::gateway_handler))
        // Phase 9: SubSpace
        .route("/api/v1/spaces/:space_id/subspaces", post(rest::rest_create_subspace))
        .route("/api/v1/spaces/:space_id/subspaces", get(rest::rest_list_subspaces))
        .route("/api/v1/subspaces/:sub_space_id", get(rest::rest_get_subspace))
        .route("/api/v1/subspaces/:sub_space_id/messages", post(rest::rest_subspace_send_message))
        .route("/api/v1/subspaces/:sub_space_id/messages", get(rest::rest_get_subspace_messages))
        .route("/api/v1/subspaces/:sub_space_id/proposals", post(rest::rest_subspace_submit_proposal))
        .route("/api/v1/subspaces/:sub_space_id/proposals", get(rest::rest_get_subspace_proposals))
        .route("/api/v1/subspaces/:sub_space_id/close", post(rest::rest_close_subspace))
        // Phase 10: Coalitions
        .route("/api/v1/spaces/:space_id/coalitions", post(rest::rest_create_coalition))
        .route("/api/v1/spaces/:space_id/coalitions", get(rest::rest_list_coalitions))
        .route("/api/v1/coalitions/:coalition_id", get(rest::rest_get_coalition))
        .route("/api/v1/coalitions/:coalition_id/join", post(rest::rest_join_coalition))
        .route("/api/v1/coalitions/:coalition_id/leave", post(rest::rest_leave_coalition))
        .route("/api/v1/coalitions/:coalition_id/stance", put(rest::rest_update_coalition_stance))
        .route("/api/v1/coalitions/:coalition_id/disband", post(rest::rest_disband_coalition))
        // Phase 11: Delegations
        .route("/api/v1/spaces/:space_id/delegations", post(rest::rest_create_delegation))
        .route("/api/v1/spaces/:space_id/delegations", get(rest::rest_list_delegations))
        .route("/api/v1/delegations/:delegation_id", delete(rest::rest_revoke_delegation))
        .route("/api/v1/agents/:agent_id/delegations", get(rest::rest_list_agent_delegations))
        // Phase 12: Recruitment
        .route("/api/v1/spaces/:space_id/recruit", post(rest::rest_create_recruitment))
        .route("/api/v1/spaces/:space_id/recruit/:recruitment_id/accept", post(rest::rest_accept_recruitment))
        .route("/api/v1/spaces/:space_id/recruit/:recruitment_id/reject", post(rest::rest_reject_recruitment))
        .route("/api/v1/spaces/:space_id/recruitments", get(rest::rest_list_recruitments))
        // Phase 14: Shared Reality Layer
        .route("/api/v1/spaces/:space_id/state", get(rest::rest_get_shared_state))
        .route("/api/v1/spaces/:space_id/reality-sync", get(rest::rest_get_reality_alignment))
        .route("/api/v1/spaces/:space_id/state/:key", get(rest::rest_get_state_key))
        .route("/api/v1/spaces/:space_id/state/:key", put(rest::rest_set_state_key))
        .route("/api/v1/spaces/:space_id/state/:key", delete(rest::rest_delete_state_key))
        .route("/api/v1/spaces/:space_id/events", get(rest::rest_get_state_events))
        .route("/api/v1/spaces/:space_id/state/reconstruct/:version", get(rest::rest_reconstruct_state))
        .route("/api/v1/spaces/:space_id/state/verify-chain", get(rest::rest_verify_chain))
        .route("/api/v1/spaces/:space_id/state/integrity", get(rest::rest_state_integrity))
        // Trace / Observability
        .route("/api/v1/spaces/:space_id/trace", get(rest::rest_get_trace))
        .route("/api/v1/events/queue-stats", get(rest::rest_queue_stats))
        // Event Queue Admin
        .route("/api/v1/events/dead-letters", get(rest::rest_list_dead_letters))
        .route("/api/v1/events/dead-letters/cleanup", post(rest::rest_cleanup_dead_letters))
        .route("/api/v1/events/dead-letters/:event_id/retry", post(rest::rest_retry_dead_letter))
        .with_state(state)
        .layer(
            // 应用速率限制中间件到所有路由（除了 /health）
            axum::middleware::from_fn_with_state(
                rate_limit_state.clone(),
                super::middleware::rate_limit_middleware,
            ),
        )
        .layer(axum::extract::Extension(rate_limit_config))
        .layer(cors)
}

mod rest {
    pub use super::super::rest::*;
}
