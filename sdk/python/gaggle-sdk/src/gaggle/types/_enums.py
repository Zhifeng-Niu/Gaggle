"""Gaggle SDK enums - matching Rust types exactly."""

from enum import Enum


class AgentType(str, Enum):
    """Agent type in the Gaggle network."""
    CONSUMER = "consumer"
    PROVIDER = "provider"


class SpaceStatus(str, Enum):
    """Status of a negotiation space."""
    CREATED = "created"
    ACTIVE = "active"
    CONCLUDED = "concluded"
    CANCELLED = "cancelled"
    EXPIRED = "expired"


class SpaceType(str, Enum):
    """Type of negotiation space."""
    BILATERAL = "bilateral"
    RFP = "rfp"


class MessageType(str, Enum):
    """Type of message sent in a space."""
    TEXT = "text"
    PROPOSAL = "proposal"
    COUNTER_PROPOSAL = "counter_proposal"
    ACCEPTANCE = "acceptance"
    REJECTION = "rejection"
    WITHDRAWAL = "withdrawal"
    ATTACHMENT = "attachment"
    SYSTEM = "system"


class ProposalType(str, Enum):
    """Type of proposal in negotiation."""
    INITIAL = "initial"
    COUNTER = "counter"
    BEST_AND_FINAL = "best_and_final"


class ProposalStatus(str, Enum):
    """Status of a proposal."""
    PENDING = "pending"
    ACCEPTED = "accepted"
    REJECTED = "rejected"
    SUPERSEDED = "superseded"


class MessageVisibility(str, Enum):
    """Visibility scope of a message."""
    BROADCAST = "broadcast"
    DIRECTED = "directed"
    PRIVATE = "private"


class AvailabilityStatus(str, Enum):
    """Provider's availability status."""
    AVAILABLE = "available"
    BUSY = "busy"
    OFFLINE = "offline"
    UNKNOWN = "unknown"


class PricingModel(str, Enum):
    """Pricing model for provider services."""
    FIXED = "fixed"
    NEGOTIATED = "negotiated"
    CUSTOM = "custom"
    UNKNOWN = "unknown"


class EventType(str, Enum):
    """Type of reputation event."""
    CONCLUDED = "concluded"
    CANCELLED = "cancelled"
    BREACH = "breach"


class Outcome(str, Enum):
    """Outcome of a negotiation event."""
    SUCCESS = "success"
    PARTIAL = "partial"
    FAILURE = "failure"


class NeedStatus(str, Enum):
    """Status of a need broadcast."""
    OPEN = "open"
    MATCHED = "matched"
    EXPIRED = "expired"
    CANCELLED = "cancelled"


class RoundStatus(str, Enum):
    """Status of a negotiation round."""
    OPEN = "open"
    CLOSED = "closed"
    EXPIRED = "expired"


class ContractStatus(str, Enum):
    """合同状态。"""
    ACTIVE = "active"
    COMPLETED = "completed"
    DISPUTED = "disputed"
    CANCELLED = "cancelled"
    EXPIRED = "expired"


class MilestoneStatus(str, Enum):
    """里程碑状态。"""
    PENDING = "pending"
    SUBMITTED = "submitted"
    ACCEPTED = "accepted"
    REJECTED = "rejected"
    DISPUTED = "disputed"
