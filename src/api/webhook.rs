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
