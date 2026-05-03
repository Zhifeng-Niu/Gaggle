//! OpenClaw 兼容 WebSocket Gateway
//!
//! 让 OpenClaw/Hermes 等 AI Agent 通过标准 WebSocket 连接到 Gaggle Negotiation Space。
//! 协议：首帧 connect 认证，后续消息按 OpenClaw 风格收发。

use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::agents::Agent;
use crate::negotiation::ProposalDimensions;

use super::rest::AppState;
use super::ws::{
    self, CloseSpacePayload, CreateSpacePayload, SendMessagePayload, SubmitProposalPayload,
    WsIncoming, WsOutgoing,
};

// ── OpenClaw 协议类型 ─────────────────────────────────

/// OpenClaw 入站消息（从客户端发送到 Gateway）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenClawIncoming {
    /// 首帧握手：认证并建立连接
    Connect { agent_id: String, api_key: String },
    /// 创建谈判空间
    CreateSpace {
        name: String,
        invitee_ids: Vec<String>,
        context: serde_json::Value,
    },
    /// 加入已有空间
    JoinSpace { space_id: String },
    /// 发送消息到空间
    SendMessage {
        space_id: String,
        msg_type: String,
        content: String,
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    },
    /// 提交提案（RFP 谈判）
    SubmitProposal {
        space_id: String,
        proposal_type: String,
        dimensions: ProposalDimensions,
        #[serde(default)]
        parent_proposal_id: Option<String>,
    },
    /// 关闭空间
    CloseSpace {
        space_id: String,
        conclusion: String,
        #[serde(default)]
        final_terms: Option<serde_json::Value>,
    },
}

/// OpenClaw 出站消息（从 Gateway 发送到客户端）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenClawOutgoing {
    /// 连接确认
    Connected { agent_id: String, status: String },
    /// 空间已创建
    SpaceCreated {
        space_id: String,
        space: crate::negotiation::Space,
        members: Vec<String>,
    },
    /// 已加入空间
    SpaceJoined { space_id: String, agent_id: String },
    /// 新消息
    NewMessage {
        space_id: String,
        message: crate::negotiation::SpaceMessage,
    },
    /// 新提案
    NewProposal {
        space_id: String,
        proposal: crate::negotiation::Proposal,
    },
    /// 提案状态更新
    ProposalUpdate {
        space_id: String,
        proposal_id: String,
        status: String,
        action: String,
    },
    /// 空间已关闭
    SpaceClosed {
        space_id: String,
        conclusion: String,
    },
    /// 错误
    Error {
        code: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        space_id: Option<String>,
    },
}

// ── Gateway 连接入口 ──────────────────────────────────

/// WebSocket 升级入口：ws://host:8080/ws/v1/gateway
pub async fn gateway_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_gateway_socket(socket, state))
}

