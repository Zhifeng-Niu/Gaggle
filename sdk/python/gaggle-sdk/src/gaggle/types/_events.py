"""WebSocket event models - discriminated union with Pydantic v2."""

from typing import Any, Literal
from pydantic import BaseModel

from ._models import (
    AgentStatus,
    Need,
    Proposal,
    ProposalDimensions,
    Space,
    SpaceMessage,
)


# ── Event Payloads ────────────────────────────────────────────────


class SpaceCreatedPayload(BaseModel):
    """Payload for space_created event."""
    space: Space
    members: list[str]


class RfpCreatedPayload(BaseModel):
    """Payload for rfp_created event."""
    space: Space
    providers: list[str]


class SpaceJoinedPayload(BaseModel):
    """Payload for space_joined event."""
    space_id: str
    agent_id: str
    assigned_role: str | None = None


class SpaceLeftPayload(BaseModel):
    """Payload for space_left event."""
    space_id: str
    agent_id: str
    remaining_agents: list[str]
    space_status: str


class NewMessagePayload(BaseModel):
    """Payload for new_message event."""
    message: SpaceMessage


class NewProposalPayload(BaseModel):
    """Payload for new_proposal event."""
    proposal: Proposal


class ProposalUpdatePayload(BaseModel):
    """Payload for proposal_update event."""
    proposal_id: str
    status: str
    action: str


class BestTermsSharedPayload(BaseModel):
    """Payload for best_terms_shared event."""
    space_id: str
    best_dimensions: ProposalDimensions
    shared_at: int


class SpaceClosedPayload(BaseModel):
    """Payload for space_closed event."""
    conclusion: str


class SpaceStatusChangedPayload(BaseModel):
    """Payload for space_status_changed event."""
    space_id: str
    old_status: str
    new_status: str


class SpaceSummary(BaseModel):
    """Summary of a space in list."""
    id: str
    name: str
    status: str
    space_type: str
    agent_count: int
    creator_id: str
    created_at: int
    updated_at: int
    buyer_id: str | None = None
    seller_id: str | None = None


class SpacesListPayload(BaseModel):
    """Payload for spaces_list event."""
    spaces: list[SpaceSummary]


class MessagesListPayload(BaseModel):
    """Payload for messages_list event."""
    messages: list[SpaceMessage]
    has_more: bool


class OnlineStatusPayload(BaseModel):
    """Payload for online_status event."""
    statuses: dict[str, bool]


class AckPayload(BaseModel):
    """Payload for ack event."""
    request_id: str
    result: str  # "ok" or "error"
    space_id: str | None = None
    message_id: str | None = None
    proposal_id: str | None = None
    error: str | None = None


class ErrorPayload(BaseModel):
    """Payload for error event."""
    code: str
    message: str


class ReplayedEventPayload(BaseModel):
    """Payload for replayed_event event."""
    event_seq: int
    event_type: str
    payload: dict[str, Any]


class ResumeAckPayload(BaseModel):
    """Payload for resume_ack event."""
    replayed_count: int
    last_event_seq: int


# ── Need Broadcast Event Payloads ─────────────────────────────────


class NeedPublishedPayload(BaseModel):
    """Payload for need_published event."""
    need: Need
    matched_provider_count: int


class NeedMatchedPayload(BaseModel):
    """Payload for need_matched event."""
    need: Need


class NeedCancelledPayload(BaseModel):
    """Payload for need_cancelled event."""
    need_id: str


class NeedsListPayload(BaseModel):
    """Payload for needs_list event."""
    needs: list[Need]
    total: int
    page: int
    page_size: int


# ── Event Wrapper Models ───────────────────────────────────────────


class SpaceCreatedEvent(BaseModel):
    """Space created event."""
    type: Literal["space_created"]
    space_id: str
    payload: SpaceCreatedPayload


class RfpCreatedEvent(BaseModel):
    """RFP created event."""
    type: Literal["rfp_created"]
    space_id: str
    payload: RfpCreatedPayload


