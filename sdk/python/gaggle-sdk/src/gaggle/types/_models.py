"""Gaggle SDK data models - Pydantic v2 models matching Rust structs."""

from typing import Any, Generic, TypeVar
from pydantic import BaseModel, ConfigDict

from ._enums import (
    AgentType,
    AvailabilityStatus,
    MessageVisibility,
    MessageType,
    NeedStatus,
    PricingModel,
    ProposalStatus,
    ProposalType,
    SpaceStatus,
    SpaceType,
    EventType,
    Outcome,
    ContractStatus,
    MilestoneStatus,
)


class ProviderCapabilities(BaseModel):
    """Provider capability description."""
    model_config = ConfigDict(populate_by_name=True)

    category: str
    tags: list[str] = []


class Space(BaseModel):
    """Negotiation space model."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    name: str
    creator_id: str
    agent_ids: list[str]
    joined_agent_ids: list[str] = []
    status: SpaceStatus
    space_type: SpaceType = SpaceType.BILATERAL
    rfp_context: dict | None = None
    context: dict = {}
    encryption_key: str = ""
    created_at: int
    updated_at: int
    closed_at: int | None = None
    buyer_id: str | None = None
    seller_id: str | None = None


class SpaceMessage(BaseModel):
    """Message within a space."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    space_id: str
    sender_id: str
    msg_type: MessageType
    content: str
    timestamp: int
    round: int
    metadata: dict | None = None
    visibility: MessageVisibility = MessageVisibility.BROADCAST
    recipient_ids: list[str] = []


class ProposalDimensions(BaseModel):
    """Multi-dimensional proposal terms."""
    model_config = ConfigDict(extra="allow", populate_by_name=True)

    price: float | None = None
    timeline_days: float | None = None
    quality_tier: str | None = None
    terms: dict | None = None


class Proposal(BaseModel):
    """Negotiation proposal."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    space_id: str
    sender_id: str
    proposal_type: ProposalType
    dimensions: ProposalDimensions
    round: int
    status: ProposalStatus
    parent_proposal_id: str | None = None
    created_at: int
    updated_at: int


class AgentPublic(BaseModel):
    """Public agent information (without secrets)."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    agent_type: AgentType
    name: str
    metadata: dict = {}
    public_key: str | None = None
    created_at: int
    user_id: str | None = None
    disabled_at: int | None = None
    organization: str | None = None
    callback_url: str | None = None
    online: bool = False


class AgentStatus(BaseModel):
    """Agent online status information."""
    model_config = ConfigDict(populate_by_name=True)

    agent_id: str
    online: bool
    connection_count: int = 0
    connected_since: int | None = None
    last_ping: int | None = None


class DiscoveryProfile(BaseModel):
    """Provider discovery profile."""
    model_config = ConfigDict(populate_by_name=True)

    agent_id: str
    display_name: str
    description: str | None = None
    skills: list[str] = []
    capabilities: ProviderCapabilities
    pricing_model: PricingModel = PricingModel.UNKNOWN
    availability_status: AvailabilityStatus = AvailabilityStatus.UNKNOWN
    min_price: float | None = None
    max_price: float | None = None
    updated_at: int


class ReputationSummary(BaseModel):
    """Reputation score summary."""
    model_config = ConfigDict(populate_by_name=True)

    agent_id: str
    reputation_score: float = 0.0
    total_negotiations: int = 0
    successful: int = 0
    fulfillment_rate: float = 0.0
    avg_rating: float | None = None
    last_updated: int


class ReputationEvent(BaseModel):
    """Reputation event record."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    agent_id: str
    space_id: str
    event_type: EventType
    outcome: Outcome
    rating: int | None = None
    counterparty_id: str
    created_at: int


class ReputationDetail(BaseModel):
    """Detailed reputation information."""
    model_config = ConfigDict(populate_by_name=True)

    summary: ReputationSummary
    recent_events: list[ReputationEvent] = []


class SpaceMembers(BaseModel):
    """Space membership information."""
    model_config = ConfigDict(populate_by_name=True)

    space_id: str
    status: str
    agent_ids: list[str]
    joined_agent_ids: list[str]
    creator_id: str


class RegisterAgentRequest(BaseModel):
    """Request to register a new agent."""
    model_config = ConfigDict(populate_by_name=True)

    agent_type: AgentType
    name: str
    metadata: dict = {}
    public_key: str | None = None
    organization: str | None = None
    callback_url: str | None = None


class RegisterAgentResponse(BaseModel):
    """Response from agent registration."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    agent_type: AgentType
    name: str
    api_key: str
    api_secret: str
    created_at: int
    organization: str | None = None


