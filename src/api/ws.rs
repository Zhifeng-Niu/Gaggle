//! WebSocket处理

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

use crate::agents::Agent;
use crate::negotiation::{
    MessageType, ProposalDimensions, ProposalResponseAction, ProposalType, SpaceStatus,
};

use super::rest::AppState;

// ── WS 协议类型 ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinSpacePayload {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessagePayload {
    pub msg_type: MessageType,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseSpacePayload {
    pub conclusion: String,
    #[serde(default)]
    pub final_terms: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSpacePayload {
    pub name: String,
    #[serde(default)]
    pub invitee_ids: Vec<String>,
    pub context: serde_json::Value,
    #[serde(default)]
    pub my_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRfpPayload {
    pub name: String,
    #[serde(default)]
    pub provider_ids: Vec<String>,
    pub allowed_rounds: Option<u32>,
    pub evaluation_criteria: Option<Vec<String>>,
    pub deadline: Option<i64>,
    pub share_best_terms: Option<bool>,
    #[serde(default)]
    pub context: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitProposalPayload {
    pub space_id: String,
    pub proposal_type: String,
    pub dimensions: ProposalDimensions,
    pub parent_proposal_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespondToProposalPayload {
    pub space_id: String,
    pub proposal_id: String,
    pub action: String,
    pub counter_dimensions: Option<ProposalDimensions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareBestTermsPayload {
    pub space_id: String,
    pub best_dimensions: ProposalDimensions,
}

// P3 命令 payloads

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetMessagesPayload {
    pub space_id: String,
    #[serde(default)]
    pub after: Option<i64>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckOnlinePayload {
    pub agent_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsIncoming {
    CreateSpace {
        payload: CreateSpacePayload,
    },
    CreateRfp {
        payload: CreateRfpPayload,
    },
    JoinSpace {
        payload: JoinSpacePayload,
    },
    SendMessage {
        space_id: String,
        payload: SendMessagePayload,
    },
    CloseSpace {
        space_id: String,
        payload: CloseSpacePayload,
    },
    SubmitProposal {
        payload: SubmitProposalPayload,
    },
    RespondToProposal {
        payload: RespondToProposalPayload,
    },
    ShareBestTerms {
        payload: ShareBestTermsPayload,
    },
    // P3 命令
    Ping {
        timestamp: Option<i64>,
    },
    ListSpaces,
    GetMessages {
        payload: GetMessagesPayload,
    },
    CheckOnline {
        payload: CheckOnlinePayload,
    },
    LeaveSpace {
        payload: LeaveSpacePayload,
    },
    // 离线事件恢复：客户端告知最后收到的 event_seq，服务端补发之后的事件
    Resume {
        last_event_seq: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaveSpacePayload {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceCreatedPayload {
    pub space: crate::negotiation::Space,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceJoinedPayload {
    pub space_id: String,
    pub agent_id: String,
    #[serde(default)]
    pub assigned_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMessagePayload {
    pub message: crate::negotiation::SpaceMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceClosedPayload {
    pub conclusion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfpCreatedPayload {
    pub space: crate::negotiation::Space,
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewProposalPayload {
    pub proposal: crate::negotiation::Proposal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalUpdatePayload {
    pub proposal_id: String,
    pub status: String,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestTermsSharedPayload {
    pub space_id: String,
    pub best_dimensions: ProposalDimensions,
    pub shared_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceStatusChangedPayload {
    pub space_id: String,
    pub old_status: String,
    pub new_status: String,
}

// P3 命令 payloads

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceSummary {
    pub id: String,
    pub name: String,
    pub status: String,
    pub space_type: String,
    pub agent_count: usize,
    pub creator_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub buyer_id: Option<String>,
    #[serde(default)]
    pub seller_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacesListPayload {
    pub spaces: Vec<SpaceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesListPayload {
    pub messages: Vec<crate::negotiation::SpaceMessage>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineStatusPayload {
    pub statuses: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceLeftPayload {
    pub space_id: String,
    pub agent_id: String,
    pub remaining_agents: Vec<String>,
    pub space_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsOutgoing {
    SpaceCreated {
        space_id: String,
        payload: SpaceCreatedPayload,
    },
    RfpCreated {
        space_id: String,
        payload: RfpCreatedPayload,
    },
    SpaceJoined {
        space_id: String,
        payload: SpaceJoinedPayload,
    },
    NewMessage {
        space_id: String,
        payload: NewMessagePayload,
    },
    NewProposal {
        space_id: String,
        payload: NewProposalPayload,
    },
    ProposalUpdate {
        space_id: String,
        payload: ProposalUpdatePayload,
    },
    BestTermsShared {
        space_id: String,
        payload: BestTermsSharedPayload,
    },
    SpaceClosed {
        space_id: String,
        payload: SpaceClosedPayload,
    },
    SpaceStatusChanged {
        space_id: String,
        payload: SpaceStatusChangedPayload,
    },
    // P3 命令
    Pong {
        timestamp: Option<i64>,
        server_time: i64,
    },
    SpacesList {
        payload: SpacesListPayload,
    },
    MessagesList {
        space_id: String,
        payload: MessagesListPayload,
    },
    OnlineStatus {
        payload: OnlineStatusPayload,
    },
    SpaceLeft {
        space_id: String,
        payload: SpaceLeftPayload,
    },
    Error {
        space_id: Option<String>,
        payload: ErrorPayload,
    },
    // 离线事件恢复
    ResumeAck {
        replayed_count: usize,
        last_event_seq: i64,
    },
    // 单条重放的事件（包装原事件 + event_seq）
    ReplayedEvent {
        event_seq: i64,
        event_type: String,
        payload: serde_json::Value,
    },
}

// ── 连接入口 ─────────────────────────────────────────

/// 推送事件到 agent：在线则直接发送，同时持久化到离线队列。
/// 如果 agent 在线，事件入队后立即标记为已送达（enqueue_delivered）。
/// 如果 agent 离线，事件留在队列中等待 resume 时重放。
pub async fn push_event(
    state: &AppState,
    agent_id: &str,
    event_type: &str,
    payload: &str,
) -> Result<i64, crate::error::GaggleError> {
    let online = state.online_agents.read().await;
    if let Some(conns) = online.get(agent_id) {
        if !conns.is_empty() {
            // 在线：广播到该 Agent 的所有 WS 连接 + 入队标记已送达
            for conn in conns {
                let _ = conn.tx.send(payload.to_string());
            }
            drop(online);
            let seq = state.event_queue.enqueue_delivered(agent_id, event_type, payload).await?;
            return Ok(seq);
        }
    }
    {
        let _ = &online; // ensure dropped
    }
    {
        // 离线：入队 + 尝试 webhook 唤醒
        drop(online);
        let seq = state.event_queue.enqueue(agent_id, event_type, payload).await?;

        // 如果 Agent 注册了 callback_url，异步发送唤醒通知
        if let Ok(Some(agent)) = state.registry.get_by_id(agent_id).await {
            if let Some(ref url) = agent.callback_url {
                let url = url.clone();
                let aid = agent_id.to_string();
                let et = event_type.to_string();
                let pl = payload.to_string();
                tokio::spawn(async move {
                    if let Err(e) = super::webhook::fire_webhook(&url, &aid, &et, &pl).await {
                        tracing::warn!(agent_id = %aid, error = %e, "webhook wake failed");
                    }
                });
            }
        }

        Ok(seq)
    }
}

#[derive(Debug, Deserialize)]
pub struct WsAuthQuery {
    #[serde(default)]
    pub token: Option<String>,
}

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Query(auth): Query<WsAuthQuery>,
) -> Response {
    // Token 鉴权
    if let Some(token) = auth.token {
        if let Ok(Some(agent)) = state.registry.get_by_api_key(&token).await {
            if agent.id != agent_id {
                // token 有效但 agent_id 不匹配
                return ws.on_upgrade(move |socket| {
                    async move {
                        let (mut sender, _) = socket.split();
                        let msg = serde_json::to_string(&WsOutgoing::Error {
                            space_id: None,
                            payload: ErrorPayload {
                                code: "FORBIDDEN".to_string(),
                                message: "Token does not match agent_id".to_string(),
                            },
                        }).unwrap_or_default();
                        let _ = sender.send(WsMessage::Text(msg)).await;
                    }
                });
            }
            // 鉴权通过
        } else {
            // 无效 token
            return ws.on_upgrade(move |socket| {
                async move {
                    let (mut sender, _) = socket.split();
                    let msg = serde_json::to_string(&WsOutgoing::Error {
                        space_id: None,
                        payload: ErrorPayload {
                            code: "UNAUTHORIZED".to_string(),
                            message: "Invalid API key".to_string(),
                        },
                    }).unwrap_or_default();
                    let _ = sender.send(WsMessage::Text(msg)).await;
                }
            });
        }
    }
    // token=None: 兼容旧客户端，只验证 agent_id 存在（后续可改为必须 token）

    ws.on_upgrade(move |socket| handle_socket(socket, state, agent_id))
}

async fn handle_socket(socket: WebSocket, state: AppState, agent_id: String) {
    let (mut sender, mut receiver) = socket.split();

    // 验证 Agent
    let agent = match state.registry.get_by_id(&agent_id).await {
        Ok(Some(a)) => a,
        _ => {
            let msg = serde_json::to_string(&WsOutgoing::Error {
                space_id: None,
                payload: ErrorPayload {
                    code: "UNAUTHORIZED".to_string(),
                    message: "Agent not found".to_string(),
                },
            })
            .unwrap_or_default();
            let _ = sender.send(WsMessage::Text(msg)).await;
            return;
        }
    };

    // 多连接支持：追加新连接（不踢旧连接）
    let now_ts = chrono::Utc::now().timestamp_millis();
    let personal_tx = {
        let mut online = state.online_agents.write().await;
        let (tx, _rx) = broadcast::channel::<String>(256);
        let conn_info = super::rest::ConnectionInfo {
            tx: tx.clone(),
            connected_since: now_ts,
            last_ping: now_ts,
        };
        online.entry(agent.id.clone()).or_default().push(conn_info);
        tx
    };

    // 订阅已有 Space 的 broadcast
    let agent_spaces = state
        .space_manager
        .get_agent_spaces(&agent.id)
        .await
        .unwrap_or_default();
    let mut space_rxs: Vec<(String, broadcast::Receiver<String>)> = Vec::new();
    let mut subscribed_space_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for space in &agent_spaces {
        if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
            space_rxs.push((space.id.clone(), tx.subscribe()));
            subscribed_space_ids.insert(space.id.clone());
        }
    }

    // 合并所有 broadcast receiver 到一个 channel（personal + space broadcasts）
    let (merged_tx, mut merged_rx) = broadcast::channel::<String>(512);

    // 转发 personal channel（新 Space 邀请等）
    let personal_forward = personal_tx.subscribe();
    let mt = merged_tx.clone();
    tokio::spawn(async move {
        let mut rx = personal_forward;
        while let Ok(msg) = rx.recv().await {
            let _ = mt.send(msg);
        }
    });

    // 转发已有的 space broadcast channels
    for (space_id, rx) in space_rxs {
        let mt = merged_tx.clone();
        tokio::spawn(async move {
            let mut rx = rx;
            while let Ok(msg) = rx.recv().await {
                let _ = mt.send(msg);
            }
        });
        let _ = &space_id;
    }

    // 自动重放离线事件
    {
        let pending = state
            .event_queue
            .get_pending(&agent.id, 0)
            .await
            .unwrap_or_default();
        if !pending.is_empty() {
            tracing::info!(
                agent_id = %agent.id,
                count = pending.len(),
                "Replaying pending events"
            );
            let mut seqs_to_mark: Vec<i64> = Vec::new();
            for evt in &pending {
                let replayed = serde_json::to_string(&WsOutgoing::ReplayedEvent {
                    event_seq: evt.event_seq,
                    event_type: evt.event_type.clone(),
                    payload: serde_json::from_str(&evt.payload).unwrap_or(serde_json::Value::Null),
                })
                .unwrap_or_default();
                if sender.send(WsMessage::Text(replayed)).await.is_err() {
                    break;
                }
                seqs_to_mark.push(evt.event_seq);
            }
            if !seqs_to_mark.is_empty() {
                let _ = state.event_queue.mark_delivered(&agent.id, &seqs_to_mark).await;
                let ack = serde_json::to_string(&WsOutgoing::ResumeAck {
                    replayed_count: seqs_to_mark.len(),
                    last_event_seq: *seqs_to_mark.last().unwrap_or(&0),
                })
                .unwrap_or_default();
                let _ = sender.send(WsMessage::Text(ack)).await;
            }
        }
    }

    // 心跳超时计时器
    let mut last_activity = Instant::now();
    let heartbeat_timeout = Duration::from_secs(90);

    // 主事件循环：tokio::select! 双向监听 + 心跳超时
    loop {
        let timeout_remaining = heartbeat_timeout.saturating_sub(last_activity.elapsed());

        tokio::select! {
            // 入站：来自 WebSocket 客户端的消息
            msg_result = receiver.next() => {
                match msg_result {
                    Some(Ok(WsMessage::Text(text))) => {
                        // 重置心跳计时器
                        last_activity = Instant::now();
                        // 更新 last_ping（只更新自己的连接）
                        {
                            let mut online = state.online_agents.write().await;
                            if let Some(conns) = online.get_mut(&agent.id) {
                                for conn in conns.iter_mut() {
                                    if conn.connected_since == now_ts {
                                        conn.last_ping = chrono::Utc::now().timestamp_millis();
                                    }
                                }
                            }
                        }

                        if let Err(e) = handle_ws_message(
                            &text, &agent, &state
                        ).await {
                            let error_msg = serde_json::to_string(&WsOutgoing::Error {
                                space_id: None,
                                payload: ErrorPayload {
                                    code: "INTERNAL_ERROR".to_string(),
                                    message: e.to_string(),
                                },
                            }).unwrap_or_default();
                            let _ = sender.send(WsMessage::Text(error_msg)).await;
                        }

                        // 处理完消息后，检查是否有新加入的 space 需要订阅
                        let current_spaces = state
                            .space_manager
                            .get_agent_spaces(&agent.id)
                            .await
                            .unwrap_or_default();
                        for space in &current_spaces {
                            if !subscribed_space_ids.contains(&space.id) {
                                if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
                                    let rx = tx.subscribe();
                                    let mt = merged_tx.clone();
                                    tokio::spawn(async move {
                                        let mut rx = rx;
                                        while let Ok(msg) = rx.recv().await {
                                            let _ = mt.send(msg);
                                        }
                                    });
                                    subscribed_space_ids.insert(space.id.clone());
                                }
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    Some(Ok(WsMessage::Ping(_))) => {
                        last_activity = Instant::now();
                        // 更新 last_ping（只更新自己的连接）
                        {
                            let mut online = state.online_agents.write().await;
                            if let Some(conns) = online.get_mut(&agent.id) {
                                for conn in conns.iter_mut() {
                                    if conn.connected_since == now_ts {
                                        conn.last_ping = chrono::Utc::now().timestamp_millis();
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            // 出站：broadcast 消息转发到 WebSocket
            broadcast_result = merged_rx.recv() => {
                match broadcast_result {
                    Ok(msg) => {
                        // 检查是否是 connection_replaced 通知
                        if msg.contains("CONNECTION_REPLACED") {
                            let _ = sender.send(WsMessage::Text(msg)).await;
                            break; // 旧连接主动退出
                        }
                        if sender.send(WsMessage::Text(msg)).await.is_err() {
                            break; // WebSocket 已关闭
                        }
                    }
                    Err(_) => continue,
                }
            }
            // 心跳超时
            _ = tokio::time::sleep(timeout_remaining) => {
                // 90s 无任何消息，断开连接
                let _ = sender.send(WsMessage::Text(
                    serde_json::to_string(&WsOutgoing::Error {
                        space_id: None,
                        payload: ErrorPayload {
                            code: "HEARTBEAT_TIMEOUT".to_string(),
                            message: "Connection closed: no activity for 90s".to_string(),
                        },
                    }).unwrap_or_default()
                )).await;
                break;
            }
        }
    }

    // 清理：从在线连接表移除自己的连接（保留其他连接）
    {
        let mut online = state.online_agents.write().await;
        if let Some(conns) = online.get_mut(&agent.id) {
            conns.retain(|c| c.connected_since != now_ts);
            if conns.is_empty() {
                online.remove(&agent.id);
            }
        }
    }
}

// ── 消息处理 ─────────────────────────────────────────

/// 处理单条 WebSocket 文本消息
/// pub 以便 openclaw 模块复用
pub async fn handle_ws_message(
    text: &str,
    agent: &Agent,
    state: &AppState,
) -> Result<(), crate::error::GaggleError> {
    let ws_msg: WsIncoming = serde_json::from_str(text)
        .map_err(|e| crate::error::GaggleError::ValidationError(e.to_string()))?;

    match ws_msg {
        WsIncoming::CreateSpace { payload } => {
            let req = crate::negotiation::CreateSpaceRequest {
                name: payload.name,
                invitee_ids: payload.invitee_ids.clone(),
                context: payload.context,
            };

            let space = state.space_manager.create_space(agent, req, payload.my_role.clone()).await?;

            let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceCreated {
                space_id: space.id.clone(),
                payload: SpaceCreatedPayload {
                    space: space.clone(),
                    members: space.agent_ids.clone(),
                },
            })?;

            // 广播到 Space channel
            if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
                let _ = tx.send(broadcast_msg.clone());
            }

            // 主动推送给 invitees 和 creator（支持离线队列）
            let _ = push_event(state, &agent.id, "space_created", &broadcast_msg).await;
            for invitee_id in &payload.invitee_ids {
                let _ = push_event(state, invitee_id, "space_created", &broadcast_msg).await;
            }
        }

        WsIncoming::CreateRfp { payload } => {
            use crate::negotiation::RfpContext;

            let rfp_context = RfpContext {
                allowed_rounds: payload.allowed_rounds,
                evaluation_criteria: payload.evaluation_criteria,
                deadline: payload.deadline,
                share_best_terms: payload.share_best_terms,
            };

            let req = crate::negotiation::CreateRfpRequest {
                name: payload.name,
                provider_ids: payload.provider_ids.clone(),
                rfp_context,
                context: payload.context,
            };

            let space = state.space_manager.create_rfp(agent, req).await?;

            let broadcast_msg = serde_json::to_string(&WsOutgoing::RfpCreated {
                space_id: space.id.clone(),
                payload: RfpCreatedPayload {
                    space: space.clone(),
                    providers: payload.provider_ids.clone(),
                },
            })?;

            // 广播到 Space channel
            if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
                let _ = tx.send(broadcast_msg.clone());
            }

            // 主动推送给 providers 和 creator（支持离线队列）
            let _ = push_event(state, &agent.id, "rfp_created", &broadcast_msg).await;
            for provider_id in &payload.provider_ids {
                let _ = push_event(state, provider_id, "rfp_created", &broadcast_msg).await;
            }
        }

        WsIncoming::JoinSpace { payload } => {
            let space = state
                .space_manager
                .join_space(agent, &payload.space_id)
                .await?;

            let assigned_role = space.get_role(&agent.id).map(|r| r.to_string());

            let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceJoined {
                space_id: payload.space_id.clone(),
                payload: SpaceJoinedPayload {
                    space_id: payload.space_id.clone(),
                    agent_id: agent.id.clone(),
                    assigned_role,
                },
            })?;

            // 广播到 Space channel（通知所有成员）
            if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
                let _ = tx.send(broadcast_msg.clone());
            }

            // 通过 personal channel 确保加入者本人也收到确认（支持离线队列）
            let _ = push_event(state, &agent.id, "space_joined", &broadcast_msg).await;

            // 如果 space 从 Created 变为 Active，广播状态变更通知
            if space.status == SpaceStatus::Active {
                let status_msg = serde_json::to_string(&WsOutgoing::SpaceStatusChanged {
                    space_id: space.id.clone(),
                    payload: SpaceStatusChangedPayload {
                        space_id: space.id.clone(),
                        old_status: "created".to_string(),
                        new_status: "active".to_string(),
                    },
                })?;

                // 广播到 space channel 和所有成员的 personal channel（支持离线队列）
                if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
                    let _ = tx.send(status_msg.clone());
                }
                for member_id in &space.agent_ids {
                    let _ = push_event(state, member_id, "space_status_changed", &status_msg).await;
                }
            }
        }

        WsIncoming::SendMessage { space_id, payload } => {
            let req = crate::negotiation::SendMessageRequest {
                msg_type: payload.msg_type,
                content: payload.content,
                metadata: payload.metadata,
            };

            let message = state
                .space_manager
                .send_message(agent, &space_id, req)
                .await?;

            let broadcast_msg = serde_json::to_string(&WsOutgoing::NewMessage {
                space_id: space_id.clone(),
                payload: NewMessagePayload { message },
            })?;

            if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
                let _ = tx.send(broadcast_msg);
            }
        }

        WsIncoming::SubmitProposal { payload } => {
            let proposal_type = match payload.proposal_type.as_str() {
                "counter" => ProposalType::Counter,
                "best_and_final" => ProposalType::BestAndFinal,
                _ => ProposalType::Initial,
            };

            let req = crate::negotiation::SubmitProposalRequest {
                proposal_type,
                dimensions: payload.dimensions,
                parent_proposal_id: payload.parent_proposal_id,
            };

            let proposal = state
                .space_manager
                .submit_proposal(agent, &payload.space_id, req)
                .await?;

            let broadcast_msg = serde_json::to_string(&WsOutgoing::NewProposal {
                space_id: payload.space_id.clone(),
                payload: NewProposalPayload { proposal },
            })?;

            if let Some(tx) = state
                .space_manager
                .get_broadcast_tx(&payload.space_id)
                .await
            {
                let _ = tx.send(broadcast_msg);
            }
        }

        WsIncoming::RespondToProposal { payload } => {
            let action = match payload.action.as_str() {
                "reject" => ProposalResponseAction::Reject,
                "counter" => ProposalResponseAction::Counter,
                _ => ProposalResponseAction::Accept,
            };

            let req = crate::negotiation::RespondToProposalRequest {
                proposal_id: payload.proposal_id,
                action,
                counter_dimensions: payload.counter_dimensions,
            };

            let (updated_proposal, counter_proposal) = state
                .space_manager
                .respond_to_proposal(agent, &payload.space_id, req)
                .await?;

            // 广播提案更新
            let update_msg = serde_json::to_string(&WsOutgoing::ProposalUpdate {
                space_id: payload.space_id.clone(),
                payload: ProposalUpdatePayload {
                    proposal_id: updated_proposal.id.clone(),
                    status: updated_proposal.status.as_str().to_string(),
                    action: "responded".to_string(),
                },
            })?;

            if let Some(tx) = state
                .space_manager
                .get_broadcast_tx(&payload.space_id)
                .await
            {
                let _ = tx.send(update_msg);
            }

            // 如果有反提案，广播新提案
            if let Some(counter) = counter_proposal {
                let counter_msg = serde_json::to_string(&WsOutgoing::NewProposal {
                    space_id: payload.space_id.clone(),
                    payload: NewProposalPayload { proposal: counter },
                })?;

                if let Some(tx) = state
                    .space_manager
                    .get_broadcast_tx(&payload.space_id)
                    .await
                {
                    let _ = tx.send(counter_msg);
                }
            }
        }

        WsIncoming::ShareBestTerms { payload } => {
            let req = crate::negotiation::ShareBestTermsRequest {
                best_dimensions: payload.best_dimensions,
            };

            let result = state
                .space_manager
                .share_best_terms(agent, &payload.space_id, req)
                .await?;

            let space_id_for_msg = payload.space_id.clone();
            let broadcast_msg = serde_json::to_string(&WsOutgoing::BestTermsShared {
                space_id: payload.space_id.clone(),
                payload: BestTermsSharedPayload {
                    space_id: payload.space_id,
                    best_dimensions: result.best_dimensions,
                    shared_at: result.shared_at,
                },
            })?;

            if let Some(tx) = state
                .space_manager
                .get_broadcast_tx(&space_id_for_msg)
                .await
            {
                let _ = tx.send(broadcast_msg);
            }
        }

        WsIncoming::CloseSpace { space_id, payload } => {
            let req = crate::negotiation::CloseSpaceRequest {
                conclusion: payload.conclusion,
                final_terms: payload.final_terms,
            };

            let space = state
                .space_manager
                .close_space(agent, &space_id, req)
                .await?;

            let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceClosed {
                space_id: space_id.clone(),
                payload: SpaceClosedPayload {
                    conclusion: format!("{:?}", space.status),
                },
            })?;

            if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
                let _ = tx.send(broadcast_msg);
            }
        }

        // P3 命令处理
        WsIncoming::Ping { timestamp } => {
            let pong_msg = serde_json::to_string(&WsOutgoing::Pong {
                timestamp,
                server_time: chrono::Utc::now().timestamp_millis(),
            })?;

            // 通过 personal channel 发送 pong（所有连接）
            let online = state.online_agents.read().await;
            if let Some(conns) = online.get(&agent.id) {
                for conn in conns {
                    let _ = conn.tx.send(pong_msg.clone());
                }
            }
        }

        WsIncoming::ListSpaces => {
            let spaces = state
                .space_manager
                .get_agent_spaces(&agent.id)
                .await
                .unwrap_or_default();

            let space_summaries: Vec<SpaceSummary> = spaces
                .into_iter()
                .map(|space| SpaceSummary {
                    id: space.id.clone(),
                    name: space.name.clone(),
                    status: space.status.as_str().to_string(),
                    space_type: space.space_type.as_str().to_string(),
                    agent_count: space.agent_ids.len(),
                    creator_id: space.creator_id.clone(),
                    created_at: space.created_at,
                    updated_at: space.updated_at,
                    buyer_id: space.buyer_id.clone(),
                    seller_id: space.seller_id.clone(),
                })
                .collect();

            let list_msg = serde_json::to_string(&WsOutgoing::SpacesList {
                payload: SpacesListPayload {
                    spaces: space_summaries,
                },
            })?;

            let online = state.online_agents.read().await;
            if let Some(conns) = online.get(&agent.id) {
                for conn in conns {
                    let _ = conn.tx.send(list_msg.clone());
                }
            }
        }

        WsIncoming::GetMessages { payload } => {
            // 检查 agent 是否是 space 成员
            let space_opt = state
                .space_manager
                .get_space(&payload.space_id)
                .await?;

            let space = space_opt.ok_or_else(|| {
                crate::error::GaggleError::NotFound(format!("Space {} not found", payload.space_id))
            })?;

            if !space.is_member(&agent.id) {
                return Err(crate::error::GaggleError::Unauthorized(format!(
                    "Agent {} is not a member of space {}",
                    agent.id, payload.space_id
                )));
            }

            let limit = payload.limit.unwrap_or(50);
            // 多取 1 条来判断 has_more
            let messages = state
                .space_manager
                .get_messages_for_agent(
                    &payload.space_id,
                    Some(&agent.id),
                    payload.after,
                    limit + 1,
                )
                .await?;

            let has_more = messages.len() > limit as usize;
            let messages = if has_more {
                messages[..messages.len() - 1].to_vec()
            } else {
                messages
            };

            let list_msg = serde_json::to_string(&WsOutgoing::MessagesList {
                space_id: payload.space_id.clone(),
                payload: MessagesListPayload {
                    messages,
                    has_more,
                },
            })?;

            let online = state.online_agents.read().await;
            if let Some(conns) = online.get(&agent.id) {
                for conn in conns {
                    let _ = conn.tx.send(list_msg.clone());
                }
            }
        }

        WsIncoming::CheckOnline { payload } => {
            // 限制查询数量
            if payload.agent_ids.len() > 100 {
                return Err(crate::error::GaggleError::ValidationError(
                    "agent_ids length cannot exceed 100".to_string(),
                ));
            }

            let online = state.online_agents.read().await;
            let mut statuses = HashMap::new();

            for agent_id in &payload.agent_ids {
                let is_online = online.contains_key(agent_id);
                statuses.insert(agent_id.clone(), is_online);
            }

            let status_msg = serde_json::to_string(&WsOutgoing::OnlineStatus {
                payload: OnlineStatusPayload { statuses },
            })?;

            // 通过 personal channel 发送结果（所有连接）
            if let Some(conns) = online.get(&agent.id) {
                for conn in conns {
                    let _ = conn.tx.send(status_msg.clone());
                }
            }
        }

        WsIncoming::LeaveSpace { payload } => {
            // 获取离开前的空间状态以检测变更
            let old_space = state
                .space_manager
                .get_space(&payload.space_id)
                .await?
                .ok_or_else(|| crate::error::GaggleError::SpaceNotFound(payload.space_id.clone()))?;

            let space = state
                .space_manager
                .leave_space(agent, &payload.space_id)
                .await?;

            let status_changed = old_space.status != space.status;

            let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceLeft {
                space_id: payload.space_id.clone(),
                payload: SpaceLeftPayload {
                    space_id: payload.space_id.clone(),
                    agent_id: agent.id.clone(),
                    remaining_agents: space.agent_ids.clone(),
                    space_status: format!("{:?}", space.status).to_lowercase(),
                },
            })?;

            // 广播到 space channel
            if let Some(tx) = state.space_manager.get_broadcast_tx(&payload.space_id).await {
                let _ = tx.send(broadcast_msg.clone());
            }

            // 通过 personal channel 通知所有成员（支持离线队列）
            for member_id in &space.agent_ids {
                let _ = push_event(state, member_id, "space_left", &broadcast_msg).await;
            }
            // 离开者本人也收到确认
            let _ = push_event(state, &agent.id, "space_left", &broadcast_msg).await;

            // 如果状态变为 Cancelled，广播 SpaceStatusChanged
            if status_changed && space.status == SpaceStatus::Cancelled {
                let status_msg = serde_json::to_string(&WsOutgoing::SpaceStatusChanged {
                    space_id: payload.space_id.clone(),
                    payload: SpaceStatusChangedPayload {
                        space_id: payload.space_id.clone(),
                        old_status: format!("{:?}", old_space.status).to_lowercase(),
                        new_status: "cancelled".to_string(),
                    },
                })?;

                if let Some(tx) = state.space_manager.get_broadcast_tx(&payload.space_id).await {
                    let _ = tx.send(status_msg.clone());
                }
                for member_id in &space.agent_ids {
                    let _ = push_event(state, member_id, "space_status_changed", &status_msg).await;
                }
            }
        }

        // Resume 命令：客户端主动请求补发离线事件
        WsIncoming::Resume { last_event_seq } => {
            let after_seq = last_event_seq.unwrap_or(0);
            let pending = state
                .event_queue
                .get_pending(&agent.id, after_seq)
                .await
                .unwrap_or_default();

            let mut seqs_to_mark: Vec<i64> = Vec::new();
            for evt in &pending {
                let replayed = serde_json::to_string(&WsOutgoing::ReplayedEvent {
                    event_seq: evt.event_seq,
                    event_type: evt.event_type.clone(),
                    payload: serde_json::from_str(&evt.payload).unwrap_or(serde_json::Value::Null),
                })?;

                // Resume 的回包通过 personal channel 发送（所有连接）
                let online = state.online_agents.read().await;
                if let Some(conns) = online.get(&agent.id) {
                    for conn in conns {
                        let _ = conn.tx.send(replayed.clone());
                    }
                }
                seqs_to_mark.push(evt.event_seq);
            }

            if !seqs_to_mark.is_empty() {
                let _ = state.event_queue.mark_delivered(&agent.id, &seqs_to_mark).await;
            }

            let ack = serde_json::to_string(&WsOutgoing::ResumeAck {
                replayed_count: seqs_to_mark.len(),
                last_event_seq: seqs_to_mark.last().copied().unwrap_or(after_seq),
            })?;

            let online = state.online_agents.read().await;
            if let Some(conns) = online.get(&agent.id) {
                for conn in conns {
                    let _ = conn.tx.send(ack.clone());
                }
            }
        }
    }

    Ok(())
}
