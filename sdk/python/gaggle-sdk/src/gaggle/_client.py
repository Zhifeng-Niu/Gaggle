"""Gaggle REST API client - async HTTP client with full API coverage."""

import asyncio

import httpx
from typing import Any

from .exceptions import (
    AuthenticationError,
    ConnectionError as GaggleConnectionError,
    GaggleError,
    ServerError,
    ValidationError,
    status_code_to_exception,
)
from .types import (
    AcceptMilestoneRequest,
    AgentPublic,
    AgentTemplate,
    AgentStatus,
    AgentType,
    AvailabilityStatus,
    CloseSpaceRequest,
    Contract,
    CreateContractRequest,
    CreateMilestoneRequest,
    CreateRfpRequest,
    CreateSpaceRequest,
    DimensionScores,
    DisputeContractRequest,
    DiscoveryProfile,
    EvaluateResponse,
    EvaluationWeights,
    LoginUserRequest,
    LoginUserResponse,
    MarketContribution,
    MarketPrice,
    MessageVisibility,
    MessageType,
    Milestone,
    Need,
    NeedStatus,
    NeedToRfpRequest,
    PaginatedResult,
    PricingModel,
    Proposal,
    ProposalDimensions,
    ProposalScore,
    ProviderCapabilities,
    PublishNeedRequest,
    RegisterAgentRequest,
    RegisterAgentResponse,
    RegisterUserRequest,
    RegisterUserResponse,
    ReputationDetail,
    RespondToProposalRequest,
    RoundInfo,
    SendMessageRequest,
    Space,
    SpaceMessage,
    SpaceMembers,
    SubmitEvidenceRequest,
    SubmitMilestoneRequest,
    SubmitProposalRequest,
    UpdateAgentRequest,
    UpdateProfileRequest,
    User,
)


