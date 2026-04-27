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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

use crate::agents::Agent;
use crate::negotiation::{
    MessageType, ProposalDimensions, ProposalResponseAction, ProposalType, SpaceStatus,
    VisibilityEngine,
};

use super::rest::AppState;

// ── 全局连接 ID 计数器 ──────────────────────────────────
pub(crate) static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

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
    pub proposal_type: String,
    pub dimensions: ProposalDimensions,
    pub parent_proposal_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespondToProposalPayload {
    pub proposal_id: String,
    pub action: String,
    pub counter_dimensions: Option<ProposalDimensions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareBestTermsPayload {
    pub best_dimensions: ProposalDimensions,
}

// Need Broadcast payloads

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishNeedWsPayload {
    pub title: String,
    pub description: String,
    pub category: String,
    #[serde(default)]
    pub required_skills: Vec<String>,
    pub budget_min: Option<f64>,
    pub budget_max: Option<f64>,
    pub deadline: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListNeedsPayload {
    pub category: Option<String>,
    pub skills: Option<String>,
    pub query: Option<String>,
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_size: Option<u32>,
}

// Phase 3: Need → RFP

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeedToRfpPayload {
    pub need_id: String,
    pub provider_ids: Vec<String>,
    pub allowed_rounds: Option<u32>,
    pub deadline: Option<i64>,
    pub share_best_terms: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundAdvancedPayload {
    pub new_round: u32,
    pub round_status: String,
}

// Phase 4: 执行引擎 payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractCreatedPayload {
    pub contract_id: String,
    pub buyer_id: String,
    pub seller_id: String,
    pub milestone_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MilestoneSubmittedPayload {
    pub milestone_id: String,
    pub milestone_title: String,
    pub deliverable_url: String,
    pub submitted_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MilestoneAcceptedPayload {
    pub milestone_id: String,
    pub milestone_title: String,
    pub accepted: bool,
    pub accepted_at: i64,
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
        #[serde(default)]
        request_id: Option<String>,
        payload: CreateSpacePayload,
    },
    CreateRfp {
        #[serde(default)]
        request_id: Option<String>,
        payload: CreateRfpPayload,
    },
    JoinSpace {
        #[serde(default)]
        request_id: Option<String>,
        space_id: String,
    },
    SendMessage {
        #[serde(default)]
        request_id: Option<String>,
        space_id: String,
        payload: SendMessagePayload,
    },
    CloseSpace {
        #[serde(default)]
        request_id: Option<String>,
        space_id: String,
        payload: CloseSpacePayload,
    },
    SubmitProposal {
        #[serde(default)]
        request_id: Option<String>,
        space_id: String,
        payload: SubmitProposalPayload,
    },
    RespondToProposal {
        #[serde(default)]
        request_id: Option<String>,
        space_id: String,
        payload: RespondToProposalPayload,
    },
    ShareBestTerms {
        #[serde(default)]
        request_id: Option<String>,
        space_id: String,
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
        #[serde(default)]
        request_id: Option<String>,
        space_id: String,
    },
    // Need Broadcast
    PublishNeed {
        #[serde(default)]
        request_id: Option<String>,
        payload: PublishNeedWsPayload,
    },
    ListNeeds {
        payload: ListNeedsPayload,
    },
    CancelNeed {
        #[serde(default)]
        request_id: Option<String>,
        need_id: String,
    },
    // Phase 3: Need → RFP
    NeedToRfp {
        #[serde(default)]
        request_id: Option<String>,
        payload: NeedToRfpPayload,
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

// Need Broadcast outgoing payloads

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeedPublishedPayload {
    pub need: crate::discovery::Need,
    pub matched_provider_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeedMatchedPayload {
    pub need: crate::discovery::Need,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeedsListPayload {
    pub needs: Vec<crate::discovery::Need>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
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
    // Need Broadcast
    NeedPublished {
        need_id: String,
        payload: NeedPublishedPayload,
    },
    NeedMatched {
        need_id: String,
        payload: NeedMatchedPayload,
    },
    NeedCancelled {
        need_id: String,
    },
    NeedsList {
        payload: NeedsListPayload,
    },
    // Phase 3: 轮次推进
    RoundAdvanced {
        space_id: String,
        payload: RoundAdvancedPayload,
    },
    // Phase 4: 执行引擎
    ContractCreated {
        space_id: String,
        payload: ContractCreatedPayload,
    },
    MilestoneSubmitted {
        contract_id: String,
        payload: MilestoneSubmittedPayload,
    },
    MilestoneAccepted {
        contract_id: String,
        payload: MilestoneAcceptedPayload,
    },
    ContractCompleted {
        contract_id: String,
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
    // Phase 9: SubSpace events
    SubSpaceCreated {
        parent_space_id: String,
        sub_space: crate::negotiation::SubSpace,
    },
    SubSpaceMessage {
        sub_space_id: String,
        message: crate::negotiation::SpaceMessage,
    },
    SubSpaceProposal {
        sub_space_id: String,
        proposal: crate::negotiation::Proposal,
    },
    SubSpaceClosed {
        sub_space_id: String,
        parent_space_id: String,
        status: crate::negotiation::SpaceStatus,
    },
    // Phase 10: Coalition events
    CoalitionCreated {
        space_id: String,
        coalition: crate::negotiation::Coalition,
    },
    CoalitionMemberJoined {
        coalition_id: String,
        agent_id: String,
    },
    CoalitionMemberLeft {
        coalition_id: String,
        agent_id: String,
    },
    CoalitionStanceUpdated {
        coalition_id: String,
        stance: Option<serde_json::Value>,
    },
    CoalitionDisbanded {
        coalition_id: String,
        space_id: String,
    },
    // Phase 11: Delegation events
    DelegationCreated {
        delegation: crate::negotiation::Delegation,
    },
    DelegationRevoked {
        delegation_id: String,
        space_id: String,
    },
    // Phase 12: Recruitment events
    RecruitmentCreated {
        recruitment: crate::negotiation::RecruitmentRequest,
    },
    RecruitmentAccepted {
        space_id: String,
        recruitment_id: String,
        target_id: String,
    },
    /// 操作确认：匹配客户端 request_id，返回操作结果
    Ack {
        request_id: String,
        result: String,  // "ok" | "error"
        #[serde(skip_serializing_if = "Option::is_none")]
        space_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        proposal_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
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
    let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
    let now_ts = chrono::Utc::now().timestamp_millis();
    let personal_tx = {
        let mut online = state.online_agents.write().await;
        let (tx, _rx) = broadcast::channel::<String>(256);
        let conn_info = super::rest::ConnectionInfo {
            tx: tx.clone(),
            connected_since: now_ts,
            last_ping: std::sync::atomic::AtomicI64::new(now_ts),
            conn_id,
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
        loop {
            match rx.recv().await {
                Ok(msg) => { let _ = mt.send(msg); }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "personal channel lagged, continuing");
                    continue;
                }
                Err(_) => break, // Closed
            }
        }
    });

    // 转发已有的 space broadcast channels
    for (_space_id, rx) in space_rxs {
        let mt = merged_tx.clone();
        tokio::spawn(async move {
            let mut rx = rx;
            loop {
                match rx.recv().await {
                    Ok(msg) => { let _ = mt.send(msg); }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "space channel lagged, continuing");
                        continue;
                    }
                    Err(_) => break,
                }
            }
        });
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

                        match handle_ws_message(
                            &text, &agent, &state
                        ).await {
                            Ok(Some(ack)) => {
                                // 发送 ACK 响应到此连接
                                let json = serde_json::to_string(&ack).unwrap_or_default();
                                let _ = sender.send(WsMessage::Text(json)).await;
                            }
                            Ok(None) => {} // 无 ACK 需要发送
                            Err(e) => {
                                let error_msg = serde_json::to_string(&WsOutgoing::Error {
                                    space_id: None,
                                    payload: ErrorPayload {
                                        code: "INTERNAL_ERROR".to_string(),
                                        message: e.to_string(),
                                    },
                                }).unwrap_or_default();
                                let _ = sender.send(WsMessage::Text(error_msg)).await;
                            }
                        }

                        // 处理完消息后，检查是否有新加入的 space 需要订阅
                        // 仅在可能变更 space 的命令后检查
                        let need_resub = match &text[..] {
                            t if t.contains("create_space") || t.contains("join_space") || t.contains("leave_space") || t.contains("create_rfp") => true,
                            _ => false,
                        };
                        if need_resub {
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
                                            loop {
                                                match rx.recv().await {
                                                    Ok(msg) => { let _ = mt.send(msg); }
                                                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                                        tracing::warn!(skipped = n, "space channel lagged, continuing");
                                                        continue;
                                                    }
                                                    Err(_) => break,
                                                }
                                            }
                                        });
                                        subscribed_space_ids.insert(space.id.clone());
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    Some(Ok(WsMessage::Ping(_))) => {
                        last_activity = Instant::now();
                        // AtomicI64 更新 last_ping，无需 write lock
                        {
                            let online = state.online_agents.read().await;
                            if let Some(conns) = online.get(&agent.id) {
                                for conn in conns {
                                    if conn.conn_id == conn_id {
                                        conn.last_ping.store(
                                            chrono::Utc::now().timestamp_millis(),
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                        break;
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
            conns.retain(|c| c.conn_id != conn_id);
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
) -> Result<Option<WsOutgoing>, crate::error::GaggleError> {
    let ws_msg: WsIncoming = serde_json::from_str(text)
        .map_err(|e| crate::error::GaggleError::ValidationError(e.to_string()))?;

    match ws_msg {
        WsIncoming::CreateSpace { request_id, payload } => {
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

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space.id),
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
            }
        }

        WsIncoming::CreateRfp { request_id, payload } => {
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

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space.id),
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
            }
        }

        WsIncoming::JoinSpace { request_id, space_id } => {
            let space = state
                .space_manager
                .join_space(agent, &space_id)
                .await?;

            let assigned_role = space.get_role(&agent.id).map(|r| r.to_string());

            let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceJoined {
                space_id: space_id.clone(),
                payload: SpaceJoinedPayload {
                    space_id: space_id.clone(),
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

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space_id),
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
            }
        }

        WsIncoming::SendMessage { request_id, space_id, payload } => {
            let req = crate::negotiation::SendMessageRequest {
                msg_type: payload.msg_type,
                content: payload.content,
                metadata: payload.metadata,
            };

            let message = state
                .space_manager
                .send_message(agent, &space_id, req)
                .await?;

            let msg_id = message.id.clone();

            // Phase 7: 按 visibility 规则过滤投递
            {
                let sp_opt = state.space_manager.get_space(&space_id).await.ok().flatten();
                if let Some(sp) = sp_opt {
                    let rules = &sp.rules;
                    let broadcast_msg = serde_json::to_string(&WsOutgoing::NewMessage {
                        space_id: space_id.clone(),
                        payload: NewMessagePayload { message: message.clone() },
                    })?;

                    // 对每个 joined agent 按 visibility 规则单独投递
                    // （不再发 broadcast channel 避免重复投递）
                    for member_id in &sp.joined_agent_ids {
                        if VisibilityEngine::should_deliver_message(rules, &message, member_id, &sp) {
                            let _ = push_event(state, member_id, "new_message", &broadcast_msg).await;
                        }
                    }
                } else {
                    // fallback: 无 space 信息时走原 broadcast 逻辑
                    let broadcast_msg = serde_json::to_string(&WsOutgoing::NewMessage {
                        space_id: space_id.clone(),
                        payload: NewMessagePayload { message },
                    })?;
                    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
                        let _ = tx.send(broadcast_msg);
                    }
                }
            }

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space_id),
                    message_id: Some(msg_id),
                    proposal_id: None,
                    error: None,
                }));
            }
        }

        WsIncoming::SubmitProposal { request_id, space_id, payload } => {
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
                .submit_proposal(agent, &space_id, req)
                .await?;

            let proposal_id = proposal.id.clone();
            let broadcast_msg = serde_json::to_string(&WsOutgoing::NewProposal {
                space_id: space_id.clone(),
                payload: NewProposalPayload { proposal },
            })?;

            if let Some(tx) = state
                .space_manager
                .get_broadcast_tx(&space_id)
                .await
            {
                let _ = tx.send(broadcast_msg);
            }

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space_id),
                    message_id: None,
                    proposal_id: Some(proposal_id),
                    error: None,
                }));
            }
        }

        WsIncoming::RespondToProposal { request_id, space_id, payload } => {
            let action = match payload.action.as_str() {
                "reject" => ProposalResponseAction::Reject,
                "counter" => ProposalResponseAction::Counter,
                _ => ProposalResponseAction::Accept,
            };

            let req = crate::negotiation::RespondToProposalRequest {
                proposal_id: payload.proposal_id.clone(),
                action,
                counter_dimensions: payload.counter_dimensions,
            };

            let (updated_proposal, counter_proposal) = state
                .space_manager
                .respond_to_proposal(agent, &space_id, req)
                .await?;

            let proposal_id = updated_proposal.id.clone();

            // 广播提案更新
            let update_msg = serde_json::to_string(&WsOutgoing::ProposalUpdate {
                space_id: space_id.clone(),
                payload: ProposalUpdatePayload {
                    proposal_id: proposal_id.clone(),
                    status: updated_proposal.status.as_str().to_string(),
                    action: "responded".to_string(),
                },
            })?;

            if let Some(tx) = state
                .space_manager
                .get_broadcast_tx(&space_id)
                .await
            {
                let _ = tx.send(update_msg);
            }

            // 如果有反提案，广播新提案
            if let Some(counter) = counter_proposal {
                let counter_msg = serde_json::to_string(&WsOutgoing::NewProposal {
                    space_id: space_id.clone(),
                    payload: NewProposalPayload { proposal: counter },
                })?;

                if let Some(tx) = state
                    .space_manager
                    .get_broadcast_tx(&space_id)
                    .await
                {
                    let _ = tx.send(counter_msg);
                }
            }

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space_id),
                    message_id: None,
                    proposal_id: Some(proposal_id),
                    error: None,
                }));
            }
        }

        WsIncoming::ShareBestTerms { request_id, space_id, payload } => {
            let req = crate::negotiation::ShareBestTermsRequest {
                best_dimensions: payload.best_dimensions,
            };

            let result = state
                .space_manager
                .share_best_terms(agent, &space_id, req)
                .await?;

            let broadcast_msg = serde_json::to_string(&WsOutgoing::BestTermsShared {
                space_id: space_id.clone(),
                payload: BestTermsSharedPayload {
                    space_id: space_id.clone(),
                    best_dimensions: result.best_dimensions,
                    shared_at: result.shared_at,
                },
            })?;

            if let Some(tx) = state
                .space_manager
                .get_broadcast_tx(&space_id)
                .await
            {
                let _ = tx.send(broadcast_msg);
            }

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space_id),
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
            }
        }

        WsIncoming::CloseSpace { request_id, space_id, payload } => {
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

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space_id),
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
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

        WsIncoming::LeaveSpace { request_id, space_id } => {
            // 获取离开前的空间状态以检测变更
            let old_space = state
                .space_manager
                .get_space(&space_id)
                .await?
                .ok_or_else(|| crate::error::GaggleError::SpaceNotFound(space_id.clone()))?;

            let space = state
                .space_manager
                .leave_space(agent, &space_id)
                .await?;

            let status_changed = old_space.status != space.status;

            let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceLeft {
                space_id: space_id.clone(),
                payload: SpaceLeftPayload {
                    space_id: space_id.clone(),
                    agent_id: agent.id.clone(),
                    remaining_agents: space.agent_ids.clone(),
                    space_status: format!("{:?}", space.status).to_lowercase(),
                },
            })?;

            // 广播到 space channel
            if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
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
                    space_id: space_id.clone(),
                    payload: SpaceStatusChangedPayload {
                        space_id: space_id.clone(),
                        old_status: format!("{:?}", old_space.status).to_lowercase(),
                        new_status: "cancelled".to_string(),
                    },
                })?;

                if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
                    let _ = tx.send(status_msg.clone());
                }
                for member_id in &space.agent_ids {
                    let _ = push_event(state, member_id, "space_status_changed", &status_msg).await;
                }
            }

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space_id),
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
            }
        }

        // Need Broadcast commands
        WsIncoming::PublishNeed { request_id, payload } => {
            let req = crate::discovery::PublishNeedRequest {
                title: payload.title,
                description: payload.description,
                category: payload.category,
                required_skills: payload.required_skills,
                budget_min: payload.budget_min,
                budget_max: payload.budget_max,
                deadline: payload.deadline,
            };

            let need = state.discovery_store.publish_need(&agent.id, req).await?;

            // 匹配 Provider
            let matched_providers = state.discovery_store.find_matching_providers(&need).await?;
            let matched_count = matched_providers.len() as i32;
            if matched_count > 0 {
                let _ = state.discovery_store.update_matched_count(&need.id, matched_count).await;
            }

            let updated_need = if matched_count > 0 {
                NeedPublishedPayload {
                    need: crate::discovery::Need {
                        matched_provider_count: matched_count,
                        ..need.clone()
                    },
                    matched_provider_count: matched_count,
                }
            } else {
                NeedPublishedPayload {
                    need: need.clone(),
                    matched_provider_count: 0,
                }
            };

            // 向 creator 发送 NeedPublished
            let published_msg = serde_json::to_string(&WsOutgoing::NeedPublished {
                need_id: need.id.clone(),
                payload: updated_need,
            })?;
            let _ = push_event(state, &agent.id, "need_published", &published_msg).await;

            // 向每个匹配的 Provider 发送 NeedMatched
            for provider in &matched_providers {
                let matched_msg = serde_json::to_string(&WsOutgoing::NeedMatched {
                    need_id: need.id.clone(),
                    payload: NeedMatchedPayload {
                        need: crate::discovery::Need {
                            matched_provider_count: matched_count,
                            ..need.clone()
                        },
                    },
                })?;
                let _ = push_event(state, &provider.agent_id, "need_matched", &matched_msg).await;
            }

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: None,
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
            }
        }

        WsIncoming::ListNeeds { payload } => {
            let query = crate::discovery::NeedSearchQuery {
                category: payload.category,
                skills: payload.skills,
                query: payload.query,
                status: None,
                page: payload.page,
                page_size: payload.page_size,
            };
            let result = state.discovery_store.search_needs(&query).await?;

            let list_msg = serde_json::to_string(&WsOutgoing::NeedsList {
                payload: NeedsListPayload {
                    needs: result.items,
                    total: result.total,
                    page: result.page,
                    page_size: result.page_size,
                },
            })?;

            let online = state.online_agents.read().await;
            if let Some(conns) = online.get(&agent.id) {
                for conn in conns {
                    let _ = conn.tx.send(list_msg.clone());
                }
            }
        }

        WsIncoming::CancelNeed { request_id, need_id } => {
            // 验证调用者是 creator
            let need = state
                .discovery_store
                .get_need(&need_id)
                .await?
                .ok_or_else(|| {
                    crate::error::GaggleError::NotFound(format!("Need not found: {}", need_id))
                })?;

            if need.creator_id != agent.id {
                return Err(crate::error::GaggleError::Forbidden(
                    "Only the creator can cancel this need".to_string(),
                ));
            }

            state
                .discovery_store
                .update_need_status(&need_id, &crate::discovery::NeedStatus::Cancelled)
                .await?;

            let cancel_msg = serde_json::to_string(&WsOutgoing::NeedCancelled {
                need_id: need_id.clone(),
            })?;

            // 通知 creator
            let _ = push_event(state, &agent.id, "need_cancelled", &cancel_msg).await;

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: None,
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
            }
        }

        // Phase 3: Need → RFP
        WsIncoming::NeedToRfp { request_id, payload } => {
            let need = state
                .discovery_store
                .get_need(&payload.need_id)
                .await?
                .ok_or_else(|| {
                    crate::error::GaggleError::NotFound(format!(
                        "Need not found: {}",
                        payload.need_id
                    ))
                })?;

            if need.creator_id != agent.id {
                return Err(crate::error::GaggleError::Forbidden(
                    "Only the need creator can create RFP".to_string(),
                ));
            }

            if need.status != crate::discovery::NeedStatus::Open {
                return Err(crate::error::GaggleError::ValidationError(
                    format!("Need is not open (status: {:?})", need.status),
                ));
            }

            let rfp_context = crate::negotiation::RfpContext {
                allowed_rounds: payload.allowed_rounds,
                evaluation_criteria: None,
                deadline: payload.deadline,
                share_best_terms: payload.share_best_terms,
            };

            let context = serde_json::json!({
                "need_id": payload.need_id,
                "title": need.title,
                "description": need.description,
                "category": need.category,
                "required_skills": need.required_skills,
                "budget_min": need.budget_min,
                "budget_max": need.budget_max,
            });

            let create_req = crate::negotiation::CreateRfpRequest {
                name: format!("RFP: {}", need.title),
                provider_ids: payload.provider_ids.clone(),
                rfp_context,
                context,
            };

            let space = state.space_manager.create_rfp(&agent, create_req).await?;

            // 更新 Need 状态
            state
                .discovery_store
                .update_need_status(&payload.need_id, &crate::discovery::NeedStatus::Matched)
                .await?;

            // 广播 RFP 创建
            broadcast_rfp_created(state, &agent, &space, &payload.provider_ids).await?;

            if let Some(rid) = request_id {
                return Ok(Some(WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space.id),
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
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

    Ok(None)
}

// ── 公共广播辅助函数（REST + WS 共用） ──────────────────

/// 创建 Space 后广播通知
pub async fn broadcast_space_created(
    state: &AppState,
    agent: &Agent,
    space: &crate::negotiation::Space,
    invitee_ids: &[String],
) -> Result<(), crate::error::GaggleError> {
    let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceCreated {
        space_id: space.id.clone(),
        payload: SpaceCreatedPayload {
            space: space.clone(),
            members: space.agent_ids.clone(),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
        let _ = tx.send(broadcast_msg.clone());
    }
    let _ = push_event(state, &agent.id, "space_created", &broadcast_msg).await;
    for invitee_id in invitee_ids {
        let _ = push_event(state, invitee_id, "space_created", &broadcast_msg).await;
    }
    Ok(())
}

/// 创建 RFP 后广播通知
pub async fn broadcast_rfp_created(
    state: &AppState,
    agent: &Agent,
    space: &crate::negotiation::Space,
    provider_ids: &[String],
) -> Result<(), crate::error::GaggleError> {
    let broadcast_msg = serde_json::to_string(&WsOutgoing::RfpCreated {
        space_id: space.id.clone(),
        payload: RfpCreatedPayload {
            space: space.clone(),
            providers: provider_ids.to_vec(),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
        let _ = tx.send(broadcast_msg.clone());
    }
    let _ = push_event(state, &agent.id, "rfp_created", &broadcast_msg).await;
    for provider_id in provider_ids {
        let _ = push_event(state, provider_id, "rfp_created", &broadcast_msg).await;
    }
    Ok(())
}

/// 加入 Space 后广播通知
pub async fn broadcast_space_joined(
    state: &AppState,
    space: &crate::negotiation::Space,
    agent_id: &str,
) -> Result<(), crate::error::GaggleError> {
    let assigned_role = space.get_role(agent_id).map(|r| r.to_string());
    let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceJoined {
        space_id: space.id.clone(),
        payload: SpaceJoinedPayload {
            space_id: space.id.clone(),
            agent_id: agent_id.to_string(),
            assigned_role,
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
        let _ = tx.send(broadcast_msg.clone());
    }
    let _ = push_event(state, agent_id, "space_joined", &broadcast_msg).await;

    // 如果 space 变为 Active，广播状态变更
    if space.status == SpaceStatus::Active {
        let status_msg = serde_json::to_string(&WsOutgoing::SpaceStatusChanged {
            space_id: space.id.clone(),
            payload: SpaceStatusChangedPayload {
                space_id: space.id.clone(),
                old_status: "created".to_string(),
                new_status: "active".to_string(),
            },
        })?;
        if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
            let _ = tx.send(status_msg.clone());
        }
        for member_id in &space.agent_ids {
            let _ = push_event(state, member_id, "space_status_changed", &status_msg).await;
        }
    }
    Ok(())
}

/// Agent 离开 Space 时广播
pub async fn broadcast_space_left(
    state: &AppState,
    space: &crate::negotiation::Space,
    agent_id: &str,
) -> Result<(), crate::error::GaggleError> {
    let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceLeft {
        space_id: space.id.clone(),
        payload: SpaceLeftPayload {
            space_id: space.id.clone(),
            agent_id: agent_id.to_string(),
            remaining_agents: space.agent_ids.clone(),
            space_status: space.status.as_str().to_string(),
        },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
        let _ = tx.send(broadcast_msg.clone());
    }
    for member_id in &space.agent_ids {
        let _ = push_event(state, member_id, "space_left", &broadcast_msg).await;
    }
    Ok(())
}

/// 发送消息后广播通知
pub async fn broadcast_new_message(
    state: &AppState,
    space_id: &str,
    message: &crate::negotiation::SpaceMessage,
) -> Result<(), crate::error::GaggleError> {
    let broadcast_msg = serde_json::to_string(&WsOutgoing::NewMessage {
        space_id: space_id.to_string(),
        payload: NewMessagePayload { message: message.clone() },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(space_id).await {
        let _ = tx.send(broadcast_msg);
    }
    Ok(())
}

/// 提交提案后广播通知
pub async fn broadcast_new_proposal(
    state: &AppState,
    space_id: &str,
    proposal: &crate::negotiation::Proposal,
) -> Result<(), crate::error::GaggleError> {
    let broadcast_msg = serde_json::to_string(&WsOutgoing::NewProposal {
        space_id: space_id.to_string(),
        payload: NewProposalPayload { proposal: proposal.clone() },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(space_id).await {
        let _ = tx.send(broadcast_msg.clone());
    }
    // 推送给 space 所有成员
    let space = state.space_manager.get_space(space_id).await?;
    if let Some(s) = &space {
        for member_id in &s.agent_ids {
            let _ = push_event(state, member_id, "new_proposal", &broadcast_msg).await;
        }
    }
    Ok(())
}

/// 关闭 Space 后广播通知
pub async fn broadcast_space_closed(
    state: &AppState,
    space_id: &str,
    conclusion: &str,
) -> Result<(), crate::error::GaggleError> {
    let broadcast_msg = serde_json::to_string(&WsOutgoing::SpaceClosed {
        space_id: space_id.to_string(),
        payload: SpaceClosedPayload { conclusion: conclusion.to_string() },
    })?;
    if let Some(tx) = state.space_manager.get_broadcast_tx(space_id).await {
        let _ = tx.send(broadcast_msg.clone());
    }
    // 推送给所有成员
    let space = state.space_manager.get_space(space_id).await?;
    if let Some(s) = &space {
        for member_id in &s.agent_ids {
            let _ = push_event(state, member_id, "space_closed", &broadcast_msg).await;
        }
    }
    Ok(())
}
