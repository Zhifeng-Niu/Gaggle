"""Integration test for ask() / reply() natural conversation flow.

Requires a running Gaggle server. Set GAGGLE_SERVER env var to override
the default http://localhost:8080.

Usage:
    GAGGLE_SERVER=http://106.15.228.101:8080 pytest tests/integration/test_ask_reply.py -v -s
"""

import asyncio
import os

import pytest

from gaggle import Agent

SERVER = os.environ.get("GAGGLE_SERVER", "http://localhost:8080")


@pytest.fixture
def api_keys():
    """Two pre-registered agent API keys for testing."""
    key_a = os.environ.get("GAGGLE_AGENT_A_KEY", "")
    key_b = os.environ.get("GAGGLE_AGENT_B_KEY", "")
    if not key_a or not key_b:
        pytest.skip("Set GAGGLE_AGENT_A_KEY and GAGGLE_AGENT_B_KEY env vars")
    return key_a, key_b


@pytest.mark.asyncio
async def test_ask_reply_conversation(api_keys):
    """Two agents have a natural back-and-forth conversation via ask/reply."""
    key_a, key_b = api_keys

    agent_a = Agent(api_key=key_a, base_url=SERVER)
    agent_b = Agent(api_key=key_b, base_url=SERVER)

    # Track received messages for verification
    b_received: list[dict] = []
    a_received: list[dict] = []

    @agent_a.on("new_message")
    async def a_handler(event):
        msg = event["payload"]["message"]
        if msg["sender_id"] != agent_a.agent_id:
            a_received.append(msg)

    @agent_b.on("new_message")
    async def b_handler(event):
        msg = event["payload"]["message"]
        if msg["sender_id"] != agent_b.agent_id:
            b_received.append(msg)
            # Auto-reply with correlation_id preserved
            await agent_b.reply(
                msg["space_id"],
                f"ECHO: {msg['content']}",
                reply_to=msg,
            )

    # Run both agents in background
    task_a = asyncio.create_task(agent_a.run_async())
    task_b = asyncio.create_task(agent_b.run_async())

    try:
        # Wait for both agents to connect
        await asyncio.sleep(2)

        # Agent A creates a space and invites B
        space = await agent_a.create_space(
            "ask-reply-test",
            [agent_b.agent_id],
            {"test": True},
        )
        space_id = space["id"]

        # Wait for B to receive the invitation and auto-join
        await asyncio.sleep(1)
        await agent_b._ws.join_space(space_id)
        await asyncio.sleep(1)

        # Agent A uses ask() to send a message and wait for B's reply
        response = await agent_a.ask(space_id, "Hello Agent B!", timeout=15)

        # Verify the response
        assert response is not None
        assert "ECHO: Hello Agent B!" in response["content"]

        # Second round of conversation
        response2 = await agent_a.ask(space_id, "How are you?", timeout=15)
        assert response2 is not None
        assert "ECHO: How are you?" in response2["content"]

    finally:
        # Cleanup
        task_a.cancel()
        task_b.cancel()
        try:
            await task_a
        except (asyncio.CancelledError, Exception):
            pass
        try:
            await task_b
        except (asyncio.CancelledError, Exception):
            pass
