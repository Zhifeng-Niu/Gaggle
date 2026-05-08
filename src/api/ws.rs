//! WebSocket处理

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
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

// ── 幂等去重缓存 ────────────────────────────────────────
/// Caches idempotency_key → serialized WsOutgoing response.
/// Prevents duplicate processing when clients retry on network failure.
use std::sync::LazyLock;
use dashmap::DashMap;

#[derive(Clone)]
struct IdempotencyEntry {
    response: String,    // Serialized WsOutgoing for the original request
    created_at: Instant, // For TTL-based cleanup
}

static IDEMPOTENCY_CACHE: LazyLock<DashMap<String, IdempotencyEntry>> = LazyLock::new(DashMap::new);

const IDEMPOTENCY_TTL_SECS: u64 = 300; // 5 minutes

// ── 消息大小限制 ──────────────────────────────────────
/// Maximum allowed content length for a single message (32 KB).
/// Prevents OOM from oversized payloads.
const MAX_CONTENT_LEN: usize = 32 * 1024;
/// Maximum allowed idempotency key length.
const MAX_IDEMPOTENCY_KEY_LEN: usize = 128;

/// Check if we've already processed this idempotency_key.
/// Returns the cached response if found (and not expired).
fn check_idempotency(key: &str) -> Option<String> {
    if let Some(entry) = IDEMPOTENCY_CACHE.get(key) {
        if entry.created_at.elapsed().as_secs() < IDEMPOTENCY_TTL_SECS {
            return Some(entry.response.clone());
        }
        // Expired — remove it
    }
    IDEMPOTENCY_CACHE.remove(key);
    None
}

/// Store a response for this idempotency_key.
fn store_idempotency(key: String, response: String) {
    // Clean up expired entries periodically (every 100 inserts)
    if IDEMPOTENCY_CACHE.len() % 100 == 0 {
        let now = Instant::now();
        IDEMPOTENCY_CACHE.retain(|_, v| now.duration_since(v.created_at).as_secs() < IDEMPOTENCY_TTL_SECS);
    }
    IDEMPOTENCY_CACHE.insert(key, IdempotencyEntry {
        response,
        created_at: Instant::now(),
    });
}

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
    /// 可选：内联提案维度。附带时同时创建 Message + Proposal 记录。
    #[serde(default)]
    pub proposal: Option<InlineProposal>,
}

