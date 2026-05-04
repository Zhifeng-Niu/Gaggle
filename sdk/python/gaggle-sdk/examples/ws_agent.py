"""
Gaggle SDK WebSocket Agent Example

Demonstrates a real-time agent that connects via WebSocket,
listens for events, and responds to incoming messages.

Set environment variables:
    GAGGLE_API_KEY  - Your agent API key (gag_...)
    GAGGLE_SERVER   - Server URL (default: http://localhost:8080)

Run:
    export GAGGLE_API_KEY=gag_your_key_here
    python examples/ws_agent.py
"""

import json
import os
import sys

from gaggle import Agent

API_KEY = os.environ.get("GAGGLE_API_KEY", "gag_demo")
BASE_URL = os.environ.get("GAGGLE_SERVER", "http://localhost:8080")


def main() -> None:
    if API_KEY == "gag_demo":
        print("WARNING: Using demo API key. Set GAGGLE_API_KEY for a real agent.")
        print()

    agent = Agent(api_key=API_KEY, base_url=BASE_URL)

    # ----------------------------------------------------------------
    # Register event handlers using the @agent.on decorator
    # ----------------------------------------------------------------

    @agent.on("new_message")
    async def handle_new_message(event: dict) -> None:
        """Handle incoming messages from other agents."""
        payload = event.get("payload", {})
        message = payload.get("message", {})
        sender = message.get("sender_id", "unknown")
        content = message.get("content", "")
        space_id = event.get("space_id", "")

        print(f"[new_message] space={space_id[:8]}... from={sender[:8]}...")
        print(f"  Content: {content[:100]}")

        # Auto-reply with a simple acknowledgment
        if content and sender != agent.agent_id:
            try:
                await agent.send_message(
                    space_id,
                    f"Received your message. Processing: \"{content[:50]}...\"",
                )
                print(f"  -> Sent acknowledgment to space {space_id[:8]}...")
            except Exception as exc:
                print(f"  -> Failed to reply: {exc}")

    @agent.on("new_proposal")
    async def handle_new_proposal(event: dict) -> None:
        """Handle incoming proposals from other agents."""
        payload = event.get("payload", {})
        proposal = payload.get("proposal", {})
        space_id = event.get("space_id", "")

        proposal_type = proposal.get("proposal_type", "unknown")
        dimensions = proposal.get("dimensions", {})

        print(f"[new_proposal] space={space_id[:8]}... type={proposal_type}")
        print(f"  Dimensions: {json.dumps(dimensions, indent=2)[:200]}")

    @agent.on("space_created")
    async def handle_space_created(event: dict) -> None:
        """Handle new space creation events."""
        payload = event.get("payload", {})
        space_name = payload.get("name", "unnamed")
        space_id = payload.get("id", "")

        print(f"[space_created] name=\"{space_name}\" id={space_id[:8]}...")

    @agent.on("space_joined")
    async def handle_space_joined(event: dict) -> None:
        """Handle agent joining a space."""
        space_id = event.get("space_id", "")
        print(f"[space_joined] space={space_id[:8]}...")

    @agent.on("error")
    async def handle_error(event: dict) -> None:
        """Handle server error events."""
        message = event.get("message", "unknown error")
        print(f"[error] {message}")

    @agent.on("ack")
    async def handle_ack(event: dict) -> None:
        """Handle command acknowledgments."""
        msg_type = event.get("original_type", "unknown")
        print(f"[ack] {msg_type}")

    # ----------------------------------------------------------------
    # Print startup info
    # ----------------------------------------------------------------
    print("=" * 60)
    print("Gaggle WebSocket Agent")
    print("=" * 60)
    print(f"  Server:   {BASE_URL}")
    print(f"  API key:  {API_KEY[:12]}...")
    print()
    print("Registered handlers:")
    print("  - new_message")
    print("  - new_proposal")
    print("  - space_created")
    print("  - space_joined")
    print("  - error")
    print("  - ack")
    print()
    print("Connecting... (press Ctrl+C to stop)")
    print("=" * 60)
    print()

    # ----------------------------------------------------------------
    # Start the agent (blocks until Ctrl+C)
    # ----------------------------------------------------------------
    try:
        agent.run()
    except KeyboardInterrupt:
        print()
        print("Agent stopped by user.")


if __name__ == "__main__":
    main()
