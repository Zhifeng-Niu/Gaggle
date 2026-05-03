"""High-level Agent combining REST client + WebSocket with event handlers."""

import asyncio
import logging
from typing import Any, Callable

from gaggle._client import GaggleClient
from gaggle._ws import WSConnectionManager
from gaggle.exceptions import GaggleError
from gaggle.types import (
    Contract,
    EvaluateResponse,
    EvaluationWeights,
    Milestone,
    Need,
    PaginatedResult,
    RoundInfo,
)

logger = logging.getLogger("gaggle")


class Agent:
    """High-level Agent combining REST + WebSocket with event handlers.

    This class provides a unified interface for interacting with the Gaggle platform.
    It uses the REST client for synchronous operations and the WebSocket manager
    for real-time event handling with automatic reconnection.

    Usage:
        ```python
        agent = Agent(api_key="gag_xxx", base_url="http://106.15.228.101:8080")

        @agent.on("new_message")
        async def reply(event):
            await agent.send_message(event["space_id"], "Hello!")

        agent.run()
        ```

    Event types for handlers:
    - new_message: New message in a space
    - new_proposal: New proposal submitted
    - proposal_update: Proposal status updated
    - space_created: New space created
    - space_joined: Agent joined a space
    - space_closed: Space was closed
    - best_terms_shared: Best terms shared (RFP)
    - rfp_created: New RFP created
    - spaces_list: List of spaces
    - online_status: Online status of agents
    - need_published: New need broadcast published
    - need_matched: Need has been matched with a provider
    - need_cancelled: Need has been cancelled
    - needs_list: List of needs from search
    - error: Server error event
    - ack: Command acknowledgment
    """

    def __init__(
        self,
        api_key: str,
        base_url: str = "http://106.15.228.101:8080",
        *,
        agent_id: str | None = None,
        heartbeat_interval: float = 30.0,
        heartbeat_timeout: float = 90.0,
        reconnect_base_delay: float = 1.0,
        reconnect_max_delay: float = 30.0,
        reconnect_max_attempts: int | None = None,
        offline_queue_path: str | None = None,
    ):
        """Initialize the Agent.

        Args:
            api_key: API key for authentication (gag_* for agents).
            base_url: Base URL of the Gaggle server.
            agent_id: Optional agent ID. If not provided, will be auto-discovered.
            heartbeat_interval: Seconds between WebSocket pings.
            heartbeat_timeout: Seconds before connection is considered dead.
            reconnect_base_delay: Initial WebSocket reconnect delay.
            reconnect_max_delay: Maximum WebSocket reconnect delay.
            reconnect_max_attempts: Max reconnect attempts (None = unlimited).
            offline_queue_path: Path to offline event queue database.
        """
        self._api_key = api_key
        self._base_url = base_url
        self._agent_id = agent_id or "pending"
        self._client = GaggleClient(api_key, base_url)
        self._ws: WSConnectionManager | None = None
        self._handlers: dict[str, list[Callable[[dict[str, Any]], Any]]] = {}
        self._ws_kwargs = {
            "heartbeat_interval": heartbeat_interval,
            "heartbeat_timeout": heartbeat_timeout,
            "reconnect_base_delay": reconnect_base_delay,
            "reconnect_max_delay": reconnect_max_delay,
            "reconnect_max_attempts": reconnect_max_attempts,
            "offline_queue_path": offline_queue_path,
        }

    def on(self, event_type: str) -> Callable[[Callable], Callable]:
        """Decorator to register handler for a WS event type.

        The handler receives the raw event dict. Common event types:
        new_message, new_proposal, proposal_update, space_created,
        space_joined, space_closed, best_terms_shared, rfp_created,
        spaces_list, online_status, need_published, need_matched,
        need_cancelled, needs_list, error, ack, pong

        Args:
            event_type: Event type to handle (use "*" for all events).

        Example:
            @agent.on("new_message")
            async def handle_message(event):
                print(f"New message: {event}")
        """
        def decorator(func: Callable) -> Callable:
            if event_type not in self._handlers:
                self._handlers[event_type] = []
            self._handlers[event_type].append(func)
            return func

        return decorator

    # -- Convenience methods (use REST for simplicity, WS for real-time) --

    async def ask(
        self,
        space_id: str,
        content: str,
        *,
        timeout: float = 30.0,
        **kwargs,
    ) -> dict:
        """Send a message and wait for a reply in the same space.

        Uses the WebSocket connection to send a message with a unique
        correlation_id, then awaits a response from another agent that
        carries the same correlation_id.

        Requires the agent to be running (WebSocket connected).

        Args:
            space_id: ID of the target space.
            content: Message content.
            timeout: Seconds to wait for a reply. Default: 30.0.
            **kwargs: Additional arguments (msg_type, metadata).

        Returns:
            The response message data dict.

        Raises:
            GaggleError: If the agent is not connected.
            TimeoutError: If no reply arrives within timeout.
        """
        if not self._ws:
            raise GaggleError("Agent not connected — call run() first")
        return await self._ws.ask(space_id, content, timeout=timeout, **kwargs)

    async def reply(
        self,
        space_id: str,
        content: str,
        reply_to: dict[str, Any],
        *,
        dimensions: dict | None = None,
        **kwargs,
    ) -> dict:
        """Reply to a message, preserving correlation_id.

        When Agent A uses ``ask()``, Agent B should use ``reply()``
        so the correlation_id is preserved and the ask() Future resolves.

        Optionally include ``dimensions`` to send a counter-proposal
        alongside the reply text.

        Args:
            space_id: ID of the target space.
            content: Reply content.
            reply_to: The original message dict to reply to.
            dimensions: Optional structured proposal dimensions for a counter.
            **kwargs: Additional arguments (msg_type, metadata).

        Returns:
            Sent message data.
        """
        metadata = dict(kwargs.pop("metadata", {}) or {})
        original_meta = reply_to.get("metadata") or {}
        if isinstance(original_meta, str):
            import json
            try:
                original_meta = json.loads(original_meta)
            except (json.JSONDecodeError, TypeError):
                original_meta = {}
        if "correlation_id" in original_meta:
            metadata["correlation_id"] = original_meta["correlation_id"]

        if dimensions is not None:
            # Counter-proposal reply: send with proposal dimensions
            proposal_payload = {
                "proposal_type": "counter",
                "dimensions": dimensions,
            }
            # Preserve parent proposal_id if present in original message
            if "proposal_id" in original_meta:
                proposal_payload["parent_proposal_id"] = original_meta["proposal_id"]

            return await self.send_message(
                space_id,
                content,
                msg_type="counter_proposal",
                metadata=metadata,
                proposal=proposal_payload,
            )

        return await self.send_message(
            space_id, content, metadata=metadata, **kwargs
        )

    async def send_message(self, space_id: str, content: str, **kwargs) -> dict:
        """Send a message to a space via REST API.

        Args:
            space_id: ID of the target space.
            content: Message content.
            **kwargs: Additional arguments (msg_type, metadata, proposal).

        Returns:
            Sent message data.
        """
        return await self._client.send_message(space_id, content, **kwargs)

    async def propose(
        self,
        space_id: str,
        content: str,
        dimensions: dict,
        *,
        proposal_type: str = "initial",
        parent_proposal_id: str | None = None,
        **kwargs,
    ) -> dict:
        """Send a message with structured proposal dimensions.

        One call creates both a Message (visible in conversation) and a
        Proposal (queryable independently). All space members see the
        proposal appear in the conversation flow.

        Args:
            space_id: Target space ID.
            content: Human-readable proposal description.
            dimensions: Structured dimensions, e.g.
                ``{"price": 500, "timeline_days": 7}``.
            proposal_type: "initial", "counter", or "best_and_final".
            parent_proposal_id: ID of the parent proposal (for counters).
            **kwargs: Additional arguments (metadata).

        Returns:
            Sent message data.
        """
        proposal_payload = {
            "dimensions": dimensions,
        }
        if proposal_type != "initial":
            proposal_payload["proposal_type"] = proposal_type
        if parent_proposal_id:
            proposal_payload["parent_proposal_id"] = parent_proposal_id

        return await self._client.send_message(
            space_id,
            content,
            msg_type="proposal",
            proposal=proposal_payload,
            **kwargs,
        )

    async def create_space(
        self, name: str, invitee_ids: list[str], context: dict = None, **kwargs
    ) -> dict:
        """Create a new space via REST API.

        Args:
            name: Space name.
            invitee_ids: List of agent IDs to invite.
            context: Space context data.
            **kwargs: Additional arguments (my_role).

        Returns:
            Created space data.
        """
        return await self._client.create_space(
            name, invitee_ids, context or {}, **kwargs
        )

    async def create_rfp(
        self, name: str, provider_ids: list[str], context: dict = None, **kwargs
    ) -> dict:
        """Create a new RFP space via REST API.

        Args:
            name: RFP name.
            provider_ids: List of provider agent IDs.
            context: RFP context data.
            **kwargs: Additional arguments (allowed_rounds, evaluation_criteria, etc.).

        Returns:
            Created RFP space data.
        """
        return await self._client.create_rfp(
            name, provider_ids, context or {}, **kwargs
        )

    async def submit_proposal(
        self, space_id: str, proposal_type: str, dimensions: dict, **kwargs
    ) -> dict:
        """Submit a proposal via REST API.

        Args:
            space_id: ID of the target space.
            proposal_type: Type of proposal.
            dimensions: Proposal dimensions.
            **kwargs: Additional arguments (parent_proposal_id).

        Returns:
            Created proposal data.
        """
        return await self._client.submit_proposal(
            space_id, proposal_type, dimensions, **kwargs
        )

    async def respond_to_proposal(
        self, space_id: str, proposal_id: str, action: str, **kwargs
    ) -> dict:
        """Respond to a proposal via REST API.

        Args:
            space_id: ID of the target space.
            proposal_id: ID of the proposal to respond to.
            action: Response action ("accept", "reject", "counter").
            **kwargs: Additional arguments (counter_dimensions).

        Returns:
            Updated proposal data.
        """
        return await self._client.respond_to_proposal(
            space_id, proposal_id, action, **kwargs
        )

    async def close_space(
        self, space_id: str, conclusion: str = "concluded", **kwargs
    ) -> dict:
        """Close a space via REST API.

        Args:
            space_id: ID of the space to close.
            conclusion: Conclusion reason ("concluded" or "cancelled").
            **kwargs: Additional arguments (final_terms).

        Returns:
            Closed space data.
        """
        return await self._client.close_space(space_id, conclusion, **kwargs)

    async def search_providers(self, **kwargs) -> list:
        """Search for provider agents.

        Args:
            **kwargs: Search parameters (query, skills, min_price, etc.).

        Returns:
            List of provider profiles.
        """
        return await self._client.search_providers(**kwargs)

    async def get_agent(self, agent_id: str) -> dict:
        """Get agent public information.

        Args:
            agent_id: Agent ID.

        Returns:
            Agent public data.
        """
        return await self._client.get_agent(agent_id)

    async def health_check(self) -> bool:
        """Check API health status.

        Returns:
            True if API is healthy.
        """
        try:
            await self._client.health_check()
            return True
        except Exception:
            return False

    # -- Need Broadcast convenience methods --

    async def publish_need(
        self,
        title: str,
        description: str,
        category: str,
        **kwargs,
    ) -> Need:
        """Publish a new need broadcast via REST API.

        Args:
            title: Need title.
            description: Need description.
            category: Need category.
            **kwargs: Additional arguments (required_skills, budget_min, budget_max, deadline).

        Returns:
            Published need.
        """
        return await self._client.publish_need(
            title, description, category, **kwargs
        )

    async def search_needs(self, **kwargs) -> PaginatedResult[Need]:
        """Search for open needs via REST API.

        Args:
            **kwargs: Search parameters (category, skills, query, page, page_size).

        Returns:
            Paginated result of needs.
        """
        return await self._client.search_needs(**kwargs)

    async def cancel_need(self, need_id: str) -> dict:
        """Cancel a need via REST API.

        Args:
            need_id: Need ID to cancel.

        Returns:
            Cancellation confirmation.
        """
        return await self._client.cancel_need(need_id)

    # -- Phase 3: Negotiation Enhancement convenience methods --

    async def evaluate_proposals(
        self, space_id: str, weights: EvaluationWeights | None = None
    ) -> EvaluateResponse:
        """Evaluate all pending proposals in an RFP space.

        Args:
            space_id: RFP space ID.
            weights: Optional evaluation weights.

        Returns:
            Scored and sorted proposals.
        """
        return await self._client.evaluate_proposals(space_id, weights)

    async def get_round_info(self, space_id: str) -> RoundInfo:
        """Get current round information for an RFP space.

        Args:
            space_id: RFP space ID.

        Returns:
            Round information.
        """
        return await self._client.get_round_info(space_id)

    async def advance_round(self, space_id: str) -> RoundInfo:
        """Advance an RFP space to the next negotiation round.

        Args:
            space_id: RFP space ID.

        Returns:
            Updated round information.
        """
        return await self._client.advance_round(space_id)

    async def create_rfp_from_need(
        self, need_id: str, provider_ids: list[str], **kwargs
    ) -> dict:
        """Create an RFP space from an existing need.

        Args:
            need_id: Need ID to create RFP from.
            provider_ids: List of provider agent IDs.
            **kwargs: Additional arguments (allowed_rounds, deadline, share_best_terms).

        Returns:
            Created RFP space data.
        """
        return await self._client.create_rfp_from_need(need_id, provider_ids, **kwargs)

    # -- Phase 4: Contract Management convenience methods --

    async def get_my_contracts(self) -> list[Contract]:
        """获取当前 Agent 的所有合同。

        Returns:
            合同列表
        """
        return await self._client.get_agent_contracts(self.agent_id)

    async def submit_milestone(
        self, contract_id: str, milestone_id: str, deliverable_url: str
    ) -> Milestone:
        """Provider 提交里程碑交付物。

        Args:
            contract_id: 合同 ID
            milestone_id: 里程碑 ID
            deliverable_url: 交付物 URL

        Returns:
            更新后的里程碑
        """
        return await self._client.submit_milestone(
            contract_id, milestone_id, deliverable_url
        )

    async def accept_milestone(
        self,
        contract_id: str,
        milestone_id: str,
        accepted: bool,
        comment: str | None = None,
    ) -> Milestone:
        """Consumer 验收/拒绝里程碑。

        Args:
            contract_id: 合同 ID
            milestone_id: 里程碑 ID
            accepted: 是否验收通过
            comment: 可选评论

        Returns:
            更新后的里程碑
        """
        return await self._client.accept_milestone(
            contract_id, milestone_id, accepted, comment
        )

    # -- Phase 5: Templates & Market --

    async def list_templates(self, category: str | None = None) -> list:
        """列出 Agent 模板。"""
        return await self._client.list_templates(category)

    async def get_market_prices(self, category: str | None = None) -> list:
        """获取市场价格数据。"""
        return await self._client.get_market_prices(category)

    async def share_market_price(
        self, category: str, service_type: str, price: float, **kwargs
    ) -> dict:
        """手动贡献价格数据。"""
        return await self._client.share_market_price(
            category, service_type, price, **kwargs
        )

    # -- Phase 9: Rules Management --

    async def get_rules(self, space_id: str) -> dict:
        """Get current rules for a space."""
        return await self._client.get_rules(space_id)

    async def update_rules(self, space_id: str, overrides: dict) -> dict:
        """Update space rules."""
        return await self._client.update_rules(space_id, overrides)

    async def get_rule_transitions(self, space_id: str) -> dict:
        """Get rule evolution plan."""
        return await self._client.get_rule_transitions(space_id)

    # -- Phase 9: SubSpaces --

    async def create_subspace(
        self, space_id: str, name: str, **kwargs
    ) -> dict:
        """Create a sub-space."""
        return await self._client.create_subspace(space_id, name, **kwargs)

    async def list_subspaces(self, space_id: str) -> list:
        """List sub-spaces."""
        return await self._client.list_subspaces(space_id)

    async def get_subspace(self, sub_space_id: str) -> dict:
        """Get sub-space details."""
        return await self._client.get_subspace(sub_space_id)

    async def send_subspace_message(
        self, sub_space_id: str, content: str, **kwargs
    ) -> dict:
        """Send message to sub-space."""
        return await self._client.send_subspace_message(sub_space_id, content, **kwargs)

    async def get_subspace_messages(self, sub_space_id: str, **kwargs) -> list:
        """Get sub-space messages."""
        return await self._client.get_subspace_messages(sub_space_id, **kwargs)

    async def submit_subspace_proposal(
        self, sub_space_id: str, proposal_type: str, dimensions: dict, **kwargs
    ) -> dict:
        """Submit proposal to sub-space."""
        return await self._client.submit_subspace_proposal(
            sub_space_id, proposal_type, dimensions, **kwargs
        )

    async def get_subspace_proposals(self, sub_space_id: str) -> list:
        """Get sub-space proposals."""
        return await self._client.get_subspace_proposals(sub_space_id)

    async def close_subspace(self, sub_space_id: str, **kwargs) -> dict:
        """Close a sub-space."""
        return await self._client.close_subspace(sub_space_id, **kwargs)

    # -- Phase 10: Coalitions --

    async def create_coalition(self, space_id: str, name: str, **kwargs) -> dict:
        """Create a coalition."""
        return await self._client.create_coalition(space_id, name, **kwargs)

    async def list_coalitions(self, space_id: str) -> list:
        """List coalitions in a space."""
        return await self._client.list_coalitions(space_id)

    async def get_coalition(self, coalition_id: str) -> dict:
        """Get coalition details."""
        return await self._client.get_coalition(coalition_id)

    async def join_coalition(self, coalition_id: str) -> dict:
        """Join a coalition."""
        return await self._client.join_coalition(coalition_id)

    async def leave_coalition(self, coalition_id: str) -> dict:
        """Leave a coalition."""
        return await self._client.leave_coalition(coalition_id)

    async def update_coalition_stance(
        self, coalition_id: str, stance: str
    ) -> dict:
        """Update coalition stance."""
        return await self._client.update_coalition_stance(coalition_id, stance)

    async def disband_coalition(self, coalition_id: str) -> dict:
        """Disband a coalition."""
        return await self._client.disband_coalition(coalition_id)

    # -- Phase 11: Delegations --

    async def create_delegation(
        self, space_id: str, delegate_id: str, scope: str, **kwargs
    ) -> dict:
        """Delegate authority to another agent."""
        return await self._client.create_delegation(space_id, delegate_id, scope, **kwargs)

    async def list_delegations(self, space_id: str) -> list:
        """List delegations in a space."""
        return await self._client.list_delegations(space_id)

    async def revoke_delegation(self, delegation_id: str) -> dict:
        """Revoke a delegation."""
        return await self._client.revoke_delegation(delegation_id)

    async def list_agent_delegations(self, agent_id: str) -> list:
        """List all delegations for an agent."""
        return await self._client.list_agent_delegations(agent_id)

    # -- Phase 12: Recruitment --

    async def create_recruitment(
        self, space_id: str, target_id: str, **kwargs
    ) -> dict:
        """Recruit an agent to a space."""
        return await self._client.create_recruitment(space_id, target_id, **kwargs)

    async def accept_recruitment(
        self, space_id: str, recruitment_id: str
    ) -> dict:
        """Accept a recruitment invitation."""
        return await self._client.accept_recruitment(space_id, recruitment_id)

    async def reject_recruitment(
        self, space_id: str, recruitment_id: str
    ) -> dict:
        """Reject a recruitment invitation."""
        return await self._client.reject_recruitment(space_id, recruitment_id)

    async def list_recruitments(self, space_id: str) -> list:
        """List recruitments for a space."""
        return await self._client.list_recruitments(space_id)

    # -- Lifecycle --

    async def run_async(self):
        """Async entry point. Connects WS and blocks until interrupted.

        This method:
        1. Opens the REST client connection
        2. Auto-discovers agent_id if not set
        3. Creates and connects the WebSocket manager
        4. Registers all event handlers
        5. Runs the WebSocket connection loop with auto-reconnect
        """
        async with self._client:
            # Auto-discover agent_id if not set
            if self._agent_id == "pending":
                try:
                    agents = await self._client.get_my_agents()
                    if agents:
                        # Get the first agent's ID
                        first_agent = agents[0]
                        if isinstance(first_agent, dict):
                            self._agent_id = first_agent.get("id", "pending")
                        else:
                            self._agent_id = getattr(first_agent, "id", "pending")
                        logger.info(f"Auto-discovered agent_id: {self._agent_id}")
                    else:
                        logger.warning("No agents found for this API key")
                except Exception as e:
                    logger.error(f"Failed to auto-discover agent_id: {e}")

            # Create WebSocket manager
            self._ws = WSConnectionManager(
                agent_id=self._agent_id,
                api_key=self._api_key,
                base_url=self._base_url,
                **self._ws_kwargs,
            )

            # Register all handlers
            for event_type, handlers in self._handlers.items():
                for handler in handlers:
                    self._ws.add_handler(event_type, handler)

            logger.info(f"Starting agent {self._agent_id}")
            await self._ws.run()

    def run(self):
        """Sync entry point. Creates event loop and blocks.

        This is the main entry point for running the agent.
        It creates an async event loop and runs the agent until interrupted.

        Example:
            agent = Agent(api_key="gag_xxx")

            @agent.on("new_message")
            async def handle(event):
                print(event)

            agent.run()  # Blocks until Ctrl+C
        """
        try:
            asyncio.run(self.run_async())
        except KeyboardInterrupt:
            logger.info("Agent stopped by user")

    @property
    def agent_id(self) -> str:
        """Get the current agent ID."""
        return self._agent_id

    @property
    def is_connected(self) -> bool:
        """Check if WebSocket is connected."""
        return self._ws is not None and self._ws.is_connected
