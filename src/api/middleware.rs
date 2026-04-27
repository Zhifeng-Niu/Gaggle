//! API 中间件

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::connect_info::ConnectInfo;

/// 速率限制状态
///
/// 使用滑动窗口算法跟踪每个客户端 IP 的请求速率
#[derive(Debug, Clone)]
pub struct RateLimitState {
    /// IP -> (窗口开始时间, 请求数)
    inner: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
}

impl RateLimitState {
    /// 创建新的速率限制状态
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 检查并更新指定 IP 的请求计数
    ///
    /// 如果超过速率限制，返回 Err 带有剩余秒数
    /// 否则更新计数并返回 Ok
    pub fn check_and_update(
        &self,
        ip: &str,
        max_requests: u32,
        window_seconds: u64,
    ) -> Result<(), u64> {
        let mut state = self.inner.lock().unwrap();
        let now = Instant::now();
        let window_duration = std::time::Duration::from_secs(window_seconds);

        // 清理过期的窗口
        state.retain(|_, (window_start, _)| {
            now.duration_since(*window_start) < window_duration
        });

        // 获取或创建该 IP 的窗口
        let entry = state.entry(ip.to_string()).or_insert_with(|| (now, 0));

        // 检查窗口是否已过期
        if now.duration_since(entry.0) >= window_duration {
            // 重置窗口
            entry.0 = now;
            entry.1 = 1;
            Ok(())
        } else if entry.1 < max_requests {
            // 窗口内，未超限
            entry.1 += 1;
            Ok(())
        } else {
            // 超限，计算重置时间
            let elapsed = now.duration_since(entry.0).as_secs();
            let retry_after = window_seconds.saturating_sub(elapsed);
            Err(retry_after)
        }
    }
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self::new()
    }
}

/// 从请求中提取客户端 IP 地址
///
/// 优先级: X-Real-IP > X-Forwarded-For 第一项 > 连接地址
fn extract_client_ip(headers: &HeaderMap, remote_addr: Option<std::net::SocketAddr>) -> String {
    // 首先检查 X-Real-IP (由 nginx 等反向代理设置)
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(ip_str) = real_ip.to_str() {
            return ip_str.to_string();
        }
    }

    // 检查 X-Forwarded-For
    if let Some(forwarded_for) = headers.get("x-forwarded-for") {
        if let Ok(forwarded_str) = forwarded_for.to_str() {
            // X-Forwarded-For 可能包含多个 IP，取第一个
            if let Some(first_ip) = forwarded_str.split(',').next() {
                return first_ip.trim().to_string();
            }
        }
    }

    // 回退到连接地址
    if let Some(addr) = remote_addr {
        addr.ip().to_string()
    } else {
        // 最后的回退
        "unknown".to_string()
    }
}

/// 速率限制配置 (存储在请求扩展中)
#[derive(Clone, Copy)]
pub struct RateLimitConfig {
    pub max_requests: u32,
    pub window_seconds: u64,
}

/// 异步中间件处理函数
pub async fn rate_limit_middleware(
    State(state): State<RateLimitState>,
    ConnectInfo(remote_addr): ConnectInfo<std::net::SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();
    let client_ip = extract_client_ip(headers, Some(remote_addr));

    // 从请求扩展中获取配置 (由 main.rs 设置)
    let max_requests = req
        .extensions()
        .get::<RateLimitConfig>()
        .map(|c| c.max_requests)
        .unwrap_or(120);
    let window_seconds = req
        .extensions()
        .get::<RateLimitConfig>()
        .map(|c| c.window_seconds)
        .unwrap_or(60);

    match state.check_and_update(&client_ip, max_requests, window_seconds) {
        Ok(_) => {
            // 未超限，继续处理请求
            next.run(req).await
        }
        Err(retry_after) => {
            // 超限，返回 429
            let json = serde_json::json!({
                "error": "rate_limit_exceeded",
                "message": "Too many requests. Please try again later.",
                "retry_after": retry_after
            });
            let mut response = Json(json).into_response();
            *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
            response.headers_mut().insert(
                "retry-after",
                retry_after.to_string().parse().unwrap(),
            );
            response
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_under_limit() {
        let state = RateLimitState::new();
        let ip = "127.0.0.1";

        // 前 5 个请求应该成功
        for _ in 0..5 {
            assert!(state.check_and_update(ip, 10, 60).is_ok());
        }
    }

    #[test]
    fn test_rate_limit_exceeded() {
        let state = RateLimitState::new();
        let ip = "127.0.0.1";

        // 达到限制
        for _ in 0..10 {
            assert!(state.check_and_update(ip, 10, 60).is_ok());
        }

        // 第 11 个请求应该失败
        assert!(state.check_and_update(ip, 10, 60).is_err());
    }

    #[test]
    fn test_rate_limit_window_reset() {
        let state = RateLimitState::new();
        let ip = "127.0.0.1";

        // 达到限制 (使用很短的窗口方便测试)
        for _ in 0..5 {
            assert!(state.check_and_update(ip, 5, 1).is_ok());
        }

        // 应该超限
        assert!(state.check_and_update(ip, 5, 1).is_err());

        // 等待窗口过期
        std::thread::sleep(std::time::Duration::from_secs(2));

        // 窗口已过期，应该成功
        assert!(state.check_and_update(ip, 5, 1).is_ok());
    }

    #[test]
    fn test_extract_client_ip_from_x_real_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "192.168.1.100".parse().unwrap());

        let ip = extract_client_ip(&headers, Some("127.0.0.1:8080".parse().unwrap()));
        assert_eq!(ip, "192.168.1.100");
    }

    #[test]
    fn test_extract_client_ip_from_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "10.0.0.1, 10.0.0.2".parse().unwrap());

        let ip = extract_client_ip(&headers, None);
        assert_eq!(ip, "10.0.0.1");
    }

    #[test]
    fn test_extract_client_ip_fallback() {
        let headers = HeaderMap::new();

        let ip = extract_client_ip(&headers, Some("192.168.1.1:8080".parse().unwrap()));
        assert_eq!(ip, "192.168.1.1");
    }
}
