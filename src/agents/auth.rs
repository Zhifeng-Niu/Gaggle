//! API Key鉴权中间件

use crate::agents::{Agent, AgentRegistry};
use axum::{extract::Request, middleware::Next, response::Response};

/// 从请求中提取并验证API Key
pub async fn api_key_auth(
    mut req: Request,
    next: Next,
    registry: AgentRegistry,
) -> Result<Response, crate::error::GaggleError> {
    // 从Authorization header提取Bearer token
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let api_key = match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            header.trim_start_matches("Bearer ").to_string()
        }
        _ => {
            return Err(crate::error::GaggleError::Unauthorized(
                "Missing or invalid Authorization header".to_string(),
            ));
        }
    };

    // 验证API Key
    let agent = registry
        .get_by_api_key(&api_key)
        .await?
        .ok_or_else(|| crate::error::GaggleError::Unauthorized("Invalid API key".to_string()))?;

    // 将Agent信息注入到请求扩展中
    req.extensions_mut().insert(agent);

    Ok(next.run(req).await)
}

/// 提取已认证的Agent
pub fn extract_agent(req: &Request) -> Option<&Agent> {
    req.extensions().get::<Agent>()
}