async fn handle_gateway_socket(socket: axum::extract::ws::WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // ── 1. 等待首帧 connect 握手 ──────────────────────
    let agent = match wait_for_connect(&mut receiver, &state).await {
        Ok(a) => a,
        Err(msg) => {
            let error = serde_json::to_string(&OpenClawOutgoing::Error {
                code: "HANDSHAKE_FAILED".to_string(),
                message: msg,
                space_id: None,
            })
            .unwrap_or_default();
            let _ = sender.send(axum::extract::ws::Message::Text(error)).await;
            return;
        }
    };

    // 发送 connected 确认
    let connected = serde_json::to_string(&OpenClawOutgoing::Connected {
        agent_id: agent.id.clone(),
        status: "ok".to_string(),
    })
    .unwrap_or_default();
    if sender
        .send(axum::extract::ws::Message::Text(connected))
        .await
        .is_err()
    {
        return;
    }

    // ── 2. 注册到在线连接表（多连接支持） ──────────────
    let now_ts = chrono::Utc::now().timestamp_millis();
    let conn_id = super::ws::NEXT_CONN_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let personal_tx = {
        let (tx, _rx) = broadcast::channel::<String>(256);
        let mut online = state.online_agents.write().await;
        online.entry(agent.id.clone()).or_default().push(super::rest::ConnectionInfo {
            tx: tx.clone(),
            connected_since: now_ts,
            last_ping: std::sync::atomic::AtomicI64::new(now_ts),
            conn_id,
        });
        tx
    };

    // 订阅已有 Space 的 broadcast
    let agent_spaces = state
        .space_manager
        .get_agent_spaces(&agent.id)
        .await
        .unwrap_or_default();
    let (merged_tx, mut merged_rx) = broadcast::channel::<String>(512);

    // 转发 personal channel
    let personal_forward = personal_tx.subscribe();
    let mt = merged_tx.clone();
    tokio::spawn(async move {
        let mut rx = personal_forward;
        loop {
            match rx.recv().await {
                Ok(msg) => { let _ = mt.send(msg); }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "openclaw personal channel lagged, continuing");
                    continue;
                }
                Err(_) => break,
            }
        }
    });

    // 转发已有 space broadcast channels
    for space in &agent_spaces {
        if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
            let mt = merged_tx.clone();
            tokio::spawn(async move {
                let mut rx = tx.subscribe();
                loop {
                    match rx.recv().await {
                        Ok(msg) => { let _ = mt.send(msg); }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(skipped = n, "openclaw space channel lagged, continuing");
                            continue;
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    }

    // ── 3. 主事件循环 ─────────────────────────────────
    loop {
        tokio::select! {
            // 入站：OpenClaw 客户端消息
            msg_result = receiver.next() => {
                match msg_result {
                    Some(Ok(axum::extract::ws::Message::Text(text))) => {
                        if let Err(e) = handle_openclaw_message(
                            &text, &agent, &state
                        ).await {
                            let error = serde_json::to_string(&OpenClawOutgoing::Error {
                                code: "INTERNAL_ERROR".to_string(),
                                message: e.to_string(),
                                space_id: None,
                            }).unwrap_or_default();
                            let _ = sender.send(axum::extract::ws::Message::Text(error)).await;
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            // 出站：Gaggle broadcast → OpenClaw 格式翻译后推送
            broadcast_result = merged_rx.recv() => {
                match broadcast_result {
                    Ok(msg) => {
                        if let Some(translated) = translate_gaggle_to_openclaw(&msg) {
                            let json = serde_json::to_string(&translated).unwrap_or_default();
                            if sender.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => continue,
                }
            }
        }
    }

    // ── 4. 清理：移除自己的连接（保留其他连接） ──────
    {
        let mut online = state.online_agents.write().await;
        if let Some(conns) = online.get_mut(&agent.id) {
            conns.retain(|c| c.conn_id != conn_id);
            if conns.is_empty() {
                online.remove(&agent.id);
            }
        }
    }
}

// ── 握手认证 ──────────────────────────────────────────

async fn wait_for_connect(
    receiver: &mut futures_util::stream::SplitStream<axum::extract::ws::WebSocket>,
    state: &AppState,
) -> Result<Agent, String> {
    // 等待首帧 connect，设置超时 10 秒
    let msg = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        match receiver.next().await {
            Some(Ok(axum::extract::ws::Message::Text(text))) => Ok(text),
            Some(Ok(axum::extract::ws::Message::Close(_))) => {
                Err("Connection closed before handshake".to_string())
            }
            Some(Err(e)) => Err(format!("WebSocket error: {e}")),
            None => Err("No message received".to_string()),
            _ => Err("Expected text message for handshake".to_string()),
        }
    })
    .await
    .map_err(|_| "Handshake timeout (10s)".to_string())?
    .map_err(|e| e)?;

    let incoming: OpenClawIncoming =
        serde_json::from_str(&msg).map_err(|e| format!("Invalid handshake: {e}"))?;

    match incoming {
        OpenClawIncoming::Connect { agent_id, api_key } => {
            // 双重验证：api_key 必须匹配 agent_id
            let agent = state
                .registry
                .get_by_api_key(&api_key)
                .await
                .map_err(|e| format!("Auth lookup failed: {e}"))?
                .ok_or_else(|| "Invalid api_key".to_string())?;

            if agent.id != agent_id {
                return Err("agent_id does not match api_key".to_string());
            }

            Ok(agent)
        }
        _ => Err("First message must be connect".to_string()),
    }
}

// ── OpenClaw 消息处理 ──────────────────────────────────

async fn handle_openclaw_message(
    text: &str,
    agent: &Agent,
    state: &AppState,
) -> Result<(), crate::error::GaggleError> {
    let oc_msg: OpenClawIncoming = serde_json::from_str(text)
        .map_err(|e| crate::error::GaggleError::ValidationError(e.to_string()))?;

    // 翻译 OpenClaw 消息为 Gaggle 内部 WsIncoming，复用 ws 模块的处理逻辑
    let gaggle_msg = match oc_msg {
        OpenClawIncoming::Connect { .. } => {
            // 忽略重复的 connect（已通过握手认证）
            return Ok(());
        }
        OpenClawIncoming::CreateSpace {
            name,
            invitee_ids,
            context,
        } => WsIncoming::CreateSpace {
            request_id: None,
            payload: CreateSpacePayload {
                name,
                invitee_ids,
                context,
                my_role: None,
            },
        },
        OpenClawIncoming::JoinSpace { space_id } => WsIncoming::JoinSpace {
            request_id: None,
            space_id,
        },
        OpenClawIncoming::SendMessage {
            space_id,
            msg_type,
            content,
            metadata,
        } => {
            let mt = parse_msg_type(&msg_type);
            WsIncoming::SendMessage {
                request_id: None,
                space_id,
                payload: SendMessagePayload {
                    msg_type: mt,
                    content,
                    metadata,
                    proposal: None,
                },
            }
        }
        OpenClawIncoming::SubmitProposal {
            space_id,
            proposal_type,
            dimensions,
            parent_proposal_id,
        } => WsIncoming::SubmitProposal {
            request_id: None,
            space_id,
            payload: SubmitProposalPayload {
                proposal_type,
                dimensions,
                parent_proposal_id,
            },
        },
        OpenClawIncoming::CloseSpace {
            space_id,
            conclusion,
            final_terms,
        } => WsIncoming::CloseSpace {
            request_id: None,
            space_id,
            payload: CloseSpacePayload {
                conclusion,
                final_terms,
            },
        },
    };

    // 复用 Gaggle 原生 WebSocket 的消息处理逻辑（忽略 ACK 返回值）
    let _ = ws::handle_ws_message(&serde_json::to_string(&gaggle_msg)?, agent, state).await?;
    Ok(())
}

/// 将字符串消息类型解析为 Gaggle MessageType
fn parse_msg_type(s: &str) -> crate::negotiation::MessageType {
    use crate::negotiation::message::MessageType;
    match s {
        "proposal" => MessageType::Proposal,
        "counter_proposal" => MessageType::CounterProposal,
        "acceptance" => MessageType::Acceptance,
        "rejection" => MessageType::Rejection,
        "withdrawal" => MessageType::Withdrawal,
        "system" => MessageType::System,
        "attachment" => MessageType::Attachment,
        _ => MessageType::Text,
    }
}

// ── Gaggle → OpenClaw 出站翻译 ─────────────────────────

/// 将 Gaggle WsOutgoing broadcast 消息翻译为 OpenClaw 格式
fn translate_gaggle_to_openclaw(gaggle_json: &str) -> Option<OpenClawOutgoing> {
    let outgoing: WsOutgoing = serde_json::from_str(gaggle_json).ok()?;

    match outgoing {
        WsOutgoing::SpaceCreated { payload, .. } => Some(OpenClawOutgoing::SpaceCreated {
            space_id: payload.space.id.clone(),
            space: payload.space,
            members: payload.members,
        }),
        WsOutgoing::SpaceJoined { payload, .. } => Some(OpenClawOutgoing::SpaceJoined {
            space_id: payload.space_id,
            agent_id: payload.agent_id,
        }),
        WsOutgoing::NewMessage { payload, .. } => Some(OpenClawOutgoing::NewMessage {
            space_id: payload.message.space_id.clone(),
            message: payload.message,
        }),
        WsOutgoing::NewProposal { space_id, payload } => Some(OpenClawOutgoing::NewProposal {
            space_id,
            proposal: payload.proposal,
        }),
        WsOutgoing::ProposalUpdate {
            space_id, payload, ..
        } => Some(OpenClawOutgoing::ProposalUpdate {
            space_id,
            proposal_id: payload.proposal_id,
            status: payload.status,
            action: payload.action,
        }),
        WsOutgoing::SpaceClosed { space_id, payload } => Some(OpenClawOutgoing::SpaceClosed {
            space_id,
            conclusion: payload.conclusion,
        }),
        WsOutgoing::Error { payload, .. } => Some(OpenClawOutgoing::Error {
            code: payload.code,
            message: payload.message,
            space_id: None,
        }),
        // RfpCreated 和 BestTermsShared 暂无直接 OpenClaw 对应类型，跳过
        _ => None,
    }
}