/// 内联提案 — 嵌入 SendMessagePayload 中，一条消息同时携带结构化提案。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineProposal {
    /// "initial" | "counter" | "best_and_final"，默认 "initial"
    #[serde(default)]
    pub proposal_type: Option<String>,
    /// 多维度报价
    pub dimensions: ProposalDimensions,
    /// 反提案时引用原提案 ID
    #[serde(default)]
    pub parent_proposal_id: Option<String>,
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
    /// Optional: if set, server will reject if space version differs (optimistic locking)
    #[serde(default)]
    pub expected_version: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespondToProposalPayload {
    pub proposal_id: String,
    pub action: String,
    pub counter_dimensions: Option<ProposalDimensions>,
    /// Optional: if set, server will reject if space version differs (optimistic locking)
    #[serde(default)]
    pub expected_version: Option<u64>,
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
pub struct GetProposalsPayload {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSpaceInfoPayload {
    pub space_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalsListPayload {
    pub proposals: Vec<crate::negotiation::Proposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceInfoPayload {
    pub status: String,
    pub version: u64,
    pub rules: Option<serde_json::Value>,
    pub agent_ids: Vec<String>,
    pub creator_id: String,
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
        #[serde(default)]
        space_id: Option<String>,
        payload: Option<JoinSpacePayload>,
    },
    SendMessage {
        #[serde(default)]
        request_id: Option<String>,
        space_id: String,
        payload: SendMessagePayload,
        /// Optional: prevents duplicate processing on retry
        #[serde(default)]
        idempotency_key: Option<String>,
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
        /// Optional: prevents duplicate processing on retry
        #[serde(default)]
        idempotency_key: Option<String>,
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
        #[serde(default)]
        request_id: Option<String>,
        timestamp: Option<i64>,
    },
    ListSpaces {
        #[serde(default)]
        request_id: Option<String>,
    },
    GetMessages {
        #[serde(default)]
        request_id: Option<String>,
        payload: GetMessagesPayload,
    },
    CheckOnline {
        #[serde(default)]
        request_id: Option<String>,
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
        #[serde(default)]
        request_id: Option<String>,
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
    // 可靠投递 ACK：客户端确认已收到 event_seq 及之前的所有事件
    EventAck {
        event_seq: i64,
    },
    // State 同步：客户端请求某个 space 的 state delta
    SyncState {
        #[serde(default)]
        request_id: Option<String>,
        space_id: String,
        #[serde(default)]
        last_known_version: Option<u64>,
    },
    // Frontend queries: get proposals for a space
    GetProposals {
        #[serde(default)]
        request_id: Option<String>,
        payload: GetProposalsPayload,
    },
    // Frontend queries: get space info (status, version, members)
    GetSpaceInfo {
        #[serde(default)]
        request_id: Option<String>,
        payload: GetSpaceInfoPayload,
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
    /// 如果消息关联了提案，此处附带提案详情
    #[serde(default)]
    pub proposal: Option<crate::negotiation::Proposal>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        timestamp: Option<i64>,
        server_time: i64,
    },
    SpacesList {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        payload: SpacesListPayload,
    },
    MessagesList {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        space_id: String,
        payload: MessagesListPayload,
    },
    OnlineStatus {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        space_id: Option<String>,
        payload: ErrorPayload,
    },
    // 离线事件恢复
    ResumeAck {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
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
    // State delta 同步响应
    StateDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        space_id: String,
        from_version: u64,
        to_version: u64,
        events: Vec<crate::negotiation::StateEvent>,
    },
    // Proposals list response (for get_proposals command)
    ProposalsList {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        space_id: String,
        payload: ProposalsListPayload,
    },
    // Space info response (for get_space_info command)
    SpaceInfo {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        space_id: String,
        payload: SpaceInfoPayload,
    },
}

// ── 连接入口 ─────────────────────────────────────────

/// 推送事件到 agent：先持久化到离线队列，在线则附带 event_seq 发送。
/// 客户端收到后应发送 EventAck 确认，服务端才标记为已送达。
/// 如果 agent 离线，事件留在队列中等待 resume 时重放。
pub async fn push_event(
    state: &AppState,
    agent_id: &str,
    event_type: &str,
    payload: &str,
) -> Result<i64, crate::error::GaggleError> {
    // 始终入队（不标记已送达），等客户端 ACK 后再标记
    let seq = state.event_queue.enqueue(agent_id, event_type, payload).await?;

    let online = state.online_agents.read().await;
    if let Some(conns) = online.get(agent_id) {
        if !conns.is_empty() {
            // 在线：注入 event_seq 后发送到该 Agent 的所有 WS 连接
            let wrapped = inject_event_seq(payload, seq);
            for conn in conns {
                let _ = conn.tx.send(wrapped.clone());
            }
            return Ok(seq);
        }
    }
    drop(online);

    // 离线：尝试 webhook 唤醒
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

/// 将 event_seq 注入到 WsOutgoing JSON 的顶层字段中。
/// 客户端可检测 event_seq 存在来区分需要 ACK 的事件和 fire-and-forget 广播。
fn inject_event_seq(payload: &str, event_seq: i64) -> String {
    match serde_json::from_str::<serde_json::Value>(payload) {
        Ok(mut json) => {
            if let Some(obj) = json.as_object_mut() {
                obj.insert(
                    "event_seq".to_string(),
                    serde_json::Value::Number(event_seq.into()),
                );
            }
            serde_json::to_string(&json).unwrap_or_else(|_| payload.to_string())
        }
        Err(_) => payload.to_string(),
    }
}

/// 将 space_version 注入到 WS 广播消息的顶层字段中。
/// 前端可据此实时追踪 Shared Reality 版本，无需 REST 轮询。
fn inject_space_version(payload: &str, version: u64) -> String {
    match serde_json::from_str::<serde_json::Value>(payload) {
        Ok(mut json) => {
            if let Some(obj) = json.as_object_mut() {
                obj.insert(
                    "space_version".to_string(),
                    serde_json::Value::Number(version.into()),
                );
            }
            serde_json::to_string(&json).unwrap_or_else(|_| payload.to_string())
        }
        Err(_) => payload.to_string(),
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
    // Token 鉴权（强制）— 不升级连接，直接返回 HTTP 401
    let token = match auth.token {
        Some(t) if !t.is_empty() => t,
        _ => {
            return (StatusCode::UNAUTHORIZED, "API key token is required").into_response();
        }
    };

    let agent = match state.registry.get_by_api_key(&token).await {
        Ok(Some(a)) => a,
        _ => {
            return (StatusCode::UNAUTHORIZED, "Invalid API key").into_response();
        }
    };

    if agent.id != agent_id {
        return (StatusCode::FORBIDDEN, "Token does not match agent_id").into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(socket, state, agent_id))
}

async fn handle_socket(socket: WebSocket, state: AppState, agent_id: String) {
    let (mut sender, mut receiver) = socket.split();

    // 验证 Agent
    let agent = match state.registry.get_by_id(&agent_id).await {
        Ok(Some(a)) => a,
        _ => {
            let msg = serde_json::to_string(&WsOutgoing::Error {
                request_id: None,
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

    // 多连接支持：追加新连接（不踢旧连接），但有上限
    let max_conns_per_agent: usize = std::env::var("MAX_WS_CONNS_PER_AGENT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
    let now_ts = chrono::Utc::now().timestamp_millis();
    let personal_tx = {
        let mut online = state.online_agents.write().await;

        // 检查连接数上限
        let current_count = online.get(&agent.id).map(|c| c.len()).unwrap_or(0);
        if current_count >= max_conns_per_agent {
            drop(online);
            let msg = serde_json::to_string(&WsOutgoing::Error {
                request_id: None,
                space_id: None,
                payload: ErrorPayload {
                    code: "TOO_MANY_CONNECTIONS".to_string(),
                    message: format!(
                        "Max {} concurrent connections per agent",
                        max_conns_per_agent
                    ),
                },
            })
            .unwrap_or_default();
            let _ = sender.send(WsMessage::Text(msg)).await;
            return;
        }

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

    // 自动重放离线事件（不立即标记已送达，等客户端 EventAck）
    {
        // 先恢复 dead letter 事件（防止事件永久丢失）
        let recovered = state
            .event_queue
            .recover_dead_letters(&agent.id)
            .await
            .unwrap_or_default();
        let recovered_count = recovered.len();

        let pending = if recovered_count > 0 {
            tracing::info!(
                agent_id = %agent.id,
                recovered = recovered_count,
                "Recovered dead letter events"
            );
            // recover_dead_letters returns all pending (including recovered)
            recovered
        } else {
            state
                .event_queue
                .get_pending(&agent.id, 0)
                .await
                .unwrap_or_default()
        };

        if !pending.is_empty() {
            tracing::info!(
                agent_id = %agent.id,
                count = pending.len(),
                "Replaying pending events"
            );
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
            }
            let last_seq = pending.last().map(|e| e.event_seq).unwrap_or(0);
            let ack = serde_json::to_string(&WsOutgoing::ResumeAck {
                request_id: None,
                replayed_count: pending.len(),
                last_event_seq: last_seq,
            })
            .unwrap_or_default();
            let _ = sender.send(WsMessage::Text(ack)).await;
        }
    }

    // Agent Memory Continuity：自动为每个 space 发送 delta
    // 根据 cursor 获取上次见到的版本，发送增量更新
    {
        let cursors = state
            .shared_state_manager
            .get_all_cursors(&agent.id)
            .await
            .unwrap_or_default();
        for (space_id, cursor_ver) in &cursors {
            let (events, current_version) = state
                .shared_state_manager
                .get_events_since(space_id, *cursor_ver)
                .await
                .unwrap_or((vec![], 0));
            if !events.is_empty() {
                let delta = serde_json::to_string(&WsOutgoing::StateDelta {
                    request_id: None,
                    space_id: space_id.clone(),
                    from_version: *cursor_ver,
                    to_version: current_version,
                    events,
                })
                .unwrap_or_default();
                if sender.send(WsMessage::Text(delta)).await.is_err() {
                    break;
                }
                // 更新 cursor
                let _ = state
                    .shared_state_manager
                    .update_cursor(&agent.id, space_id, current_version)
                    .await;
            }
        }
    }

    // 心跳超时计时器
    let mut last_activity = Instant::now();
    let heartbeat_timeout = Duration::from_secs(90);
    let ping_threshold = Duration::from_secs(60); // send server Ping after 60s idle
    let mut ping_sent = false;

    // WS 消息 rate limit: per-agent sliding window
    let ws_rate_max: u32 = std::env::var("WS_RATE_LIMIT_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30); // 默认 30 条/秒
    let ws_rate_window = Duration::from_secs(1);
    let mut ws_msg_count: u32 = 0;
    let mut ws_window_start = Instant::now();

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
                        ping_sent = false;

                        // Rate limit check
                        if ws_window_start.elapsed() >= ws_rate_window {
                            ws_msg_count = 0;
                            ws_window_start = Instant::now();
                        }
                        ws_msg_count += 1;
                        if ws_msg_count > ws_rate_max {
                            // 提取 request_id 用于错误关联
                            let rid = serde_json::from_str::<serde_json::Value>(&text)
                                .ok()
                                .and_then(|v| v.get("request_id").and_then(|r| r.as_str().map(String::from)));
                            let error_msg = serde_json::to_string(&WsOutgoing::Error {
                                request_id: rid,
                                space_id: None,
                                payload: ErrorPayload {
                                    code: "RATE_LIMITED".to_string(),
                                    message: format!("WS message rate limit exceeded ({} msg/s)", ws_rate_max),
                                },
                            }).unwrap_or_default();
                            let _ = sender.send(WsMessage::Text(error_msg)).await;
                            continue;
                        }

                        // 提取 request_id 用于错误关联（避免 handle_ws_message 内部错误丢失 request_id）
                        let msg_request_id = serde_json::from_str::<serde_json::Value>(&text)
                            .ok()
                            .and_then(|v| v.get("request_id").and_then(|r| r.as_str().map(String::from)));

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
                                    request_id: msg_request_id,
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
                        ping_sent = false;
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
            // merged_rx 包含 personal channel 和 space broadcast 的所有消息。
            // push_event() 发到 conn.tx（无 receiver），消息实际通过
            // space broadcast channel → merged_rx 到达 WS sender。
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
                let idle = last_activity.elapsed();
                if idle >= ping_threshold && !ping_sent {
                    // Send a WebSocket Ping to probe the connection
                    let _ = sender.send(WsMessage::Ping(vec![])).await;
                    ping_sent = true;
                    // Continue loop — will check again after remaining timeout
                } else {
                    // 90s total idle → disconnect
                    let _ = sender.send(WsMessage::Text(
                        serde_json::to_string(&WsOutgoing::Error {
                            request_id: None,
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

            let broadcast_raw = serde_json::to_string(&WsOutgoing::SpaceCreated {
                space_id: space.id.clone(),
                payload: SpaceCreatedPayload {
                    space: space.clone(),
                    members: space.agent_ids.clone(),
                },
            })?;
            let broadcast_msg = inject_space_version(&broadcast_raw, space.version);

            // 广播到 Space channel
            if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
                let _ = tx.send(broadcast_msg.clone());
            }

            // 主动推送给 invitees 和 creator（支持离线队列）
            let _ = push_event(state, &agent.id, "space_created", &broadcast_msg).await;
            for invitee_id in &payload.invitee_ids {
                let _ = push_event(state, invitee_id, "space_created", &broadcast_msg).await;
            }

            // Audit: space created
            let _ = state.trace_store.log_action(
                &space.id,
                &agent.id,
                crate::api::trace::AuditAction::SpaceCreated,
                Some("space"),
                Some(&space.id),
                Some(serde_json::json!({"name": space.name, "type": format!("{:?}", space.space_type)})),
            ).await;

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

        WsIncoming::JoinSpace { request_id, space_id, payload } => {
            // Support both formats: { space_id: "..." } and { payload: { space_id: "..." } }
            let effective_space_id = space_id
                .or_else(|| payload.map(|p| p.space_id))
                .ok_or_else(|| crate::error::GaggleError::ValidationError(
                    "missing field `space_id`".to_string(),
                ))?;

            let space = state
                .space_manager
                .join_space(agent, &effective_space_id)
                .await?;

            let assigned_role = space.get_role(&agent.id).map(|r| r.to_string());
            let space_version = space.version;

            let broadcast_raw = serde_json::to_string(&WsOutgoing::SpaceJoined {
                space_id: effective_space_id.clone(),
                payload: SpaceJoinedPayload {
                    space_id: effective_space_id.clone(),
                    agent_id: agent.id.clone(),
                    assigned_role,
                },
            })?;
            let broadcast_msg = inject_space_version(&broadcast_raw, space_version);

            // 广播到 Space channel（通知所有成员）
            if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
                let _ = tx.send(broadcast_msg.clone());
            }

            // 通过 personal channel 确保加入者本人也收到确认（支持离线队列）
            let _ = push_event(state, &agent.id, "space_joined", &broadcast_msg).await;

            // Audit: agent joined space
            let _ = state.trace_store.log_action(
                &effective_space_id,
                &agent.id,
                crate::api::trace::AuditAction::SpaceJoined,
                Some("space"),
                Some(&effective_space_id),
                Some(serde_json::json!({"status": format!("{:?}", space.status)})),
            ).await;

            // 如果 space 从 Created 变为 Active，广播状态变更通知
            if space.status == SpaceStatus::Active {
                let status_raw = serde_json::to_string(&WsOutgoing::SpaceStatusChanged {
                    space_id: space.id.clone(),
                    payload: SpaceStatusChangedPayload {
                        space_id: space.id.clone(),
                        old_status: "created".to_string(),
                        new_status: "active".to_string(),
                        trigger: Some("all_agents_joined".to_string()),
                        agent_id: Some(agent.id.clone()),
                    },
                })?;
                let status_msg = inject_space_version(&status_raw, space_version);

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
                    space_id: Some(effective_space_id),
                    message_id: None,
                    proposal_id: None,
                    error: None,
                }));
            }
        }

        WsIncoming::SendMessage { request_id, space_id, payload, idempotency_key } => {
            // Idempotency: return cached response if this key was already processed
            if let Some(ref key) = idempotency_key {
                if key.len() > MAX_IDEMPOTENCY_KEY_LEN {
                    return Ok(Some(WsOutgoing::Error {
                        request_id: request_id.clone(),
                        space_id: Some(space_id.clone()),
                        payload: ErrorPayload {
                            code: "PAYLOAD_TOO_LARGE".to_string(),
                            message: format!("idempotency_key exceeds {} bytes", MAX_IDEMPOTENCY_KEY_LEN),
                        },
                    }));
                }
                if let Some(cached) = check_idempotency(key) {
                    tracing::debug!(key = %key, "send_message idempotency hit");
                    let resp: WsOutgoing = serde_json::from_str(&cached)?;
                    return Ok(Some(resp));
                }
            }

            // Content size validation
            if payload.content.len() > MAX_CONTENT_LEN {
                return Ok(Some(WsOutgoing::Error {
                    request_id: request_id.clone(),
                    space_id: Some(space_id.clone()),
                    payload: ErrorPayload {
                        code: "PAYLOAD_TOO_LARGE".to_string(),
                        message: format!(
                            "message content exceeds {} bytes (got {})",
                            MAX_CONTENT_LEN, payload.content.len()
                        ),
                    },
                }));
            }

            let req = crate::negotiation::SendMessageRequest {
                msg_type: payload.msg_type.clone(),
                content: payload.content,
                metadata: payload.metadata,
            };

            let message = state
                .space_manager
                .send_message(agent, &space_id, req)
                .await?;

            let msg_id = message.id.clone();
            let mut proposal_obj: Option<crate::negotiation::Proposal> = None;
            let mut proposal_id_for_ack: Option<String> = None;

            // ── 内联提案：如果 payload.proposal 存在，同时创建 Proposal 记录 ──
            if let Some(inline) = payload.proposal {
                let ptype = match inline.proposal_type.as_deref() {
                    Some("counter") => ProposalType::Counter,
                    Some("best_and_final") => ProposalType::BestAndFinal,
                    _ => ProposalType::Initial,
                };
                let prop_req = crate::negotiation::SubmitProposalRequest {
                    proposal_type: ptype,
                    dimensions: inline.dimensions,
                    parent_proposal_id: inline.parent_proposal_id,
                };
                let proposal = state
                    .space_manager
                    .submit_proposal(agent, &space_id, prop_req)
                    .await?;
                proposal_id_for_ack = Some(proposal.id.clone());
                proposal_obj = Some(proposal);
            }

            // ── Acceptance / Rejection：自动更新关联 Proposal 状态 ──
            if matches!(payload.msg_type, MessageType::Acceptance | MessageType::Rejection) {
                if let Some(ref meta) = message.metadata {
                    if let Some(pid) = meta.get("proposal_id").and_then(|v| v.as_str()) {
                        let action = if matches!(payload.msg_type, MessageType::Acceptance) {
                            ProposalResponseAction::Accept
                        } else {
                            ProposalResponseAction::Reject
                        };
                        let resp_req = crate::negotiation::RespondToProposalRequest {
                            proposal_id: pid.to_string(),
                            action,
                            counter_dimensions: None,
                        };
                        // 忽略错误 — 提案可能已被处理
                        let _ = state.space_manager.respond_to_proposal(agent, &space_id, resp_req).await;
                    }
                }
            }

            // Phase 7: 按 visibility 规则过滤投递
            {
                let sp_opt = state.space_manager.get_space(&space_id).await.ok().flatten();
                if let Some(sp) = sp_opt {
                    let rules = &sp.rules;
                    let broadcast_msg = serde_json::to_string(&WsOutgoing::NewMessage {
                        space_id: space_id.clone(),
                        payload: NewMessagePayload {
                            message: message.clone(),
                            proposal: proposal_obj.clone(),
                        },
                    })?;

                    // 对每个 joined agent 按 visibility 规则单独投递
                    for member_id in &sp.joined_agent_ids {
                        if VisibilityEngine::should_deliver_message(rules, &message, member_id, &sp) {
                            let _ = push_event(state, member_id, "new_message", &broadcast_msg).await;
                        }
                    }
                } else {
                    // fallback: 无 space 信息时走原 broadcast 逻辑
                    let broadcast_msg = serde_json::to_string(&WsOutgoing::NewMessage {
                        space_id: space_id.clone(),
                        payload: NewMessagePayload {
                            message,
                            proposal: proposal_obj.clone(),
                        },
                    })?;
                    if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
                        let _ = tx.send(broadcast_msg);
                    }
                }
            }

            // 向后兼容：如果有内联提案，也广播 NewProposal 事件
            if let Some(ref proposal) = proposal_obj {
                let prop_broadcast = serde_json::to_string(&WsOutgoing::NewProposal {
                    space_id: space_id.clone(),
                    payload: NewProposalPayload { proposal: proposal.clone() },
                })?;
                if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
                    let _ = tx.send(prop_broadcast);
                }
                // 也 push_event 给所有 space 成员
                if let Ok(Some(sp)) = state.space_manager.get_space(&space_id).await {
                    for member_id in &sp.agent_ids {
                        let _ = push_event(state, member_id, "new_proposal", &serde_json::to_string(
                            &WsOutgoing::NewProposal {
                                space_id: space_id.clone(),
                                payload: NewProposalPayload { proposal: proposal.clone() },
                            },
                        )?).await;
                    }
                }
            }

            if let Some(rid) = request_id {
                let ack = WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space_id),
                    message_id: Some(msg_id),
                    proposal_id: proposal_id_for_ack,
                    error: None,
                };
                if let Some(key) = idempotency_key {
                    store_idempotency(key, serde_json::to_string(&ack)?);
                }
                return Ok(Some(ack));
            }

            // 审计追踪
            let _ = state.trace_store.log_action(
                &space_id,
                &agent.id,
                crate::api::trace::AuditAction::MessageSent,
                Some("message"),
                Some(&msg_id),
                Some(serde_json::json!({"msg_type": format!("{:?}", payload.msg_type)})),
            ).await;
        }

        WsIncoming::SubmitProposal { request_id, space_id, payload, idempotency_key } => {
            // Idempotency: return cached response if this key was already processed
            if let Some(ref key) = idempotency_key {
                if key.len() > MAX_IDEMPOTENCY_KEY_LEN {
                    return Ok(Some(WsOutgoing::Error {
                        request_id: request_id.clone(),
                        space_id: Some(space_id.clone()),
                        payload: ErrorPayload {
                            code: "PAYLOAD_TOO_LARGE".to_string(),
                            message: format!("idempotency_key exceeds {} bytes", MAX_IDEMPOTENCY_KEY_LEN),
                        },
                    }));
                }
                if let Some(cached) = check_idempotency(key) {
                    tracing::debug!(key = %key, "submit_proposal idempotency hit");
                    let resp: WsOutgoing = serde_json::from_str(&cached)?;
                    return Ok(Some(resp));
                }
            }

            // Optimistic locking: reject if space version has changed
            if let Some(expected) = payload.expected_version {
                if let Ok(Some(space)) = state.space_manager.get_space(&space_id).await {
                    if space.version != expected {
                        return Ok(Some(WsOutgoing::Error {
                            request_id: request_id.clone(),
                            space_id: Some(space_id.clone()),
                            payload: ErrorPayload {
                                code: "VERSION_CONFLICT".to_string(),
                                message: format!(
                                    "space version mismatch: expected {} but current is {}",
                                    expected, space.version
                                ),
                            },
                        }));
                    }
                }
            }

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
            let broadcast_raw = serde_json::to_string(&WsOutgoing::NewProposal {
                space_id: space_id.clone(),
                payload: NewProposalPayload { proposal },
            })?;
            let space_ver = state.space_manager.get_space(&space_id).await?.map(|s| s.version).unwrap_or(0);
            let broadcast_msg = inject_space_version(&broadcast_raw, space_ver);

            if let Some(tx) = state
                .space_manager
                .get_broadcast_tx(&space_id)
                .await
            {
                let _ = tx.send(broadcast_msg);
            }

            // 审计追踪
            let _ = state.trace_store.log_action(
                &space_id,
                &agent.id,
                crate::api::trace::AuditAction::ProposalSubmitted,
                Some("proposal"),
                Some(&proposal_id),
                Some(serde_json::json!({"proposal_type": payload.proposal_type})),
            ).await;

            if let Some(rid) = request_id {
                let ack = WsOutgoing::Ack {
                    request_id: rid,
                    result: "ok".to_string(),
                    space_id: Some(space_id),
                    message_id: None,
                    proposal_id: Some(proposal_id),
                    error: None,
                };
                if let Some(key) = idempotency_key {
                    store_idempotency(key, serde_json::to_string(&ack)?);
                }
                return Ok(Some(ack));
            }
        }

        WsIncoming::RespondToProposal { request_id, space_id, payload } => {
            // Optimistic locking: reject if space version has changed
            if let Some(expected) = payload.expected_version {
                if let Ok(Some(space)) = state.space_manager.get_space(&space_id).await {
                    if space.version != expected {
                        return Ok(Some(WsOutgoing::Error {
                            request_id: request_id.clone(),
                            space_id: Some(space_id.clone()),
                            payload: ErrorPayload {
                                code: "VERSION_CONFLICT".to_string(),
                                message: format!(
                                    "space version mismatch: expected {} but current is {}",
                                    expected, space.version
                                ),
                            },
                        }));
                    }
                }
            }

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
            let space_ver = state.space_manager.get_space(&space_id).await?.map(|s| s.version).unwrap_or(0);

            // 广播提案更新
            let update_raw = serde_json::to_string(&WsOutgoing::ProposalUpdate {
                space_id: space_id.clone(),
                payload: ProposalUpdatePayload {
                    proposal_id: proposal_id.clone(),
                    status: updated_proposal.status.as_str().to_string(),
                    action: "responded".to_string(),
                },
            })?;
            let update_msg = inject_space_version(&update_raw, space_ver);

            if let Some(tx) = state
                .space_manager
                .get_broadcast_tx(&space_id)
                .await
            {
                let _ = tx.send(update_msg);
            }

            // 如果有反提案，广播新提案
            if let Some(counter) = counter_proposal {
                let counter_raw = serde_json::to_string(&WsOutgoing::NewProposal {
                    space_id: space_id.clone(),
                    payload: NewProposalPayload { proposal: counter },
                })?;
                let counter_msg = inject_space_version(&counter_raw, space_ver);

                if let Some(tx) = state
                    .space_manager
                    .get_broadcast_tx(&space_id)
                    .await
                {
                    let _ = tx.send(counter_msg);
                }
            }

            // Audit: proposal responded
            let _ = state.trace_store.log_action(
                &space_id,
                &agent.id,
                crate::api::trace::AuditAction::ProposalResponded,
                Some("proposal"),
                Some(&proposal_id),
                Some(serde_json::json!({"action": payload.action})),
            ).await;

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

            let dims_for_audit = result.best_dimensions.clone();
            let broadcast_raw = serde_json::to_string(&WsOutgoing::BestTermsShared {
                space_id: space_id.clone(),
                payload: BestTermsSharedPayload {
                    space_id: space_id.clone(),
                    best_dimensions: result.best_dimensions,
                    shared_at: result.shared_at,
                },
            })?;
            let space_ver = state.space_manager.get_space(&space_id).await?.map(|s| s.version).unwrap_or(0);
            let broadcast_msg = inject_space_version(&broadcast_raw, space_ver);

            if let Some(tx) = state
                .space_manager
                .get_broadcast_tx(&space_id)
                .await
            {
                let _ = tx.send(broadcast_msg);
            }

            // Audit: best terms shared
            let _ = state.trace_store.log_action(
                &space_id,
                &agent.id,
                crate::api::trace::AuditAction::BestTermsShared,
                Some("space"),
                Some(&space_id),
                Some(serde_json::json!({"dimensions": dims_for_audit})),
            ).await;

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
            let conclusion_for_audit = payload.conclusion.clone();
            let req = crate::negotiation::CloseSpaceRequest {
                conclusion: payload.conclusion,
                final_terms: payload.final_terms,
            };

            let space = state
                .space_manager
                .close_space(agent, &space_id, req)
                .await?;

            let broadcast_raw = serde_json::to_string(&WsOutgoing::SpaceClosed {
                space_id: space_id.clone(),
                payload: SpaceClosedPayload {
                    conclusion: format!("{:?}", space.status),
                },
            })?;
            let broadcast_msg = inject_space_version(&broadcast_raw, space.version);

            if let Some(tx) = state.space_manager.get_broadcast_tx(&space_id).await {
                let _ = tx.send(broadcast_msg);
            }

            // Audit: space closed
            let _ = state.trace_store.log_action(
                &space_id,
                &agent.id,
                crate::api::trace::AuditAction::SpaceClosed,
                Some("space"),
                Some(&space_id),
                Some(serde_json::json!({"conclusion": conclusion_for_audit, "status": format!("{:?}", space.status)})),
            ).await;

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
        WsIncoming::Ping { request_id, timestamp } => {
            let pong_msg = serde_json::to_string(&WsOutgoing::Pong {
                request_id,
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

        WsIncoming::ListSpaces { request_id } => {
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
                request_id,
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

        WsIncoming::GetMessages { request_id, payload } => {
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
                request_id,
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

        WsIncoming::CheckOnline { request_id, payload } => {
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
                request_id,
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
            let space_version = space.version;

            let broadcast_raw = serde_json::to_string(&WsOutgoing::SpaceLeft {
                space_id: space_id.clone(),
                payload: SpaceLeftPayload {
                    space_id: space_id.clone(),
                    agent_id: agent.id.clone(),
                    remaining_agents: space.agent_ids.clone(),
                    space_status: format!("{:?}", space.status).to_lowercase(),
                },
            })?;
            let broadcast_msg = inject_space_version(&broadcast_raw, space_version);

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

            // Audit: agent left space
            let _ = state.trace_store.log_action(
                &space_id,
                &agent.id,
                crate::api::trace::AuditAction::SpaceLeft,
                Some("space"),
                Some(&space_id),
                Some(serde_json::json!({"remaining": space.agent_ids.len()})),
            ).await;

            // 如果状态变为 Cancelled，广播 SpaceStatusChanged
            if status_changed && space.status == SpaceStatus::Cancelled {
                let status_raw = serde_json::to_string(&WsOutgoing::SpaceStatusChanged {
                    space_id: space_id.clone(),
                    payload: SpaceStatusChangedPayload {
                        space_id: space_id.clone(),
                        old_status: format!("{:?}", old_space.status).to_lowercase(),
                        new_status: "cancelled".to_string(),
                        trigger: Some("agent_left".to_string()),
                        agent_id: Some(agent.id.clone()),
                    },
                })?;
                let status_msg = inject_space_version(&status_raw, space_version);

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

        WsIncoming::ListNeeds { request_id, payload } => {
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
                request_id,
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

        // Resume 命令：客户端主动请求补发离线事件（不立即标记已送达，等 EventAck）
        WsIncoming::Resume { last_event_seq } => {
            let after_seq = last_event_seq.unwrap_or(0);
            let pending = state
                .event_queue
                .get_pending(&agent.id, after_seq)
                .await
                .unwrap_or_default();

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
            }

            let last_seq = pending.last().map(|e| e.event_seq).unwrap_or(after_seq);
            let ack = serde_json::to_string(&WsOutgoing::ResumeAck {
                request_id: None,
                replayed_count: pending.len(),
                last_event_seq: last_seq,
            })?;

            let online = state.online_agents.read().await;
            if let Some(conns) = online.get(&agent.id) {
                for conn in conns {
                    let _ = conn.tx.send(ack.clone());
                }
            }
        }

        // 可靠投递 ACK：客户端确认已收到 event_seq 及之前的所有事件
        WsIncoming::EventAck { event_seq } => {
            let count = state
                .event_queue
                .mark_delivered_up_to(&agent.id, event_seq)
                .await
                .unwrap_or(0);
            if count > 0 {
                tracing::debug!(
                    agent_id = %agent.id,
                    event_seq,
                    marked_count = count,
                    "EventAck: cumulative delivery confirmed"
                );
            }
        }

        // State 同步：客户端请求 shared state delta
        WsIncoming::SyncState { request_id, space_id, last_known_version } => {
            // 验证 space 存在 + member 权限
            let space = match state.space_manager.get_space(&space_id).await {
                Ok(Some(s)) => s,
                Ok(None) => {
                    return Ok(Some(WsOutgoing::Error {
                        request_id: request_id.clone(),
                        space_id: Some(space_id),
                        payload: ErrorPayload {
                            code: "NOT_FOUND".to_string(),
                            message: "Space not found".to_string(),
                        },
                    }));
                }
                Err(e) => return Err(e),
            };

            if !space.agent_ids.contains(&agent.id) {
                return Ok(Some(WsOutgoing::Error {
                    request_id: request_id.clone(),
                    space_id: Some(space_id),
                    payload: ErrorPayload {
                        code: "FORBIDDEN".to_string(),
                        message: "Not a member of this space".to_string(),
                    },
                }));
            }

            // 如果客户端未提供 version，使用服务端持久化的 cursor
            let after_version = match last_known_version {
                Some(v) => v,
                None => state
                    .shared_state_manager
                    .get_cursor(&agent.id, &space_id)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(0),
            };

            let (events, current_version) = state
                .shared_state_manager
                .get_events_since(&space_id, after_version)
                .await
                .unwrap_or((vec![], 0));

            // 更新 agent cursor 为当前版本
            if current_version > 0 {
                let _ = state
                    .shared_state_manager
                    .update_cursor(&agent.id, &space_id, current_version)
                    .await;
            }

            return Ok(Some(WsOutgoing::StateDelta {
                request_id,
                space_id,
                from_version: after_version,
                to_version: current_version,
                events,
            }));
        }

        // 获取 space 的所有 proposals
        WsIncoming::GetProposals { request_id, payload } => {
            let space_id = &payload.space_id;

            // 验证 space 存在 + member 权限
            let space = match state.space_manager.get_space(space_id).await {
                Ok(Some(s)) => s,
                Ok(None) => {
                    return Ok(Some(WsOutgoing::Error {
                        request_id: request_id.clone(),
                        space_id: Some(space_id.clone()),
                        payload: ErrorPayload {
                            code: "NOT_FOUND".to_string(),
                            message: "Space not found".to_string(),
                        },
                    }));
                }
                Err(e) => return Err(e),
            };

            if !space.agent_ids.contains(&agent.id) {
                return Ok(Some(WsOutgoing::Error {
                    request_id: request_id.clone(),
                    space_id: Some(space_id.clone()),
                    payload: ErrorPayload {
                        code: "FORBIDDEN".to_string(),
                        message: "Not a member of this space".to_string(),
                    },
                }));
            }

            let proposals = state.space_manager.get_space_proposals(space_id).await?;

            return Ok(Some(WsOutgoing::ProposalsList {
                request_id,
                space_id: space_id.clone(),
                payload: ProposalsListPayload { proposals },
            }));
        }

        // 获取 space 元信息（status, version, round, rules, members）
        WsIncoming::GetSpaceInfo { request_id, payload } => {
            let space_id = &payload.space_id;

            let space = match state.space_manager.get_space(space_id).await {
                Ok(Some(s)) => s,
                Ok(None) => {
                    return Ok(Some(WsOutgoing::Error {
                        request_id: request_id.clone(),
                        space_id: Some(space_id.clone()),
                        payload: ErrorPayload {
                            code: "NOT_FOUND".to_string(),
                            message: "Space not found".to_string(),
                        },
                    }));
                }
                Err(e) => return Err(e),
            };

            if !space.agent_ids.contains(&agent.id) {
                return Ok(Some(WsOutgoing::Error {
                    request_id: request_id.clone(),
                    space_id: Some(space_id.clone()),
                    payload: ErrorPayload {
                        code: "FORBIDDEN".to_string(),
                        message: "Not a member of this space".to_string(),
                    },
                }));
            }

            let rules_value = serde_json::to_value(&space.rules).ok();

            return Ok(Some(WsOutgoing::SpaceInfo {
                request_id,
                space_id: space_id.clone(),
                payload: SpaceInfoPayload {
                    status: space.status.as_str().to_string(),
                    version: space.version,
                    rules: rules_value,
                    agent_ids: space.agent_ids,
                    creator_id: space.creator_id,
                },
            }));
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
    let broadcast_raw = serde_json::to_string(&WsOutgoing::SpaceCreated {
        space_id: space.id.clone(),
        payload: SpaceCreatedPayload {
            space: space.clone(),
            members: space.agent_ids.clone(),
        },
    })?;
    let broadcast_msg = inject_space_version(&broadcast_raw, space.version);
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
    let broadcast_raw = serde_json::to_string(&WsOutgoing::RfpCreated {
        space_id: space.id.clone(),
        payload: RfpCreatedPayload {
            space: space.clone(),
            providers: provider_ids.to_vec(),
        },
    })?;
    let broadcast_msg = inject_space_version(&broadcast_raw, space.version);
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
    let broadcast_raw = serde_json::to_string(&WsOutgoing::SpaceJoined {
        space_id: space.id.clone(),
        payload: SpaceJoinedPayload {
            space_id: space.id.clone(),
            agent_id: agent_id.to_string(),
            assigned_role,
        },
    })?;
    let broadcast_msg = inject_space_version(&broadcast_raw, space.version);
    if let Some(tx) = state.space_manager.get_broadcast_tx(&space.id).await {
        let _ = tx.send(broadcast_msg.clone());
    }
    let _ = push_event(state, agent_id, "space_joined", &broadcast_msg).await;

    // 如果 space 变为 Active，广播状态变更
    if space.status == SpaceStatus::Active {
        let status_raw = serde_json::to_string(&WsOutgoing::SpaceStatusChanged {
            space_id: space.id.clone(),
            payload: SpaceStatusChangedPayload {
                space_id: space.id.clone(),
                old_status: "created".to_string(),
                new_status: "active".to_string(),
                trigger: Some("all_agents_joined".to_string()),
                agent_id: Some(agent_id.to_string()),
            },
        })?;
        let status_msg = inject_space_version(&status_raw, space.version);
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
    let broadcast_raw = serde_json::to_string(&WsOutgoing::SpaceLeft {
        space_id: space.id.clone(),
        payload: SpaceLeftPayload {
            space_id: space.id.clone(),
            agent_id: agent_id.to_string(),
            remaining_agents: space.agent_ids.clone(),
            space_status: space.status.as_str().to_string(),
        },
    })?;
    let broadcast_msg = inject_space_version(&broadcast_raw, space.version);
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
    broadcast_new_message_with_proposal(state, space_id, message, None).await
}

/// 广播新消息（可附带提案信息）
pub async fn broadcast_new_message_with_proposal(
    state: &AppState,
    space_id: &str,
    message: &crate::negotiation::SpaceMessage,
    proposal: Option<&crate::negotiation::Proposal>,
) -> Result<(), crate::error::GaggleError> {
    let broadcast_raw = serde_json::to_string(&WsOutgoing::NewMessage {
        space_id: space_id.to_string(),
        payload: NewMessagePayload {
            message: message.clone(),
            proposal: proposal.cloned(),
        },
    })?;
    let space_ver = state.space_manager.get_space(space_id).await?.map(|s| s.version).unwrap_or(0);
    let broadcast_msg = inject_space_version(&broadcast_raw, space_ver);
    if let Some(tx) = state.space_manager.get_broadcast_tx(space_id).await {
        let _ = tx.send(broadcast_msg.clone());
    }
    // 通过 personal channel 推送给所有成员（确保 REST 创建的 space 也能送达）
    if let Ok(Some(space)) = state.space_manager.get_space(space_id).await {
        for member_id in &space.agent_ids {
            let _ = push_event(state, member_id, "new_message", &broadcast_msg).await;
        }
    }
    Ok(())
}

/// 提交提案后广播通知
pub async fn broadcast_new_proposal(
    state: &AppState,
    space_id: &str,
    proposal: &crate::negotiation::Proposal,
) -> Result<(), crate::error::GaggleError> {
    let broadcast_raw = serde_json::to_string(&WsOutgoing::NewProposal {
        space_id: space_id.to_string(),
        payload: NewProposalPayload { proposal: proposal.clone() },
    })?;
    let space_ver = state.space_manager.get_space(space_id).await?.map(|s| s.version).unwrap_or(0);
    let broadcast_msg = inject_space_version(&broadcast_raw, space_ver);
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
    let broadcast_raw = serde_json::to_string(&WsOutgoing::SpaceClosed {
        space_id: space_id.to_string(),
        payload: SpaceClosedPayload { conclusion: conclusion.to_string() },
    })?;
    let space_ver = state.space_manager.get_space(space_id).await?.map(|s| s.version).unwrap_or(0);
    let broadcast_msg = inject_space_version(&broadcast_raw, space_ver);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_event_seq_adds_top_level_field() {
        let payload = r#"{"type":"new_message","space_id":"s1","payload":{"content":"hi"}}"#;
        let result = inject_event_seq(payload, 42);

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["event_seq"], 42);
        assert_eq!(parsed["type"], "new_message");
        assert_eq!(parsed["space_id"], "s1");
    }

    #[test]
    fn test_inject_event_seq_handles_invalid_json() {
        let payload = "not json";
        let result = inject_event_seq(payload, 7);
        assert_eq!(result, "not json");
    }

    #[test]
    fn test_inject_event_seq_preserves_existing_fields() {
        let payload = r#"{"type":"pong","timestamp":123}"#;
        let result = inject_event_seq(payload, 99);

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["event_seq"], 99);
        assert_eq!(parsed["type"], "pong");
        assert_eq!(parsed["timestamp"], 123);
    }
}
