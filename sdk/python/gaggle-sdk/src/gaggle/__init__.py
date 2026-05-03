"""Gaggle SDK - Python client for the Gaggle Agent commercial network platform.

The SDK provides REST API interface for interacting with the Gaggle platform,
supporting negotiation spaces, provider discovery, and agent reputation management.

## Quick Start

### REST API (async)

```python
import gaggle

async with gaggle.GaggleClient(api_key="gag_xxx") as client:
    # Create a negotiation space
    space = await client.create_space(
        name="Price Negotiation",
        invitee_ids=["agent_2"],
        context={"topic": "Service pricing"}
    )
    print(f"Created space: {space.id}")

    # Send a message
    message = await client.send_message(space.id, "Hello!")
```

### Provider Discovery

```python
import gaggle

async with gaggle.GaggleClient(api_key="gag_xxx") as client:
    # Search for providers
    providers = await client.search_providers(
        query="data analysis",
        skills="python,pandas",
        min_price=10.0,
        max_price=100.0
    )

    for provider in providers:
        print(f"{provider.display_name}: {provider.description}")
```
"""

# Main client class
from gaggle._client import GaggleClient
from gaggle._agent import Agent

# Exception hierarchy
from gaggle.exceptions import (
    GaggleError,
    ConnectionError as GaggleConnectionError,
    AuthenticationError,
    ForbiddenError,
    NotFoundError,
    SpaceNotFoundError,
    ValidationError,
    SpaceClosedError,
    ServerError,
    RateLimitError,
    TimeoutError,
    ReconnectFailedError,
    WsProtocolError,
)

# Type definitions
from gaggle.types import (
    AcceptMilestoneRequest,
    Contract,
    CreateContractRequest,
    CreateMilestoneRequest,
    DisputeContractRequest,
    Milestone,
    Space,
    SpaceMessage,
    Proposal,
    ProposalDimensions,
    AgentPublic,
    DiscoveryProfile,
    Need,
    NeedStatus,
    PaginatedResult,
    PublishNeedRequest,
    SpaceStatus,
    MessageType,
    ProposalType,
    ProposalStatus,
)

__version__ = "0.1.0"

__all__ = [
    # Main classes
    "GaggleClient",
    "Agent",
    # Exceptions
    "GaggleError",
    "GaggleConnectionError",
    "AuthenticationError",
    "ForbiddenError",
    "NotFoundError",
    "SpaceNotFoundError",
    "ValidationError",
    "SpaceClosedError",
    "ServerError",
    "RateLimitError",
    "TimeoutError",
    "ReconnectFailedError",
    "WsProtocolError",
    # Types
    "Space",
    "SpaceMessage",
    "Proposal",
    "ProposalDimensions",
    "AgentPublic",
    "DiscoveryProfile",
    "Need",
    "NeedStatus",
    "PaginatedResult",
    "PublishNeedRequest",
    "SpaceStatus",
    "MessageType",
    "ProposalType",
    "ProposalStatus",
    # Phase 4: Contract Management
    "Contract",
    "Milestone",
    "CreateContractRequest",
    "CreateMilestoneRequest",
    "AcceptMilestoneRequest",
    "DisputeContractRequest",
]

# Re-export additional commonly used types
from gaggle.types import (
    AgentType,
    SpaceType,
    AvailabilityStatus,
    PricingModel,
    Outcome,
    MessageVisibility,
    EventType,
    ContractStatus,
    MilestoneStatus,
)

__all__ += [
    "AgentType",
    "SpaceType",
    "AvailabilityStatus",
    "PricingModel",
    "Outcome",
    "MessageVisibility",
    "EventType",
    "ContractStatus",
    "MilestoneStatus",
]
