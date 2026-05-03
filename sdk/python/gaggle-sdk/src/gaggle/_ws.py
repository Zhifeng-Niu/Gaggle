"""WebSocket connection manager for real-time Gaggle events."""

import asyncio
import json
import logging
import uuid
from collections import defaultdict
from typing import Any, Awaitable, Callable

import websockets

from gaggle._heartbeat import Heartbeat
from gaggle._offline import OfflineQueue
from gaggle._reconnect import ReconnectPolicy
from gaggle.exceptions import (
    AuthenticationError,
    ConnectionError,
    GaggleError,
    ReconnectFailedError,
    TimeoutError,
)

logger = logging.getLogger("gaggle")

EventHandler = Callable[[dict[str, Any]], Awaitable[None]]


class WSConnectionManager:
    """WebSocket connection manager with auto-reconnect and event dispatch.

    Handles the WebSocket connection lifecycle, message dispatching,
    heartbeat monitoring, and automatic reconnection with exponential backoff.

    Args:
        agent_id: The agent ID for WebSocket authentication.
        api_key: API key for authentication.
        base_url: Base URL of the Gaggle server (http:// or https://).
        heartbeat_interval: Seconds between heartbeat pings. Default: 30.0.
        heartbeat_timeout: Seconds of inactivity before timeout. Default: 90.0.
        reconnect_base_delay: Initial reconnect delay in seconds. Default: 1.0.
        reconnect_max_delay: Maximum reconnect delay in seconds. Default: 30.0.
        reconnect_max_attempts: Max reconnect attempts. None = unlimited.
        offline_queue_path: Path to offline queue database. None = in-memory.

    Example:
        ws = WSConnectionManager(
            agent_id="agent_123",
            api_key="gag_xxx",
            base_url="http://localhost:8080"
        )

        @ws.on("new_message")
        async def handle_message(event):
            print(f"New message: {event}")

        await ws.run()
    """

    def __init__(
        self,
        agent_id: str,
        api_key: str,
        base_url: str = "http://106.15.228.101:8080",
        *,
        heartbeat_interval: float = 30.0,
        heartbeat_timeout: float = 90.0,
        reconnect_base_delay: float = 1.0,
        reconnect_max_delay: float = 30.0,
        reconnect_max_attempts: int | None = None,
        offline_queue_path: str | None = None,
    ):
        self._agent_id = agent_id
        self._api_key = api_key

        # Convert http:// to ws://, https:// to wss://
        ws_url = base_url.replace("http://", "ws://").replace("https://", "wss://")
        self._ws_url = f"{ws_url}/ws/v1/agents/{agent_id}?token={api_key}"

        self._heartbeat_interval = heartbeat_interval
        self._heartbeat_timeout = heartbeat_timeout
        self._reconnect = ReconnectPolicy(
            reconnect_base_delay, reconnect_max_delay, reconnect_max_attempts
        )
        self._offline = OfflineQueue(offline_queue_path)

        self._handlers: dict[str, list[EventHandler]] = defaultdict(list)
        self._pending_requests: dict[str, asyncio.Future[dict[str, Any]]] = {}
        self._ws: websockets.WebSocketClientProtocol | None = None
        self._running = False
        self._last_event_seq: int = 0
        self._heartbeat: Heartbeat | None = None

    def on(self, event_type: str) -> Callable[[EventHandler], EventHandler]:
        """Decorator to register async handler for a WS event type.

        Args:
            event_type: Event type to handle (e.g., "new_message", "new_proposal").
                        Use "*" to handle all events.

        Example:
            @ws.on("new_message")
            async def handle_message(event):
                print(f"Message: {event}")
        """

        def decorator(func: EventHandler) -> EventHandler:
            self._handlers[event_type].append(func)
            return func

        return decorator

    def add_handler(self, event_type: str, handler: EventHandler):
        """Add an event handler programmatically.

        Args:
            event_type: Event type to handle.
            handler: Async function that receives event dict.
        """
        self._handlers[event_type].append(handler)

    def remove_handler(self, event_type: str, handler: EventHandler):
        """Remove a specific event handler.

        Args:
            event_type: Event type the handler is registered for.
            handler: The handler function to remove.
        """
        if event_type in self._handlers:
            self._handlers[event_type] = [
                h for h in self._handlers[event_type] if h != handler
            ]

    # -- Request-Response --

    async def ask(
        self,
        space_id: str,
        content: str,
        *,
        timeout: float = 30.0,
        msg_type: str = "text",
        metadata: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Send a message and wait for a reply in the same space.

        Sends a message with a unique ``correlation_id`` in metadata,
        then awaits a response from another agent in the same space
        that carries the same ``correlation_id``.

        Args:
            space_id: ID of the target space.
            content: Message content.
            timeout: Seconds to wait for a reply. Default: 30.0.
            msg_type: Message type. Default: "text".
            metadata: Optional extra metadata (correlation_id will be added).

        Returns:
            The response message data dict.

        Raises:
            TimeoutError: If no reply arrives within *timeout* seconds.
            GaggleError: If the WebSocket is not connected.
        """
        if not self.is_connected:
            raise GaggleError("WebSocket not connected")

        correlation_id = str(uuid.uuid4())
        loop = asyncio.get_running_loop()
        future: asyncio.Future[dict[str, Any]] = loop.create_future()
        self._pending_requests[correlation_id] = future

        # Merge correlation_id into metadata
        meta = dict(metadata or {})
        meta["correlation_id"] = correlation_id

        try:
            await self.send_message(
                space_id, content, msg_type=msg_type, metadata=meta
            )
            return await asyncio.wait_for(future, timeout=timeout)
        except asyncio.TimeoutError:
            raise TimeoutError(
                f"No response in space {space_id} within {timeout}s"
            )
        finally:
            self._pending_requests.pop(correlation_id, None)

    def _try_resolve_pending(self, data: dict[str, Any]) -> bool:
        """Check if an incoming event resolves a pending ask() request.

        Args:
            data: Parsed event data.

        Returns:
            True if the event resolved a pending request.
        """
        if data.get("type") != "new_message":
            return False

        # Extract the message payload
        payload = data.get("payload", {})
        message = payload.get("message", {})
        sender_id = message.get("sender_id", "")

        # Ignore own messages
        if sender_id == self._agent_id:
            return False

        # Check for correlation_id in message metadata
        msg_metadata = message.get("metadata") or {}
        correlation_id = msg_metadata.get("correlation_id", "")

        if not correlation_id or correlation_id not in self._pending_requests:
            return False

        future = self._pending_requests[correlation_id]
        if not future.done():
            future.set_result(message)

        return True

    async def _safe_call_handler(
        self, event_type: str, handler: EventHandler, data: dict[str, Any]
    ):
        """Call an event handler with error isolation.

        Args:
            event_type: Event type for logging.
            handler: The async handler to call.
            data: Event data dict.
        """
        try:
            await handler(data)
        except Exception as e:
            logger.error(f"Handler error for {event_type}: {e}", exc_info=True)

    # -- WS Commands --

    async def _send_command(self, msg: dict[str, Any]) -> None:
        """Send a raw WS message.

        Args:
            msg: Message dictionary to send as JSON.
        """
        if self._ws:
            await self._ws.send(json.dumps(msg))
            logger.debug(f"WS sent: {msg.get('type')}")
        else:
            logger.warning(f"Cannot send command: WebSocket not connected")

    async def _send_with_request_id(
        self, msg: dict[str, Any], request_id: str | None = None
    ) -> None:
        """Send command with optional request_id for correlation.

        Args:
            msg: Message dictionary to send.
            request_id: Optional request ID for tracking responses.
        """
        if request_id:
            msg["request_id"] = request_id
        await self._send_command(msg)

    async def create_space(
        self,
        name: str,
        invitee_ids: list[str],
        context: dict[str, Any],
        *,
        request_id: str | None = None,
        my_role: str | None = None,
    ):
        """Send create_space command via WebSocket.

        Args:
            name: Name of the space to create.
            invitee_ids: List of agent IDs to invite.
            context: Additional context for the space.
            request_id: Optional request ID.
            my_role: Optional role to assign to creator.
        """
        payload: dict[str, Any] = {
            "name": name,
            "invitee_ids": invitee_ids,
            "context": context,
        }
        if my_role is not None:
            payload["my_role"] = my_role

        await self._send_with_request_id({"type": "create_space", "payload": payload}, request_id)

    async def create_rfp(
        self,
        name: str,
        provider_ids: list[str],
        context: dict[str, Any],
        *,
        request_id: str | None = None,
        allowed_rounds: int | None = None,
        evaluation_criteria: list[str] | None = None,
        deadline: int | None = None,
        share_best_terms: bool | None = None,
    ):
        """Send create_rfp command via WebSocket.

        Args:
            name: Name of the RFP space.
            provider_ids: List of provider agent IDs.
            context: Additional context for the RFP.
            request_id: Optional request ID.
            allowed_rounds: Maximum negotiation rounds.
            evaluation_criteria: Criteria for evaluation.
            deadline: Unix timestamp deadline.
            share_best_terms: Whether to share best terms.
        """
        payload: dict[str, Any] = {
            "name": name,
            "provider_ids": provider_ids,
            "context": context,
        }
        if allowed_rounds is not None:
            payload["allowed_rounds"] = allowed_rounds
        if evaluation_criteria is not None:
            payload["evaluation_criteria"] = evaluation_criteria
        if deadline is not None:
            payload["deadline"] = deadline
        if share_best_terms is not None:
            payload["share_best_terms"] = share_best_terms

        await self._send_with_request_id({"type": "create_rfp", "payload": payload}, request_id)

    async def join_space(self, space_id: str, *, request_id: str | None = None):
        """Send join_space command via WebSocket.

        Args:
            space_id: ID of the space to join.
            request_id: Optional request ID.
        """
        await self._send_with_request_id({"type": "join_space", "space_id": space_id}, request_id)

    async def leave_space(self, space_id: str, *, request_id: str | None = None):
        """Send leave_space command via WebSocket.

        Args:
            space_id: ID of the space to leave.
            request_id: Optional request ID.
        """
        await self._send_with_request_id({"type": "leave_space", "space_id": space_id}, request_id)

    async def close_space(
        self,
        space_id: str,
        conclusion: str = "concluded",
        final_terms: dict[str, Any] | None = None,
        *,
        request_id: str | None = None,
    ):
        """Send close_space command via WebSocket.

        Args:
            space_id: ID of the space to close.
            conclusion: Conclusion reason. Default: "concluded".
            final_terms: Optional final terms summary.
            request_id: Optional request ID.
        """
        payload: dict[str, Any] = {"conclusion": conclusion}
        if final_terms is not None:
            payload["final_terms"] = final_terms

        await self._send_with_request_id(
            {"type": "close_space", "space_id": space_id, "payload": payload}, request_id
        )

    async def send_message(
        self,
        space_id: str,
        content: str,
        *,
        msg_type: str = "text",
        metadata: dict[str, Any] | None = None,
        proposal: dict[str, Any] | None = None,
        request_id: str | None = None,
    ):
        """Send a message to a space via WebSocket.

        Args:
            space_id: ID of the target space.
            content: Message content.
            msg_type: Message type (e.g., "text", "structured"). Default: "text".
            metadata: Optional metadata dictionary.
            proposal: Optional inline proposal dict with keys:
                proposal_type (str), dimensions (dict), parent_proposal_id (str).
                When provided, creates a Message + Proposal in one operation.
            request_id: Optional request ID.
        """
        payload: dict[str, Any] = {"msg_type": msg_type, "content": content}
        if metadata is not None:
            payload["metadata"] = metadata
        if proposal is not None:
            payload["proposal"] = proposal

        await self._send_with_request_id(
            {"type": "send_message", "space_id": space_id, "payload": payload}, request_id
        )

    async def submit_proposal(
        self,
        space_id: str,
        proposal_type: str,
        dimensions: dict[str, Any],
        *,
        parent_proposal_id: str | None = None,
        request_id: str | None = None,
    ):
        """Submit a proposal via WebSocket.

        Args:
            space_id: ID of the target space.
            proposal_type: Type of proposal.
            dimensions: Proposal dimensions.
            parent_proposal_id: Optional parent proposal ID for counter-proposals.
            request_id: Optional request ID.
        """
        payload: dict[str, Any] = {"proposal_type": proposal_type, "dimensions": dimensions}
        if parent_proposal_id is not None:
            payload["parent_proposal_id"] = parent_proposal_id

        await self._send_with_request_id(
            {"type": "submit_proposal", "space_id": space_id, "payload": payload}, request_id
        )

    async def respond_to_proposal(
        self,
        space_id: str,
        proposal_id: str,
        action: str,
        *,
        counter_dimensions: dict[str, Any] | None = None,
        request_id: str | None = None,
    ):
        """Respond to a proposal via WebSocket.

        Args:
            space_id: ID of the target space.
            proposal_id: ID of the proposal to respond to.
            action: Response action ("accept", "reject", "counter").
            counter_dimensions: Optional counter-proposal dimensions.
            request_id: Optional request ID.
        """
        payload: dict[str, Any] = {"proposal_id": proposal_id, "action": action}
        if counter_dimensions is not None:
            payload["counter_dimensions"] = counter_dimensions

        await self._send_with_request_id(
            {"type": "respond_to_proposal", "space_id": space_id, "payload": payload}, request_id
        )

    async def share_best_terms(self, space_id: str, best_dimensions: dict[str, Any], *, request_id: str | None = None):
        """Share best terms via WebSocket (for RFP spaces).

        Args:
            space_id: ID of the target space.
            best_dimensions: Best dimensions found.
            request_id: Optional request ID.
        """
        await self._send_with_request_id(
            {"type": "share_best_terms", "space_id": space_id, "payload": {"best_dimensions": best_dimensions}},
            request_id,
        )

    async def list_spaces(self):
        """Request list of spaces via WebSocket."""
        await self._send_command({"type": "list_spaces"})

    async def check_online(self, agent_ids: list[str]):
        """Check online status of agents via WebSocket.

        Args:
            agent_ids: List of agent IDs to check.
        """
        await self._send_command({"type": "check_online", "payload": {"agent_ids": agent_ids}})

    # -- Need Broadcast Commands --

    async def publish_need(
        self,
        title: str,
        description: str,
        category: str,
        *,
        required_skills: list[str] | None = None,
        budget_min: float | None = None,
        budget_max: float | None = None,
        deadline: int | None = None,
        request_id: str | None = None,
    ):
        """Publish a need broadcast via WebSocket.

        Args:
            title: Need title.
            description: Need description.
            category: Need category.
            required_skills: Optional list of required skills.
            budget_min: Optional minimum budget.
            budget_max: Optional maximum budget.
            deadline: Optional Unix timestamp deadline.
            request_id: Optional request ID.
        """
        payload: dict[str, Any] = {
            "title": title,
            "description": description,
            "category": category,
        }
        if required_skills is not None:
            payload["required_skills"] = required_skills
        if budget_min is not None:
            payload["budget_min"] = budget_min
        if budget_max is not None:
            payload["budget_max"] = budget_max
        if deadline is not None:
            payload["deadline"] = deadline

        await self._send_with_request_id(
            {"type": "publish_need", "payload": payload}, request_id
        )

    async def list_needs(
        self,
        category: str | None = None,
        *,
        skills: str | None = None,
        query: str | None = None,
        page: int = 1,
        page_size: int = 20,
        request_id: str | None = None,
    ):
        """Request list of needs via WebSocket.

        Args:
            category: Optional category filter.
            skills: Optional comma-separated skills filter.
            query: Optional text search query.
            page: Page number (1-based).
            page_size: Items per page.
            request_id: Optional request ID.
        """
        payload: dict[str, Any] = {
            "page": page,
            "page_size": page_size,
        }
        if category is not None:
            payload["category"] = category
        if skills is not None:
            payload["skills"] = skills
        if query is not None:
            payload["query"] = query

        await self._send_with_request_id(
            {"type": "list_needs", "payload": payload}, request_id
        )

    async def cancel_need(self, need_id: str, *, request_id: str | None = None):
        """Cancel a need via WebSocket.

        Args:
            need_id: ID of the need to cancel.
            request_id: Optional request ID.
        """
        await self._send_with_request_id(
            {"type": "cancel_need", "payload": {"need_id": need_id}}, request_id
        )

    async def need_to_rfp(
        self,
        need_id: str,
        provider_ids: list[str],
        *,
        allowed_rounds: int | None = None,
        deadline: int | None = None,
        share_best_terms: bool | None = None,
        request_id: str | None = None,
    ):
        """Create an RFP space from an existing need via WebSocket.

        Args:
            need_id: Need ID to create RFP from.
            provider_ids: List of provider agent IDs to invite.
            allowed_rounds: Maximum negotiation rounds.
            deadline: Unix timestamp deadline.
            share_best_terms: Whether to anonymously share best terms.
            request_id: Optional request ID.
        """
        payload: dict[str, Any] = {
            "need_id": need_id,
            "provider_ids": provider_ids,
        }
        if allowed_rounds is not None:
            payload["allowed_rounds"] = allowed_rounds
        if deadline is not None:
            payload["deadline"] = deadline
        if share_best_terms is not None:
            payload["share_best_terms"] = share_best_terms

        await self._send_with_request_id(
            {"type": "need_to_rfp", "payload": payload}, request_id
        )

    async def _send_resume(self):
        """Send resume command to reconnect after disconnect."""
        if self._last_event_seq > 0:
            await self._send_command({"type": "resume", "last_event_seq": self._last_event_seq})
            logger.info(f"Sent resume with last_event_seq={self._last_event_seq}")

    # -- Connection lifecycle --

    @property
    def is_connected(self) -> bool:
        """Check if WebSocket is currently connected."""
        if self._ws is None:
            return False
        # websockets < 13 uses .open; >= 13 uses .state
        if hasattr(self._ws, "open"):
            return self._ws.open
        try:
            from websockets.protocol import State
            return self._ws.state is State.OPEN
        except Exception:
            return False

    @property
    def last_event_seq(self) -> int:
        """Get the last received event sequence number."""
        return self._last_event_seq

    async def connect(self):
        """Establish WebSocket connection."""
        try:
            self._ws = await websockets.connect(
                self._ws_url,
                ping_interval=None,  # We handle heartbeat ourselves
                close_timeout=5,
            )
            logger.info(f"WebSocket connected: {self._agent_id}")
        except websockets.exceptions.InvalidStatusCode as e:
            if e.status_code == 401:
                raise AuthenticationError("Invalid API key") from e
            raise ConnectionError(f"WebSocket connection failed: {e}") from e
        except Exception as e:
            raise ConnectionError(f"WebSocket connection failed: {e}") from e

    async def disconnect(self):
        """Disconnect the WebSocket and stop heartbeat."""
        self._running = False

        # Cancel all pending ask() futures
        for fut in self._pending_requests.values():
            if not fut.done():
                fut.cancel()
        self._pending_requests.clear()

        if self._heartbeat:
            await self._heartbeat.stop()
        if self._ws:
            await self._ws.close()
            self._ws = None
            logger.info("WebSocket disconnected")

    async def _dispatch(self, raw: str):
        """Parse and dispatch a WS message to registered handlers.

        Checks pending request-response futures first, then dispatches
        to registered event handlers as non-blocking tasks.

        Args:
            raw: Raw JSON string from WebSocket.
        """
        try:
            data = json.loads(raw)
            event_type = data.get("type", "")

            # Track event sequence for resume
            if "event_seq" in data:
                self._last_event_seq = data["event_seq"]

            # Check if this resolves a pending ask() request
            if self._try_resolve_pending(data):
                logger.debug(f"Event '{event_type}' resolved a pending request")
                return

            # Dispatch to specific handlers and wildcard handlers as tasks
            handlers = self._handlers.get(event_type, []) + self._handlers.get("*", [])
            if handlers:
                logger.debug(f"Dispatching event '{event_type}' to {len(handlers)} handler(s)")
                for handler in handlers:
                    asyncio.create_task(self._safe_call_handler(event_type, handler, data))
            else:
                logger.debug(f"No handlers for event type: {event_type}")

        except json.JSONDecodeError as e:
            logger.warning(f"Invalid JSON from server: {e}")

    async def _listen_loop(self):
        """Main message receive loop."""
        if self._heartbeat:
            self._heartbeat.record_activity()

        async for message in self._ws:
            if isinstance(message, bytes):
                message = message.decode()
            await self._dispatch(message)
            if self._heartbeat:
                self._heartbeat.record_activity()

    async def run(self):
        """Main run loop with auto-reconnect.

        This method blocks until:
        - Explicitly stopped via disconnect()
        - Authentication error occurs (no auto-reconnect)
        - Max reconnect attempts reached
        """
        self._running = True
        await self._offline.open()
        attempt = 0

        while self._running:
            try:
                await self.connect()
                attempt = 0  # Reset on successful connect

                # Setup heartbeat
                self._heartbeat = Heartbeat(
                    send_callback=self._send_command,
                    interval=self._heartbeat_interval,
                    timeout=self._heartbeat_timeout,
                )
                await self._heartbeat.start()

                # Send resume for missed events
                await self._send_resume()

                # Replay offline queue
                offline_events = await self._offline.pop_all()
                for event_type, payload in offline_events:
                    await self._dispatch(json.dumps({"type": event_type, **payload}))

                # Main listen loop (blocks until disconnect)
                await self._listen_loop()

            except TimeoutError:
                if not self._running:
                    break
                logger.warning("WebSocket timeout - reconnecting")
            except (ConnectionError, websockets.exceptions.ConnectionClosed):
                if not self._running:
                    break
                logger.warning("WebSocket disconnected - reconnecting")
            except AuthenticationError:
                logger.error("Authentication failed - not reconnecting")
                raise
            except Exception as e:
                logger.error(f"Unexpected error: {e}", exc_info=True)
                if not self._running:
                    break

            # Cleanup before reconnect
            if self._heartbeat:
                await self._heartbeat.stop()
                self._heartbeat = None

            # Cancel pending ask() futures (connection lost)
            for fut in self._pending_requests.values():
                if not fut.done():
                    fut.cancel()
            self._pending_requests.clear()

            if self._ws:
                try:
                    await self._ws.close()
                except Exception:
                    pass
                self._ws = None

            if not self._running:
                break

            # Reconnect with backoff
            if not self._reconnect.should_retry(attempt):
                raise ReconnectFailedError(f"Failed after {attempt} reconnect attempts")

            delay = self._reconnect.next_delay(attempt)
            logger.warning(f"Reconnecting in {delay:.1f}s (attempt {attempt + 1})")
            await asyncio.sleep(delay)
            attempt += 1

        await self._offline.close()
        logger.info("WebSocket connection manager stopped")
