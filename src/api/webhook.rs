//! Webhook 唤醒服务
//!
//! 当 Agent 离线时，Gaggle 向其 `callback_url` 发送 HTTP POST 通知，
//! 引导 Agent 重新建立 WebSocket 连接并 resume 离线事件。

use reqwest::Client;
use serde_json::json;
use std::time::Duration;

/// 构建一个共享的 HTTP client（连接池复用，5s 超时）
fn http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default()
}

/// Validate callback_url to prevent SSRF attacks.
/// Blocks: private IPs, link-local, loopback, metadata endpoints.
fn validate_callback_url(raw_url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(raw_url).map_err(|e| format!("invalid URL: {e}"))?;

    // Only HTTPS or HTTP schemes allowed
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err(format!("unsupported scheme: {}", parsed.scheme())),
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL must have a host".to_string())?
        .trim_start_matches('[')
        .trim_end_matches(']'); // Strip IPv6 brackets

    // Block obvious internal/metadata hosts
    let blocked_hosts = [
        "localhost",
        "127.0.0.1",
        "0.0.0.0",
        "::1",
        "169.254.169.254", // AWS/GCP metadata
        "metadata.google.internal",
    ];
    for &blocked in &blocked_hosts {
        if host == blocked {
            return Err(format!("callback URL blocked: {host}"));
        }
    }

    // Block private IP ranges (10.x, 172.16-31.x, 192.168.x, 169.254.x)
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if ip.is_loopback() {
            return Err(format!("callback URL blocked: loopback IP {ip}"));
        }
        match &ip {
            std::net::IpAddr::V4(v4) => {
                let octets = v4.octets();
                // 10.0.0.0/8
                if octets[0] == 10 {
                    return Err(format!("callback URL blocked: private IP {ip}"));
                }
                // 172.16.0.0/12
                if octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31 {
                    return Err(format!("callback URL blocked: private IP {ip}"));
                }
                // 192.168.0.0/16
                if octets[0] == 192 && octets[1] == 168 {
                    return Err(format!("callback URL blocked: private IP {ip}"));
                }
                // 169.254.0.0/16 (link-local / cloud metadata)
                if octets[0] == 169 && octets[1] == 254 {
                    return Err(format!("callback URL blocked: link-local IP {ip}"));
                }
                // 127.0.0.0/8 (loopback range)
                if octets[0] == 127 {
                    return Err(format!("callback URL blocked: loopback IP {ip}"));
                }
                // 0.0.0.0
                if octets == [0, 0, 0, 0] {
                    return Err(format!("callback URL blocked: unspecified IP {ip}"));
                }
            }
            std::net::IpAddr::V6(v6) => {
                if v6.is_loopback() {
                    return Err(format!("callback URL blocked: loopback IP {ip}"));
                }
            }
        }
    }

    Ok(())
}

/// 向 Agent 的 callback_url 发送唤醒通知。
///
/// 请求体格式：
/// ```json
/// {
///   "agent_id": "...",
///   "event": "new_message",
///   "payload": "{ ... }",
///   "timestamp": 1745577600000,
///   "action": "reconnect"
/// }
/// ```
///
/// Agent 收到后应：1) 建立 WS 连接  2) 发送 Resume 命令  3) 处理离线事件
pub async fn fire_webhook(
    callback_url: &str,
    agent_id: &str,
    event_type: &str,
    payload: &str,
) -> Result<(), String> {
    // SSRF protection: validate target URL before making request
    validate_callback_url(callback_url)?;

    let body = json!({
        "agent_id": agent_id,
        "event": event_type,
        "payload": payload,
        "timestamp": chrono::Utc::now().timestamp_millis(),
        "action": "reconnect",
    });

    let client = http_client();

    // 最多重试 3 次（首次 + 2 次重试）
    for attempt in 0..3 {
        match client
            .post(callback_url)
            .json(&body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!(
                    agent_id = %agent_id,
                    attempt = attempt + 1,
                    "webhook delivered successfully"
                );
                return Ok(());
            }
            Ok(resp) => {
                tracing::warn!(
                    agent_id = %agent_id,
                    attempt = attempt + 1,
                    status = %resp.status(),
                    "webhook returned non-2xx"
                );
            }
            Err(e) => {
                tracing::warn!(
                    agent_id = %agent_id,
                    attempt = attempt + 1,
                    error = %e,
                    "webhook delivery failed"
                );
            }
        }

        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(500 * (attempt as u64 + 1))).await;
        }
    }

    Err(format!(
        "webhook to {} failed after 3 attempts",
        callback_url
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_callback_url_blocks_ssrf() {
        // Internal IPs should be blocked
        assert!(validate_callback_url("http://127.0.0.1/webhook").is_err());
        assert!(validate_callback_url("http://localhost/webhook").is_err());
        assert!(validate_callback_url("http://10.0.0.1/webhook").is_err());
        assert!(validate_callback_url("http://172.16.0.1/webhook").is_err());
        assert!(validate_callback_url("http://192.168.1.1/webhook").is_err());
        assert!(validate_callback_url("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(validate_callback_url("http://0.0.0.0/webhook").is_err());
        assert!(validate_callback_url("http://[::1]/webhook").is_err());
    }

    #[test]
    fn test_validate_callback_url_allows_public() {
        // Public URLs should be allowed
        assert!(validate_callback_url("https://example.com/webhook").is_ok());
        assert!(validate_callback_url("http://203.0.113.50/webhook").is_ok());
        assert!(validate_callback_url("https://api.agent-callback.io/hook").is_ok());
    }

    #[test]
    fn test_validate_callback_url_rejects_bad_schemes() {
        assert!(validate_callback_url("ftp://example.com/webhook").is_err());
        assert!(validate_callback_url("file:///etc/passwd").is_err());
        assert!(validate_callback_url("gopher://example.com/").is_err());
    }
}