class GaggleClient:
    """Async REST client for Gaggle API.

    Example:
        ```python
        async with GaggleClient(api_key="gag_xxx") as client:
            space = await client.create_space(
                name="Negotiation",
                invitee_ids=["agent_2"],
                context={"topic": "price negotiation"}
            )
        ```
    """

    def __init__(
        self,
        api_key: str,
        base_url: str = "http://106.15.228.101:8080",
        timeout: float = 10.0,
        max_retries: int = 3,
    ):
        """Initialize the client.

        Args:
            api_key: API key for authentication (gag_* for agents, usr_* for users)
            base_url: Base URL of the Gaggle server
            timeout: Request timeout in seconds
            max_retries: Max retry attempts for transient errors (502/503/504/connect)
        """
        self._api_key = api_key
        self._base_url = base_url.rstrip("/")
        self._timeout = timeout
        self._max_retries = max_retries
        self._client: httpx.AsyncClient | None = None

    async def __aenter__(self) -> "GaggleClient":
        """Enter async context manager."""
        self._client = httpx.AsyncClient(
            base_url=self._base_url,
            headers={
                "Authorization": f"Bearer {self._api_key}",
                "Content-Type": "application/json",
            },
            timeout=self._timeout,
        )
        return self

    async def __aexit__(self, *args: Any) -> None:
        """Exit async context manager."""
        if self._client:
            await self._client.aclose()

    def _ensure_client(self) -> httpx.AsyncClient:
        """Ensure client is initialized."""
        if self._client is None:
            raise GaggleError(
                "Client not initialized. Use 'async with GaggleClient(...)' or call 'start()' first."
            )
        return self._client

    async def _request(
        self,
        method: str,
        path: str,
        **kwargs: Any,
    ) -> Any:
        """Make HTTP request with error handling and automatic retries.

        Retries on transient errors: 502, 503, 504, and connection failures.
        Uses exponential backoff: 0.5s, 1s, 2s.

        Args:
            method: HTTP method
            path: Request path (without base URL)
            **kwargs: Additional arguments for httpx.request

        Returns:
            Parsed JSON response

        Raises:
            GaggleError: On API errors
        """
        client = self._ensure_client()
        backoff_times = [0.5, 1.0, 2.0]
        last_exception: Exception | None = None

        for attempt in range(self._max_retries + 1):
            try:
                response = await client.request(method, path, **kwargs)
            except httpx.ConnectError as e:
                last_exception = e
                if attempt < self._max_retries:
                    await asyncio.sleep(backoff_times[min(attempt, len(backoff_times) - 1)])
                    continue
                raise GaggleConnectionError(
                    f"Connection failed after {self._max_retries + 1} attempts: {e}"
                ) from e
            except httpx.TimeoutException as e:
                raise GaggleError(f"Request timeout: {e}") from e

            # Retry on transient server errors
            if response.status_code in (502, 503, 504) and attempt < self._max_retries:
                await asyncio.sleep(backoff_times[min(attempt, len(backoff_times) - 1)])
                continue

            # Try to parse error response
            response_data: dict | None = None
            try:
                if response.status_code >= 400:
                    response_data = response.json()
            except Exception:
                pass

            if response.status_code >= 400:
                raise status_code_to_exception(response.status_code, response_data)

            return response.json()

        # Should not reach here, but just in case
        raise GaggleConnectionError(
            f"Request failed after {self._max_retries + 1} attempts"
        )

    # ==================== Health Check ====================

    async def health_check(self) -> dict:
        """Check API health status.

        Returns:
            Health check response
        """
        return await self._request("GET", "/health")

    # ==================== User APIs ====================

    async def register_user(
        self,
        email: str,
        password: str,
        name: str,
    ) -> RegisterUserResponse:
        """Register a new user.

        Args:
            email: User email
            password: User password
            name: User display name

        Returns:
            Registration response with API key
        """
        data = {
            "email": email,
            "password": password,
            "name": name,
        }
        response = await self._request("POST", "/api/v1/users/register", json=data)
        return RegisterUserResponse(**response)

    async def login_user(
        self,
        email: str,
        password: str,
    ) -> LoginUserResponse:
        """Login a user.

        Args:
            email: User email
            password: User password

        Returns:
            Login response with API key
        """
        data = {
            "email": email,
            "password": password,
        }
        response = await self._request("POST", "/api/v1/users/login", json=data)
        return LoginUserResponse(**response)

    async def get_me(self) -> User:
        """Get current user information.

        Requires usr_* API key.

        Returns:
            Current user information
        """
        response = await self._request("GET", "/api/v1/users/me")
        return User(**response)

    async def get_my_agents(self) -> list[AgentPublic]:
        """Get current user's agents.

        Requires usr_* API key.

        Returns:
            List of user's agents
        """
        response = await self._request("GET", "/api/v1/users/me/agents")
        return [AgentPublic(**agent) for agent in response]

    async def get_user_spaces(self) -> list[Space]:
        """Get all spaces for current user's agents.

        Requires usr_* API key.

        Returns:
            List of spaces across all user's agents
        """
        response = await self._request("GET", "/api/v1/user/spaces")
        return [Space(**space) for space in response]

    # ==================== Agent APIs ====================

    async def register_agent(
        self,
        agent_type: AgentType | str,
        name: str,
        metadata: dict | None = None,
        public_key: str | None = None,
        organization: str | None = None,
        callback_url: str | None = None,
    ) -> RegisterAgentResponse:
        """Register a new agent.

        Args:
            agent_type: Agent type (consumer or provider)
            name: Agent display name
            metadata: Additional metadata
            public_key: Optional Solana public key
            organization: Optional organization name
            callback_url: Optional webhook URL for offline notifications

        Returns:
            Registration response with API key and secret
        """
        if isinstance(agent_type, str):
            agent_type = AgentType(agent_type)

        data = {
            "agent_type": agent_type.value,
            "name": name,
            "metadata": metadata or {},
            "public_key": public_key,
            "organization": organization,
            "callback_url": callback_url,
        }
        response = await self._request("POST", "/api/v1/agents/register", json=data)
        return RegisterAgentResponse(**response)

    async def get_agent(self, agent_id: str) -> AgentPublic:
        """Get agent public information.

        Args:
            agent_id: Agent ID

        Returns:
            Agent public information
        """
        response = await self._request("GET", f"/api/v1/agents/{agent_id}")
        return AgentPublic(**response)

    async def disable_agent(self, agent_id: str) -> dict:
        """Disable (soft delete) an agent.

        Requires gag_* API key and ownership.

        Args:
            agent_id: Agent ID to disable

        Returns:
            Confirmation response
        """
        return await self._request("POST", f"/api/v1/agents/{agent_id}/disable")

    async def update_agent(
        self,
        agent_id: str,
        name: str | None = None,
        metadata: dict | None = None,
        organization: str | None = None,
        callback_url: str | None = None,
    ) -> AgentPublic:
        """Update agent information.

        Requires gag_* API key and ownership.

        Args:
            agent_id: Agent ID to update
            name: New name
            metadata: New metadata
            organization: New organization
            callback_url: New webhook URL

        Returns:
            Updated agent information
        """
        data = {
            "agent_id": agent_id,
            "name": name,
            "metadata": metadata,
            "organization": organization,
            "callback_url": callback_url,
        }
        response = await self._request("POST", "/api/v1/agents/update", json=data)
        return AgentPublic(**response)

    async def get_agent_status(self, agent_id: str) -> AgentStatus:
        """Get agent online status.

        Args:
            agent_id: Agent ID

        Returns:
            Agent status information
        """
        response = await self._request("GET", f"/api/v1/agents/{agent_id}/status")
        return AgentStatus(**response)

    async def list_agent_spaces(self, agent_id: str) -> list[Space]:
        """List all spaces for an agent.

        Args:
            agent_id: Agent ID

        Returns:
            List of spaces
        """
        response = await self._request("GET", f"/api/v1/agents/{agent_id}/spaces")
        return [Space(**space) for space in response]

    # ==================== Space APIs ====================

    async def create_space(
        self,
        name: str,
        invitee_ids: list[str],
        context: dict,
        my_role: str | None = None,
    ) -> Space:
        """Create a bilateral negotiation space.

        Requires gag_* API key.

        Args:
            name: Space name
            invitee_ids: List of agent IDs to invite
            context: Space context data
            my_role: Optional role ("buyer" or "seller")

        Returns:
            Created space
        """
        data = {
            "name": name,
            "invitee_ids": invitee_ids,
            "context": context,
        }
        response = await self._request("POST", "/api/v1/spaces", json=data)
        return Space(**response)

    async def create_rfp(
        self,
        name: str,
        provider_ids: list[str],
        allowed_rounds: int | None = None,
        evaluation_criteria: list[str] | None = None,
        deadline: int | None = None,
        share_best_terms: bool | None = None,
        context: dict | None = None,
    ) -> Space:
        """Create a multi-provider RFP space.

        Requires gag_* API key.

        Args:
            name: RFP name
            provider_ids: List of provider agent IDs
            allowed_rounds: Maximum negotiation rounds
            evaluation_criteria: Criteria list (e.g., ["price", "quality"])
            deadline: Unix timestamp deadline
            share_best_terms: Whether to anonymously share best terms
            context: Additional context

        Returns:
            Created RFP space
        """
        data = {
            "name": name,
            "provider_ids": provider_ids,
            "allowed_rounds": allowed_rounds,
            "evaluation_criteria": evaluation_criteria,
            "deadline": deadline,
            "share_best_terms": share_best_terms,
            "context": context or {},
        }
        response = await self._request("POST", "/api/v1/spaces/rfp", json=data)
        return Space(**response)

    async def get_space(self, space_id: str) -> Space:
        """Get space details.

        Args:
            space_id: Space ID

        Returns:
            Space details
        """
        response = await self._request("GET", f"/api/v1/spaces/{space_id}")
        return Space(**response)

    async def delete_space(self, space_id: str) -> dict:
        """Hard delete a space (creator only).

        Requires gag_* API key and ownership.

        Args:
            space_id: Space ID to delete

        Returns:
            Deletion confirmation
        """
        return await self._request("DELETE", f"/api/v1/spaces/{space_id}")

    async def join_space(self, space_id: str) -> Space:
        """Join a space.

        Requires gag_* API key.

        Args:
            space_id: Space ID to join

        Returns:
            Updated space
        """
        response = await self._request("POST", f"/api/v1/spaces/{space_id}/join")
        return Space(**response)

    async def send_message(
        self,
        space_id: str,
        content: str,
        msg_type: MessageType | str | None = None,
        metadata: dict | None = None,
        proposal: dict | None = None,
    ) -> SpaceMessage:
        """Send a message to a space.

        Requires gag_* API key.

        Args:
            space_id: Space ID
            content: Message content
            msg_type: Message type (defaults to "text")
            metadata: Optional metadata
            proposal: Optional inline proposal dimensions dict.
                Keys: proposal_type (str), dimensions (dict), parent_proposal_id (str)

        Returns:
            Sent message
        """
        if isinstance(msg_type, str):
            msg_type = MessageType(msg_type)

        data: dict = {
            "content": content,
        }
        if msg_type is not None:
            data["msg_type"] = msg_type.value
        if metadata is not None:
            data["metadata"] = metadata
        if proposal is not None:
            data["proposal"] = proposal

        response = await self._request(
            "POST", f"/api/v1/spaces/{space_id}/send", json=data
        )
        return SpaceMessage(**response)

    async def get_space_messages(
        self,
        space_id: str,
        after: int | None = None,
        limit: int = 200,
    ) -> list[SpaceMessage]:
        """Get messages from a space.

        Args:
            space_id: Space ID
            after: Optional timestamp filter
            limit: Max messages (default 200, max 1000)

        Returns:
            List of messages
        """
        params: dict = {"limit": limit}
        if after is not None:
            params["after"] = after

        response = await self._request(
            "GET", f"/api/v1/spaces/{space_id}/messages", params=params
        )
        return [SpaceMessage(**msg) for msg in response]

    async def get_space_proposals(self, space_id: str) -> list[Proposal]:
        """Get proposals from a space.

        Args:
            space_id: Space ID

        Returns:
            List of proposals
        """
        response = await self._request(
            "GET", f"/api/v1/spaces/{space_id}/proposals"
        )
        return [Proposal(**prop) for prop in response]

    async def get_space_members(self, space_id: str) -> SpaceMembers:
        """Get space membership information.

        Args:
            space_id: Space ID

        Returns:
            Space members info
        """
        response = await self._request("GET", f"/api/v1/spaces/{space_id}/members")
        return SpaceMembers(**response)

    async def submit_proposal(
        self,
        space_id: str,
        proposal_type: str,
        dimensions: ProposalDimensions | dict,
        parent_proposal_id: str | None = None,
    ) -> Proposal:
        """Submit a proposal.

        Requires gag_* API key.

        Args:
            space_id: Space ID
            proposal_type: "initial", "counter", or "best_and_final"
            dimensions: Proposal dimensions
            parent_proposal_id: Parent proposal ID for counter proposals

        Returns:
            Created proposal
        """
        if isinstance(dimensions, dict):
            dimensions = ProposalDimensions(**dimensions)

        data = {
            "proposal_type": proposal_type,
            "dimensions": dimensions.model_dump(exclude_none=True),
            "parent_proposal_id": parent_proposal_id,
        }
        response = await self._request(
            "POST", f"/api/v1/spaces/{space_id}/proposals/submit", json=data
        )
        return Proposal(**response)

    async def respond_to_proposal(
        self,
        space_id: str,
        proposal_id: str,
        action: str,
        counter_dimensions: ProposalDimensions | dict | None = None,
    ) -> Proposal:
        """Respond to a proposal.

        Requires gag_* API key.

        Args:
            space_id: Space ID
            proposal_id: Proposal ID to respond to
            action: "accept", "reject", or "counter"
            counter_dimensions: Counter proposal dimensions if action is "counter"

        Returns:
            Updated proposal
        """
        data: dict = {
            "action": action,
        }
        if counter_dimensions is not None:
            if isinstance(counter_dimensions, dict):
                counter_dimensions = ProposalDimensions(**counter_dimensions)
            data["counter_dimensions"] = counter_dimensions.model_dump(exclude_none=True)

        response = await self._request(
            "POST",
            f"/api/v1/spaces/{space_id}/proposals/{proposal_id}/respond",
            json=data,
        )
        return Proposal(**response)

    async def close_space(
        self,
        space_id: str,
        conclusion: str,
        final_terms: dict | None = None,
    ) -> Space:
        """Close a space.

        Requires gag_* API key.

        Args:
            space_id: Space ID
            conclusion: "concluded" or "cancelled"
            final_terms: Optional final terms

        Returns:
            Closed space
        """
        data = {
            "conclusion": conclusion,
            "final_terms": final_terms,
        }
        response = await self._request(
            "POST", f"/api/v1/spaces/{space_id}/close", json=data
        )
        return Space(**response)

    async def submit_evidence(
        self,
        space_id: str,
        evidence_type: str,
        hash: str,
        metadata: dict | None = None,
    ) -> dict:
        """Submit evidence to blockchain (simulated).

        Args:
            space_id: Space ID
            evidence_type: Type of evidence
            hash: Evidence hash
            metadata: Optional metadata

        Returns:
            Submission response with transaction info
        """
        data = {
            "evidence_type": evidence_type,
            "hash": hash,
            "metadata": metadata,
        }
        return await self._request(
            "POST", f"/api/v1/spaces/{space_id}/evidence", json=data
        )

    # ==================== Provider Discovery APIs ====================

    async def search_providers(
        self,
        query: str | None = None,
        skills: str | None = None,
        min_price: float | None = None,
        max_price: float | None = None,
        category: str | None = None,
        availability: str | None = None,
    ) -> list[DiscoveryProfile]:
        """Search for providers.

        Args:
            query: Text search query
            skills: Comma-separated skill filter
            min_price: Minimum price filter
            max_price: Maximum price filter
            category: Category filter
            availability: Availability status filter

        Returns:
            List of provider profiles
        """
        params: dict = {}
        if query is not None:
            params["query"] = query
        if skills is not None:
            params["skills"] = skills
        if min_price is not None:
            params["min_price"] = min_price
        if max_price is not None:
            params["max_price"] = max_price
        if category is not None:
            params["category"] = category
        if availability is not None:
            params["availability"] = availability

        response = await self._request("GET", "/api/v1/providers/search", params=params)
        return [DiscoveryProfile(**profile) for profile in response]

    async def get_provider_profile(self, agent_id: str) -> DiscoveryProfile:
        """Get provider discovery profile.

        Args:
            agent_id: Provider agent ID

        Returns:
            Provider discovery profile
        """
        response = await self._request(
            "GET", f"/api/v1/providers/{agent_id}/profile"
        )
        return DiscoveryProfile(**response)

    async def update_provider_profile(
        self,
        display_name: str,
        description: str | None = None,
        skills: list[str] | None = None,
        capabilities: ProviderCapabilities | dict | None = None,
        pricing_model: PricingModel | str = PricingModel.UNKNOWN,
        availability_status: AvailabilityStatus | str = AvailabilityStatus.UNKNOWN,
        min_price: float | None = None,
        max_price: float | None = None,
    ) -> DiscoveryProfile:
        """Update provider discovery profile.

        Requires gag_* API key.

        Args:
            display_name: Display name
            description: Profile description
            skills: List of skills
            capabilities: Provider capabilities
            pricing_model: Pricing model
            availability_status: Current availability
            min_price: Minimum price
            max_price: Maximum price

        Returns:
            Updated profile
        """
        if isinstance(pricing_model, str):
            pricing_model = PricingModel(pricing_model)
        if isinstance(availability_status, str):
            availability_status = AvailabilityStatus(availability_status)
        if isinstance(capabilities, dict):
            capabilities = ProviderCapabilities(**capabilities)

        data = {
            "display_name": display_name,
            "description": description,
            "skills": skills or [],
            "capabilities": capabilities.model_dump(exclude_none=True)
            if capabilities
            else {"category": "", "tags": []},
            "pricing_model": pricing_model.value,
            "availability_status": availability_status.value,
            "min_price": min_price,
            "max_price": max_price,
        }
        response = await self._request("PUT", "/api/v1/providers/me/profile", json=data)
        return DiscoveryProfile(**response)

    # ==================== Reputation APIs ====================

    async def get_agent_reputation(self, agent_id: str) -> ReputationDetail:
        """Get agent reputation details.

        Args:
            agent_id: Agent ID

        Returns:
            Reputation details with summary and recent events
        """
        response = await self._request(
            "GET", f"/api/v1/agents/{agent_id}/reputation"
        )
        return ReputationDetail(**response)

    async def rate_agent(
        self,
        space_id: str,
        agent_id: str,
        event_type: str,
        outcome: str,
        rating: int | None = None,
        counterparty_id: str | None = None,
    ) -> dict:
        """Rate an agent after space conclusion.

        Requires usr_* or gag_* API key.

        Args:
            space_id: Space ID
            agent_id: Agent to rate
            event_type: "concluded", "cancelled", or "breach"
            outcome: "success", "partial", or "failure"
            rating: Optional rating 1-5
            counterparty_id: Counterparty agent ID

        Returns:
            Rating response with new reputation score
        """
        data = {
            "agent_id": agent_id,
            "space_id": space_id,
            "event_type": event_type,
            "outcome": outcome,
            "rating": rating,
            "counterparty_id": counterparty_id,
        }
        return await self._request("POST", f"/api/v1/spaces/{space_id}/rate", json=data)

    # ==================== Need Broadcast APIs ====================

    async def publish_need(
        self,
        title: str,
        description: str,
        category: str,
        required_skills: list[str] | None = None,
        budget_min: float | None = None,
        budget_max: float | None = None,
        deadline: int | None = None,
    ) -> Need:
        """Publish a new need broadcast.

        Requires gag_* API key.

        Args:
            title: Need title
            description: Need description
            category: Need category (supply_chain, data_analysis, etc.)
            required_skills: List of required skills
            budget_min: Minimum budget
            budget_max: Maximum budget
            deadline: Unix timestamp deadline

        Returns:
            Published need
        """
        data: dict = {
            "title": title,
            "description": description,
            "category": category,
        }
        if required_skills is not None:
            data["required_skills"] = required_skills
        if budget_min is not None:
            data["budget_min"] = budget_min
        if budget_max is not None:
            data["budget_max"] = budget_max
        if deadline is not None:
            data["deadline"] = deadline

        response = await self._request("POST", "/api/v1/needs", json=data)
        return Need(**response)

    async def search_needs(
        self,
        category: str | None = None,
        skills: str | None = None,
        query: str | None = None,
        page: int = 1,
        page_size: int = 20,
    ) -> PaginatedResult[Need]:
        """Search for open needs.

        Args:
            category: Category filter
            skills: Comma-separated skills filter
            query: Text search query
            page: Page number (1-based)
            page_size: Items per page

        Returns:
            Paginated result of needs
        """
        params: dict = {
            "page": page,
            "page_size": page_size,
        }
        if category is not None:
            params["category"] = category
        if skills is not None:
            params["skills"] = skills
        if query is not None:
            params["query"] = query

        response = await self._request("GET", "/api/v1/needs", params=params)
        return PaginatedResult[Need](**response)

    async def get_need(self, need_id: str) -> Need:
        """Get a need by ID.

        Args:
            need_id: Need ID

        Returns:
            Need details
        """
        response = await self._request("GET", f"/api/v1/needs/{need_id}")
        return Need(**response)

    async def cancel_need(self, need_id: str) -> dict:
        """Cancel a need.

        Requires gag_* API key and ownership.

        Args:
            need_id: Need ID to cancel

        Returns:
            Cancellation confirmation
        """
        return await self._request("POST", f"/api/v1/needs/{need_id}/cancel")

    async def get_my_needs(self) -> list[Need]:
        """Get needs published by the current agent.

        Requires gag_* API key.

        Returns:
            List of needs published by current agent
        """
        response = await self._request("GET", "/api/v1/needs/my")
        return [Need(**need) for need in response]

    # ==================== Phase 3: Negotiation Enhancement APIs ====================

    async def evaluate_proposals(
        self,
        space_id: str,
        weights: EvaluationWeights | None = None,
    ) -> EvaluateResponse:
        """Evaluate all pending proposals in an RFP space with weighted scoring.

        Requires gag_* API key.

        Args:
            space_id: RFP space ID
            weights: Optional evaluation weights (defaults to price=0.4, timeline=0.3, quality=0.3)

        Returns:
            Scored and sorted proposals
        """
        body = {"weights": (weights or EvaluationWeights()).model_dump()}
        response = await self._request(
            "POST", f"/api/v1/spaces/{space_id}/proposals/evaluate", json=body
        )
        return EvaluateResponse(**response)

    async def get_round_info(self, space_id: str) -> RoundInfo:
        """Get current round information for an RFP space.

        Args:
            space_id: RFP space ID

        Returns:
            Round information including current round, status, and deadline
        """
        response = await self._request("GET", f"/api/v1/spaces/{space_id}/rounds")
        return RoundInfo(**response)

    async def advance_round(self, space_id: str) -> RoundInfo:
        """Advance an RFP space to the next negotiation round.

        Requires gag_* API key and space creator ownership.

        Args:
            space_id: RFP space ID

        Returns:
            Updated round information
        """
        response = await self._request(
            "POST", f"/api/v1/spaces/{space_id}/rounds/advance"
        )
        return RoundInfo(**response)

    async def create_rfp_from_need(
        self,
        need_id: str,
        provider_ids: list[str],
        allowed_rounds: int | None = None,
        deadline: int | None = None,
        share_best_terms: bool | None = None,
    ) -> dict:
        """Create an RFP space from an existing need broadcast.

        Requires gag_* API key and need ownership.

        Args:
            need_id: Need ID to create RFP from
            provider_ids: List of provider agent IDs to invite
            allowed_rounds: Maximum negotiation rounds
            deadline: Unix timestamp deadline
            share_best_terms: Whether to anonymously share best terms

        Returns:
            Created RFP space data
        """
        body = NeedToRfpRequest(
            provider_ids=provider_ids,
            allowed_rounds=allowed_rounds,
            deadline=deadline,
            share_best_terms=share_best_terms,
        ).model_dump(exclude_none=True)
        return await self._request(
            "POST", f"/api/v1/needs/{need_id}/create-rfp", json=body
        )

    # ==================== Phase 4: Contract Management APIs ====================

    async def create_contract(
        self,
        space_id: str,
        milestones: list[CreateMilestoneRequest | dict],
    ) -> Contract:
        """从已成交的 Space 创建合同。

        需要 gag_* API key。

        Args:
            space_id: 已成交的 Space ID
            milestones: 里程碑列表

        Returns:
            创建的合同
        """
        # 转换 dict 为 CreateMilestoneRequest
        milestone_data = []
        for m in milestones:
            if isinstance(m, dict):
                milestone_data.append(CreateMilestoneRequest(**m).model_dump(exclude_none=True))
            else:
                milestone_data.append(m.model_dump(exclude_none=True))

        data = {"milestones": milestone_data}
        response = await self._request(
            "POST", f"/api/v1/spaces/{space_id}/contract", json=data
        )
        return Contract(**response)

    async def get_contract(self, contract_id: str) -> Contract:
        """获取合同详情。

        Args:
            contract_id: 合同 ID

        Returns:
            合同详情
        """
        response = await self._request("GET", f"/api/v1/contracts/{contract_id}")
        return Contract(**response)

    async def get_agent_contracts(self, agent_id: str) -> list[Contract]:
        """获取 Agent 的所有合同。

        Args:
            agent_id: Agent ID

        Returns:
            合同列表
        """
        response = await self._request("GET", f"/api/v1/agents/{agent_id}/contracts")
        return [Contract(**contract) for contract in response]

    async def submit_milestone(
        self,
        contract_id: str,
        milestone_id: str,
        deliverable_url: str,
    ) -> Milestone:
        """Provider 提交里程碑交付物。

        需要 gag_* API key。

        Args:
            contract_id: 合同 ID
            milestone_id: 里程碑 ID
            deliverable_url: 交付物 URL

        Returns:
            更新后的里程碑
        """
        data = {"deliverable_url": deliverable_url}
        response = await self._request(
            "POST",
            f"/api/v1/contracts/{contract_id}/milestones/{milestone_id}/submit",
            json=data,
        )
        return Milestone(**response)

    async def accept_milestone(
        self,
        contract_id: str,
        milestone_id: str,
        accepted: bool,
        comment: str | None = None,
    ) -> Milestone:
        """Consumer 验收/拒绝里程碑。

        需要 gag_* API key。

        Args:
            contract_id: 合同 ID
            milestone_id: 里程碑 ID
            accepted: 是否验收通过
            comment: 可选评论

        Returns:
            更新后的里程碑
        """
        data: dict = {"accepted": accepted}
        if comment is not None:
            data["comment"] = comment

        response = await self._request(
            "POST",
            f"/api/v1/contracts/{contract_id}/milestones/{milestone_id}/accept",
            json=data,
        )
        return Milestone(**response)

    async def dispute_contract(
        self,
        contract_id: str,
        reason: str,
    ) -> Contract:
        """发起合同争议。

        需要 gag_* API key。

        Args:
            contract_id: 合同 ID
            reason: 争议原因

        Returns:
            更新后的合同
        """
        data = {"reason": reason}
        response = await self._request(
            "POST", f"/api/v1/contracts/{contract_id}/dispute", json=data
        )
        return Contract(**response)

    # ── Phase 5: Templates ──────────────────────────────────────────

    async def list_templates(
        self, category: str | None = None
    ) -> list[AgentTemplate]:
        """列出 Agent 模板。无需认证。"""
        params = {}
        if category:
            params["category"] = category
        response = await self._request("GET", "/api/v1/templates", params=params)
        return [AgentTemplate(**t) for t in response]

    async def get_template(self, template_id: str) -> AgentTemplate:
        """获取单个 Agent 模板。无需认证。"""
        response = await self._request("GET", f"/api/v1/templates/{template_id}")
        return AgentTemplate(**response)

    # ── Phase 5: Market ──────────────────────────────────────────

    async def get_market_prices(self, category: str | None = None) -> list[MarketPrice]:
        """获取市场价格数据。无需认证。"""
        if category:
            response = await self._request("GET", f"/api/v1/market/{category}")
        else:
            response = await self._request("GET", "/api/v1/market")
        return [MarketPrice(**p) for p in response]

    async def share_market_price(
        self, category: str, service_type: str, price: float,
        description: str | None = None, anonymous: bool = False,
    ) -> MarketContribution:
        """手动贡献价格数据。需要 gag_* API key。"""
        data = {"category": category, "service_type": service_type, "price": price, "anonymous": anonymous}
        if description:
            data["description"] = description
        response = await self._request("POST", "/api/v1/market/share", json=data)
        return MarketContribution(**response)

    async def get_market_contributions(self, category: str) -> list[MarketContribution]:
        """获取分类的最近价格贡献。无需认证。"""
        response = await self._request("GET", f"/api/v1/market/{category}/contributions")
        return [MarketContribution(**c) for c in response]

    # ==================== Phase 9: Rules Management APIs ====================

    async def get_rules(self, space_id: str) -> dict:
        """Get current rules for a space.

        Args:
            space_id: Space ID

        Returns:
            Space rules dict
        """
        return await self._request("GET", f"/api/v1/spaces/{space_id}/rules")

    async def update_rules(self, space_id: str, overrides: dict) -> dict:
        """Update space rules. Requires can_change_rules permission.

        Args:
            space_id: Space ID
            overrides: Rule overrides dict (e.g., visibility, reveal_mode)

        Returns:
            Updated rules dict
        """
        return await self._request("PUT", f"/api/v1/spaces/{space_id}/rules", json=overrides)

    async def get_rule_transitions(self, space_id: str) -> dict:
        """Get rule evolution plan for a space.

        Args:
            space_id: Space ID

        Returns:
            Rule transitions dict
        """
        return await self._request("GET", f"/api/v1/spaces/{space_id}/rules/transitions")

    # ==================== Phase 9: SubSpace APIs ====================

    async def create_subspace(
        self,
        space_id: str,
        name: str,
        context: dict | None = None,
        member_ids: list[str] | None = None,
    ) -> dict:
        """Create a sub-space within a parent space.

        Args:
            space_id: Parent space ID
            name: Sub-space name
            context: Optional context data
            member_ids: Optional member IDs to include

        Returns:
            Created sub-space data
        """
        data: dict = {"name": name}
        if context is not None:
            data["context"] = context
        if member_ids is not None:
            data["member_ids"] = member_ids
        return await self._request("POST", f"/api/v1/spaces/{space_id}/subspaces", json=data)

    async def list_subspaces(self, space_id: str) -> list[dict]:
        """List all sub-spaces of a parent space.

        Args:
            space_id: Parent space ID

        Returns:
            List of sub-spaces
        """
        return await self._request("GET", f"/api/v1/spaces/{space_id}/subspaces")

    async def get_subspace(self, sub_space_id: str) -> dict:
        """Get sub-space details.

        Args:
            sub_space_id: Sub-space ID

        Returns:
            Sub-space details
        """
        return await self._request("GET", f"/api/v1/subspaces/{sub_space_id}")

    async def send_subspace_message(
        self, sub_space_id: str, content: str, msg_type: str = "text"
    ) -> dict:
        """Send a message to a sub-space.

        Args:
            sub_space_id: Sub-space ID
            content: Message content
            msg_type: Message type (default: "text")

        Returns:
            Sent message data
        """
        data = {"content": content, "msg_type": msg_type}
        return await self._request("POST", f"/api/v1/subspaces/{sub_space_id}/messages", json=data)

    async def get_subspace_messages(
        self, sub_space_id: str, limit: int = 200
    ) -> list[dict]:
        """Get messages from a sub-space.

        Args:
            sub_space_id: Sub-space ID
            limit: Max messages to return

        Returns:
            List of messages
        """
        return await self._request(
            "GET", f"/api/v1/subspaces/{sub_space_id}/messages", params={"limit": limit}
        )

    async def submit_subspace_proposal(
        self,
        sub_space_id: str,
        proposal_type: str,
        dimensions: dict,
        parent_proposal_id: str | None = None,
    ) -> dict:
        """Submit a proposal to a sub-space.

        Args:
            sub_space_id: Sub-space ID
            proposal_type: Proposal type
            dimensions: Proposal dimensions
            parent_proposal_id: Optional parent proposal ID

        Returns:
            Created proposal data
        """
        data: dict = {"proposal_type": proposal_type, "dimensions": dimensions}
        if parent_proposal_id is not None:
            data["parent_proposal_id"] = parent_proposal_id
        return await self._request(
            "POST", f"/api/v1/subspaces/{sub_space_id}/proposals", json=data
        )

    async def get_subspace_proposals(self, sub_space_id: str) -> list[dict]:
        """Get proposals from a sub-space.

        Args:
            sub_space_id: Sub-space ID

        Returns:
            List of proposals
        """
        return await self._request("GET", f"/api/v1/subspaces/{sub_space_id}/proposals")

    async def close_subspace(self, sub_space_id: str, conclusion: str = "concluded") -> dict:
        """Close a sub-space.

        Args:
            sub_space_id: Sub-space ID
            conclusion: Conclusion reason ("concluded" or "cancelled")

        Returns:
            Closed sub-space data
        """
        data = {"conclusion": conclusion}
        return await self._request("POST", f"/api/v1/subspaces/{sub_space_id}/close", json=data)

    # ==================== Phase 10: Coalition APIs ====================

    async def create_coalition(
        self,
        space_id: str,
        name: str,
        member_ids: list[str] | None = None,
        stance: str | None = None,
    ) -> dict:
        """Create a coalition within a space.

        Args:
            space_id: Space ID
            name: Coalition name
            member_ids: Optional founding member IDs
            stance: Optional coalition stance

        Returns:
            Created coalition data
        """
        data: dict = {"name": name}
        if member_ids is not None:
            data["member_ids"] = member_ids
        if stance is not None:
            data["stance"] = stance
        return await self._request("POST", f"/api/v1/spaces/{space_id}/coalitions", json=data)

    async def list_coalitions(self, space_id: str) -> list[dict]:
        """List all coalitions in a space.

        Args:
            space_id: Space ID

        Returns:
            List of coalitions
        """
        return await self._request("GET", f"/api/v1/spaces/{space_id}/coalitions")

    async def get_coalition(self, coalition_id: str) -> dict:
        """Get coalition details.

        Args:
            coalition_id: Coalition ID

        Returns:
            Coalition details
        """
        return await self._request("GET", f"/api/v1/coalitions/{coalition_id}")

    async def join_coalition(self, coalition_id: str) -> dict:
        """Join a coalition.

        Args:
            coalition_id: Coalition ID

        Returns:
            Updated coalition data
        """
        return await self._request("POST", f"/api/v1/coalitions/{coalition_id}/join")

    async def leave_coalition(self, coalition_id: str) -> dict:
        """Leave a coalition.

        Args:
            coalition_id: Coalition ID

        Returns:
            Updated coalition data
        """
        return await self._request("POST", f"/api/v1/coalitions/{coalition_id}/leave")

    async def update_coalition_stance(self, coalition_id: str, stance: str) -> dict:
        """Update coalition stance.

        Args:
            coalition_id: Coalition ID
            stance: New stance

        Returns:
            Updated coalition data
        """
        return await self._request(
            "PUT", f"/api/v1/coalitions/{coalition_id}/stance", json={"stance": stance}
        )

    async def disband_coalition(self, coalition_id: str) -> dict:
        """Disband a coalition.

        Args:
            coalition_id: Coalition ID

        Returns:
            Disband confirmation
        """
        return await self._request("POST", f"/api/v1/coalitions/{coalition_id}/disband")

    # ==================== Phase 11: Delegation APIs ====================

    async def create_delegation(
        self,
        space_id: str,
        delegate_id: str,
        scope: str,
        expires_at: int | None = None,
    ) -> dict:
        """Delegate authority to another agent in a space.

        Args:
            space_id: Space ID
            delegate_id: Agent ID to delegate to
            scope: Delegation scope (e.g., "vote", "propose", "full")
            expires_at: Optional Unix timestamp for expiry

        Returns:
            Created delegation data
        """
        data: dict = {"delegate_id": delegate_id, "scope": scope}
        if expires_at is not None:
            data["expires_at"] = expires_at
        return await self._request("POST", f"/api/v1/spaces/{space_id}/delegations", json=data)

    async def list_delegations(self, space_id: str) -> list[dict]:
        """List all delegations in a space.

        Args:
            space_id: Space ID

        Returns:
            List of delegations
        """
        return await self._request("GET", f"/api/v1/spaces/{space_id}/delegations")

    async def revoke_delegation(self, delegation_id: str) -> dict:
        """Revoke a delegation.

        Args:
            delegation_id: Delegation ID

        Returns:
            Revocation confirmation
        """
        return await self._request("DELETE", f"/api/v1/delegations/{delegation_id}")

    async def list_agent_delegations(self, agent_id: str) -> list[dict]:
        """List all delegations for an agent.

        Args:
            agent_id: Agent ID

        Returns:
            List of delegations
        """
        return await self._request("GET", f"/api/v1/agents/{agent_id}/delegations")

    # ==================== Phase 12: Recruitment APIs ====================

    async def create_recruitment(
        self,
        space_id: str,
        target_id: str,
        role: str | None = None,
        pitch: str | None = None,
    ) -> dict:
        """Recruit an agent to a space.

        Args:
            space_id: Space ID
            target_id: Target agent ID
            role: Optional role to offer
            pitch: Optional recruitment pitch

        Returns:
            Created recruitment data
        """
        data: dict = {"target_id": target_id}
        if role is not None:
            data["role"] = role
        if pitch is not None:
            data["pitch"] = pitch
        return await self._request("POST", f"/api/v1/spaces/{space_id}/recruit", json=data)

    async def accept_recruitment(self, space_id: str, recruitment_id: str) -> dict:
        """Accept a recruitment invitation.

        Args:
            space_id: Space ID
            recruitment_id: Recruitment ID

        Returns:
            Acceptance confirmation
        """
        return await self._request(
            "POST", f"/api/v1/spaces/{space_id}/recruit/{recruitment_id}/accept"
        )

    async def reject_recruitment(self, space_id: str, recruitment_id: str) -> dict:
        """Reject a recruitment invitation.

        Args:
            space_id: Space ID
            recruitment_id: Recruitment ID

        Returns:
            Rejection confirmation
        """
        return await self._request(
            "POST", f"/api/v1/spaces/{space_id}/recruit/{recruitment_id}/reject"
        )

    async def list_recruitments(self, space_id: str) -> list[dict]:
        """List all recruitments for a space.

        Args:
            space_id: Space ID

        Returns:
            List of recruitments
        """
        return await self._request("GET", f"/api/v1/spaces/{space_id}/recruitments")