class RegisterUserRequest(BaseModel):
    """Request to register a new user."""
    model_config = ConfigDict(populate_by_name=True)

    email: str
    password: str
    name: str


class RegisterUserResponse(BaseModel):
    """Response from user registration."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    email: str
    name: str
    api_key: str
    created_at: int


class LoginUserRequest(BaseModel):
    """Request to login a user."""
    model_config = ConfigDict(populate_by_name=True)

    email: str
    password: str


class LoginUserResponse(BaseModel):
    """Response from user login."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    email: str
    name: str
    api_key: str


class User(BaseModel):
    """User information."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    email: str
    name: str
    api_key: str
    created_at: int


class CreateSpaceRequest(BaseModel):
    """Request to create a bilateral space."""
    model_config = ConfigDict(populate_by_name=True)

    name: str
    invitee_ids: list[str]
    context: dict


class CreateRfpRequest(BaseModel):
    """Request to create an RFP space."""
    model_config = ConfigDict(populate_by_name=True)

    name: str
    provider_ids: list[str]
    allowed_rounds: int | None = None
    evaluation_criteria: list[str] | None = None
    deadline: int | None = None
    share_best_terms: bool | None = None
    context: dict = {}


class SendMessageRequest(BaseModel):
    """Request to send a message."""
    model_config = ConfigDict(populate_by_name=True)

    msg_type: MessageType | None = None
    content: str
    metadata: dict | None = None


class SubmitProposalRequest(BaseModel):
    """Request to submit a proposal."""
    model_config = ConfigDict(populate_by_name=True)

    proposal_type: str
    dimensions: ProposalDimensions
    parent_proposal_id: str | None = None


class RespondToProposalRequest(BaseModel):
    """Request to respond to a proposal."""
    model_config = ConfigDict(populate_by_name=True)

    action: str
    counter_dimensions: ProposalDimensions | None = None


class CloseSpaceRequest(BaseModel):
    """Request to close a space."""
    model_config = ConfigDict(populate_by_name=True)

    conclusion: str
    final_terms: dict | None = None


class SubmitEvidenceRequest(BaseModel):
    """Request to submit evidence to blockchain."""
    model_config = ConfigDict(populate_by_name=True)

    evidence_type: str
    hash: str
    metadata: dict | None = None


class UpdateProfileRequest(BaseModel):
    """Request to update provider discovery profile."""
    model_config = ConfigDict(populate_by_name=True)

    display_name: str
    description: str | None = None
    skills: list[str]
    capabilities: ProviderCapabilities
    pricing_model: PricingModel
    availability_status: AvailabilityStatus
    min_price: float | None = None
    max_price: float | None = None


class RateAgentRequest(BaseModel):
    """Request to rate an agent after space conclusion."""
    model_config = ConfigDict(populate_by_name=True)

    agent_id: str
    space_id: str
    event_type: EventType
    outcome: Outcome
    rating: int | None = None
    counterparty_id: str


class RateAgentResponse(BaseModel):
    """Response from agent rating."""
    model_config = ConfigDict(populate_by_name=True)

    event_id: str
    agent_id: str
    new_reputation_score: float


class UpdateAgentRequest(BaseModel):
    """Request to update agent information."""
    model_config = ConfigDict(populate_by_name=True)

    agent_id: str
    name: str | None = None
    metadata: dict | None = None
    organization: str | None = None
    callback_url: str | None = None


# ── Need Broadcast Models ──────────────────────────────────────────


class Need(BaseModel):
    """Need broadcast model."""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    creator_id: str
    title: str
    description: str | None = None
    category: str
    required_skills: list[str] = []
    budget_min: float | None = None
    budget_max: float | None = None
    deadline: int | None = None
    status: NeedStatus
    created_at: int
    updated_at: int
    matched_provider_count: int = 0


class PublishNeedRequest(BaseModel):
    """Request to publish a need broadcast."""
    model_config = ConfigDict(populate_by_name=True)

    title: str
    description: str
    category: str
    required_skills: list[str] = []
    budget_min: float | None = None
    budget_max: float | None = None
    deadline: int | None = None


# ── Phase 3: Negotiation Enhancement Models ──────────────────────


class EvaluationWeights(BaseModel):
    """Weights for multi-dimensional proposal evaluation."""
    model_config = ConfigDict(populate_by_name=True)

    price: float = 0.4
    timeline: float = 0.3
    quality: float = 0.3


class DimensionScores(BaseModel):
    """Per-dimension scores for a proposal."""
    model_config = ConfigDict(populate_by_name=True)

    price_score: float
    timeline_score: float
    quality_score: float


class ProposalScore(BaseModel):
    """Scored result for a single proposal."""
    model_config = ConfigDict(populate_by_name=True)

    proposal_id: str
    provider_id: str
    weighted_score: float
    dimension_scores: DimensionScores


class EvaluateResponse(BaseModel):
    """Response from proposal evaluation endpoint."""
    model_config = ConfigDict(populate_by_name=True)

    scores: list[ProposalScore]
    sorted_by: str


class RoundInfo(BaseModel):
    """Current round information for an RFP space."""
    model_config = ConfigDict(populate_by_name=True)

    current_round: int
    allowed_rounds: int | None = None
    round_status: str  # "open" | "closed" | "expired"
    round_deadline: int | None = None


class NeedToRfpRequest(BaseModel):
    """Request to create an RFP space from an existing need."""
    model_config = ConfigDict(populate_by_name=True)

    provider_ids: list[str]
    allowed_rounds: int | None = None
    evaluation_criteria: list[str] | None = None
    deadline: int | None = None
    share_best_terms: bool | None = None


T = TypeVar("T")


class PaginatedResult(BaseModel, Generic[T]):
    """Paginated result wrapper."""
    model_config = ConfigDict(populate_by_name=True, arbitrary_types_allowed=True)

    items: list[T]
    total: int
    page: int
    page_size: int
    total_pages: int


# ── Phase 4: Contract Management Models ───────────────────────────


class Milestone(BaseModel):
    """合同里程碑模型。"""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    contract_id: str
    title: str
    description: str | None = None
    status: MilestoneStatus
    deliverable_url: str | None = None
    amount: float | None = None
    due_date: int | None = None
    submitted_at: int | None = None
    accepted_at: int | None = None
    created_at: int
    updated_at: int


class Contract(BaseModel):
    """合同模型。"""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    space_id: str
    buyer_id: str
    seller_id: str
    terms: dict
    milestones: list[Milestone]
    status: ContractStatus
    deadline: int | None = None
    created_at: int
    updated_at: int


class CreateMilestoneRequest(BaseModel):
    """创建里程碑请求。"""
    model_config = ConfigDict(populate_by_name=True)

    title: str
    description: str | None = None
    amount: float | None = None
    due_date: int | None = None


class CreateContractRequest(BaseModel):
    """创建合同请求。"""
    model_config = ConfigDict(populate_by_name=True)

    milestones: list[CreateMilestoneRequest]


class SubmitMilestoneRequest(BaseModel):
    """提交里程碑交付物请求。"""
    model_config = ConfigDict(populate_by_name=True)

    deliverable_url: str


class AcceptMilestoneRequest(BaseModel):
    """验收/拒绝里程碑请求。"""
    model_config = ConfigDict(populate_by_name=True)

    accepted: bool
    comment: str | None = None


class DisputeContractRequest(BaseModel):
    """发起争议请求。"""
    model_config = ConfigDict(populate_by_name=True)

    reason: str


# ── Phase 5: Network Effects Models ──────────────────────────────


class AgentTemplate(BaseModel):
    """Agent 模板。"""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    name: str
    description: str
    category: str
    capabilities: list[str]
    default_config: dict


class MarketPrice(BaseModel):
    """市场价格汇总。"""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    category: str
    service_type: str
    avg_price: float
    min_price: float
    max_price: float
    sample_count: int
    period: str
    updated_at: int


class MarketContribution(BaseModel):
    """市场价格贡献。"""
    model_config = ConfigDict(populate_by_name=True)

    id: str
    contributor_id: str
    category: str
    service_type: str
    price: float
    description: str | None = None
    anonymous: bool = False
    created_at: int


class SharePriceRequest(BaseModel):
    """手动贡献价格请求。"""
    model_config = ConfigDict(populate_by_name=True)

    category: str
    service_type: str
    price: float
    description: str | None = None
    anonymous: bool = False


class ScoredDiscoveryProfile(BaseModel):
    """带信誉评分的搜索结果。"""
    model_config = ConfigDict(populate_by_name=True)

    profile: DiscoveryProfile
    reputation_score: float
    capability_match: float
    final_score: float