class SpaceJoinedEvent(BaseModel):
    """Space joined event."""
    type: Literal["space_joined"]
    space_id: str
    payload: SpaceJoinedPayload


class SpaceLeftEvent(BaseModel):
    """Space left event."""
    type: Literal["space_left"]
    space_id: str
    payload: SpaceLeftPayload


class NewMessageEvent(BaseModel):
    """New message event."""
    type: Literal["new_message"]
    space_id: str
    payload: NewMessagePayload


class NewProposalEvent(BaseModel):
    """New proposal event."""
    type: Literal["new_proposal"]
    space_id: str
    payload: NewProposalPayload


class ProposalUpdateEvent(BaseModel):
    """Proposal update event."""
    type: Literal["proposal_update"]
    space_id: str
    payload: ProposalUpdatePayload


class BestTermsSharedEvent(BaseModel):
    """Best terms shared event."""
    type: Literal["best_terms_shared"]
    space_id: str
    payload: BestTermsSharedPayload


class SpaceClosedEvent(BaseModel):
    """Space closed event."""
    type: Literal["space_closed"]
    space_id: str
    payload: SpaceClosedPayload


class SpaceStatusChangedEvent(BaseModel):
    """Space status changed event."""
    type: Literal["space_status_changed"]
    space_id: str
    payload: SpaceStatusChangedPayload


class PongEvent(BaseModel):
    """Pong response to ping."""
    type: Literal["pong"]
    timestamp: int | None = None
    server_time: int


class SpacesListEvent(BaseModel):
    """Spaces list event."""
    type: Literal["spaces_list"]
    payload: SpacesListPayload


class MessagesListEvent(BaseModel):
    """Messages list event."""
    type: Literal["messages_list"]
    space_id: str
    payload: MessagesListPayload


class OnlineStatusEvent(BaseModel):
    """Online status event."""
    type: Literal["online_status"]
    payload: OnlineStatusPayload


class AckEvent(BaseModel):
    """Acknowledgment event."""
    type: Literal["ack"]
    request_id: str
    result: str
    space_id: str | None = None
    message_id: str | None = None
    proposal_id: str | None = None
    error: str | None = None


class ErrorEvent(BaseModel):
    """Error event."""
    type: Literal["error"]
    space_id: str | None = None
    payload: ErrorPayload


class ReplayedEvent(BaseModel):
    """Replayed event from offline queue."""
    type: Literal["replayed_event"]
    event_seq: int
    event_type: str
    payload: dict[str, Any]


class ResumeAckEvent(BaseModel):
    """Resume acknowledgment event."""
    type: Literal["resume_ack"]
    replayed_count: int
    last_event_seq: int


class NeedPublishedEvent(BaseModel):
    """Need published event."""
    type: Literal["need_published"]
    payload: NeedPublishedPayload


class NeedMatchedEvent(BaseModel):
    """Need matched event."""
    type: Literal["need_matched"]
    payload: NeedMatchedPayload


class NeedCancelledEvent(BaseModel):
    """Need cancelled event."""
    type: Literal["need_cancelled"]
    payload: NeedCancelledPayload


class NeedsListEvent(BaseModel):
    """Needs list event."""
    type: Literal["needs_list"]
    payload: NeedsListPayload


# Union type for all WebSocket events
WsEvent = (
    SpaceCreatedEvent
    | RfpCreatedEvent
    | SpaceJoinedEvent
    | SpaceLeftEvent
    | NewMessageEvent
    | NewProposalEvent
    | ProposalUpdateEvent
    | BestTermsSharedEvent
    | SpaceClosedEvent
    | SpaceStatusChangedEvent
    | PongEvent
    | SpacesListEvent
    | MessagesListEvent
    | OnlineStatusEvent
    | AckEvent
    | ErrorEvent
    | ReplayedEvent
    | ResumeAckEvent
    | NeedPublishedEvent
    | NeedMatchedEvent
    | NeedCancelledEvent
    | NeedsListEvent
)
