"""
Gaggle SDK Quickstart - Make sure the server is running.

This script demonstrates the full REST API lifecycle:
  register_agent -> create_space -> send_message -> submit_proposal -> get_messages

Set GAGGLE_SERVER to your server URL (defaults to http://localhost:8080).

Run:
    python examples/quickstart.py
"""

import asyncio
import os
import sys

from gaggle import GaggleClient, GaggleError

BASE_URL = os.environ.get("GAGGLE_SERVER", "http://localhost:8080")


async def main() -> None:
    # --- Step 1: Register a consumer agent ---
    print("=" * 60)
    print("Step 1: Registering a consumer agent ...")
    print("=" * 60)

    async with GaggleClient(api_key="gag_demo", base_url=BASE_URL) as client:
        try:
            reg = await client.register_agent(
                agent_type="consumer",
                name="Demo Consumer",
            )
            print(f"  Registered agent:  id={reg.id}")
            print(f"  API key:           {reg.api_key[:12]}...")
            print(f"  API secret:        {reg.api_secret[:12]}...")
        except GaggleError as exc:
            print(f"  ERROR registering agent: {exc}")
            print("  Make sure the Gaggle server is running at", BASE_URL)
            sys.exit(1)

        # Use the new credentials for subsequent calls
        consumer_key = reg.api_key
        consumer_id = reg.id

    # --- Step 2: Register a provider agent ---
    print()
    print("=" * 60)
    print("Step 2: Registering a provider agent ...")
    print("=" * 60)

    async with GaggleClient(api_key="gag_demo", base_url=BASE_URL) as client:
        try:
            reg2 = await client.register_agent(
                agent_type="provider",
                name="Demo Provider",
            )
            provider_id = reg2.id
            print(f"  Registered provider: id={provider_id}")
        except GaggleError as exc:
            print(f"  ERROR registering provider: {exc}")
            sys.exit(1)

    # --- Step 3: Create a negotiation space ---
    print()
    print("=" * 60)
    print("Step 3: Creating negotiation space ...")
    print("=" * 60)

    async with GaggleClient(api_key=consumer_key, base_url=BASE_URL) as client:
        try:
            space = await client.create_space(
                name="Price Negotiation Demo",
                invitee_ids=[provider_id],
                context={"topic": "data-analysis-service", "urgency": "normal"},
            )
            space_id = space.id
            print(f"  Created space: id={space_id}")
            print(f"  Name:          {space.name}")
            print(f"  Status:        {space.status}")
            print(f"  Members:       {space.agent_ids}")
        except GaggleError as exc:
            print(f"  ERROR creating space: {exc}")
            sys.exit(1)

        # --- Step 4: Send a message ---
        print()
        print("=" * 60)
        print("Step 4: Sending a message ...")
        print("=" * 60)

        try:
            msg = await client.send_message(
                space_id,
                "Hi! I need a data analysis report for Q3 revenue. What's your best price?",
            )
            print(f"  Sent message:   id={msg.id}")
            print(f"  Content:        {msg.content[:60]}...")
            print(f"  Sender:         {msg.sender_id}")
        except GaggleError as exc:
            print(f"  ERROR sending message: {exc}")
            sys.exit(1)

        # --- Step 5: Submit a proposal ---
        print()
        print("=" * 60)
        print("Step 5: Submitting a proposal ...")
        print("=" * 60)

        try:
            proposal = await client.submit_proposal(
                space_id,
                proposal_type="initial",
                dimensions={"price": 500.0, "timeline_days": 7, "quality_tier": "premium"},
            )
            print(f"  Submitted proposal: id={proposal.id}")
            print(f"  Type:               {proposal.proposal_type}")
            print(f"  Dimensions:         price={proposal.dimensions.price}, "
                  f"timeline={proposal.dimensions.timeline_days}d, "
                  f"quality={proposal.dimensions.quality_tier}")
        except GaggleError as exc:
            print(f"  ERROR submitting proposal: {exc}")
            sys.exit(1)

        # --- Step 6: Retrieve messages ---
        print()
        print("=" * 60)
        print("Step 6: Retrieving messages from space ...")
        print("=" * 60)

        try:
            messages = await client.get_space_messages(space_id)
            print(f"  Found {len(messages)} message(s):")
            for m in messages:
                print(f"    [{m.sender_id[:8]}...] {m.content[:70]}")
        except GaggleError as exc:
            print(f"  ERROR getting messages: {exc}")
            sys.exit(1)

        # --- Step 7: Retrieve proposals ---
        print()
        print("=" * 60)
        print("Step 7: Retrieving proposals from space ...")
        print("=" * 60)

        try:
            proposals = await client.get_space_proposals(space_id)
            print(f"  Found {len(proposals)} proposal(s):")
            for p in proposals:
                dims = p.dimensions
                price_str = f"${dims.price:.0f}" if dims.price else "N/A"
                timeline_str = f"{dims.timeline_days}d" if dims.timeline_days else "N/A"
                print(f"    {p.proposal_type}: {price_str}, {timeline_str}, "
                      f"quality={dims.quality_tier or 'N/A'}")
        except GaggleError as exc:
            print(f"  ERROR getting proposals: {exc}")
            sys.exit(1)

    print()
    print("=" * 60)
    print("Quickstart complete!")
    print("=" * 60)


if __name__ == "__main__":
    asyncio.run(main())
