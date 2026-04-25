//! 健康检查端点

use axum::{extract::State, Json};
use chrono::Utc;
use serde::Serialize;

use super::rest::AppState;

/// GET /health 响应体
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_seconds: i64,
    pub spaces_count: usize,
    pub agents_count: usize,
    pub online_agents: usize,
}

/// 服务器启动时间，在 main.rs 中初始化
pub static SERVER_START: std::sync::OnceLock<i64> = std::sync::OnceLock::new();

/// GET /health
pub async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    let started_at = SERVER_START
        .get()
        .copied()
        .unwrap_or_else(|| Utc::now().timestamp());
    let uptime = Utc::now().timestamp() - started_at;

    let online_count = state.online_agents.read().await.len();

    let spaces_count = state
        .space_manager
        .count_spaces()
        .await
        .unwrap_or(0);
    let agents_count = state.registry.count().await.unwrap_or(0);

    Json(HealthResponse {
        status: "ok".to_string(),
        uptime_seconds: uptime,
        spaces_count,
        agents_count,
        online_agents: online_count,
    })
}
