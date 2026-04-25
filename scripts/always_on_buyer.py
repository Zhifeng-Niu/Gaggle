#!/usr/bin/env python3
"""
Gaggle Buyer Agent
实现 Buyer 谈判流程：搜索 Sellers、创建 RFP、收集报价、选择最优
"""

import argparse
import json
import logging
import random
import time
import threading
from typing import Dict, Any, List, Optional

from gaggle_agent_base import GaggleAgent

logger = logging.getLogger("gaggle.buyer")


class AlwaysOnBuyer(GaggleAgent):
    """
    持久在线 Buyer Agent

    流程：
    1. 搜索 sellers (GET /api/v1/providers/search)
    2. create_rfp 邀请 sellers
    3. 收集 proposals -> share_best_terms -> 收集 counter proposals -> accept best
    """

    def __init__(self, name: str, server: str, description: str = ""):
        super().__init__(
            name=name,
            server=server,
            agent_type="buyer",
            description=description or "Always-on Buyer Agent"
        )

        # Negotiation state
        self.active_spaces: Dict[str, Dict[str, Any]] = {}
        self.proposals_received: Dict[str, List[Dict[str, Any]]] = {}

    def on_event(self, event_type: str, data: Dict[str, Any]):
        """处理服务器事件"""
        logger.info(f"Received event: {event_type}")

        if event_type == "rfp_created":
            self._handle_rfp_created(data)
        elif event_type == "space_created":
            self._handle_space_created(data)
        elif event_type == "new_proposal":
            self._handle_new_proposal(data)
        elif event_type == "best_terms_shared":
            self._handle_best_terms_shared(data)
        elif event_type == "proposal_update":
            self._handle_proposal_update(data)
        elif event_type == "space_status_changed":
            self._handle_space_status_changed(data)
        elif event_type == "error":
            logger.error(f"Server error: {data.get('payload', {}).get('message')}")
        elif event_type in ("pong", "spaces_list", "replayed_event", "resume_ack", "space_joined"):
            pass  # Ignore these events
        else:
            logger.debug(f"Unhandled event type: {event_type}")

    def search_sellers(self, limit: int = 5) -> List[Dict[str, Any]]:
        """搜索在线的 Sellers"""
        try:
            result = self.http_get(f"/api/v1/providers/search?limit={limit}")
            providers = result.get("providers", [])
            logger.info(f"Found {len(providers)} providers")
            return providers
        except Exception as e:
            logger.error(f"Failed to search sellers: {e}")
            return []

    def create_rfp(
        self,
        name: str,
        provider_ids: List[str],
        context: Optional[Dict[str, Any]] = None
    ) -> str:
        """创建 RFP 并邀请 Sellers"""
        payload = {
            "name": name,
            "provider_ids": provider_ids,
            "context": context or {}
        }

        self.send({
            "type": "create_rfp",
            "payload": payload
        })

        logger.info(f"Sent create_rfp: {name}, inviting {len(provider_ids)} providers")
        # Note: space_id will be returned in rfp_created event
        return "pending"

    def _handle_rfp_created(self, data: Dict[str, Any]):
        """处理 RFP 创建成功事件"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        space = payload.get("space", {})
        providers = payload.get("providers", [])

        logger.info(f"RFP created: {space.get('name')} ({space_id})")
        logger.info(f"Invited providers: {[p.get('name', 'unknown') for p in providers]}")

        # Initialize space state
        self.active_spaces[space_id] = {
            "name": space.get("name"),
            "status": "gathering_proposals",
            "providers": providers,
            "best_terms_shared": False,
            "best_proposal": None,
        }
        self.proposals_received[space_id] = []

    def _handle_space_created(self, data: Dict[str, Any]):
        """处理空间创建事件（作为创建者）"""
        space_id = data.get("space_id")
        logger.info(f"Space created: {space_id}")

    def _handle_new_proposal(self, data: Dict[str, Any]):
        """处理收到的新报价"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        proposal = payload.get("proposal", {})

        proposal_id = proposal.get("id")
        agent_id = proposal.get("agent_id")
        dimensions = proposal.get("dimensions", {})

        logger.info(f"New proposal {proposal_id} from {agent_id}: {dimensions}")

        # Record proposal
        if space_id not in self.proposals_received:
            self.proposals_received[space_id] = []
        self.proposals_received[space_id].append(proposal)

        # Update best proposal
        self._update_best_proposal(space_id)

        # Check if we should share best terms
        space_state = self.active_spaces.get(space_id, {})
        num_providers = len(space_state.get("providers", []))
        num_proposals = len(self.proposals_received.get(space_id, []))

        # If we have proposals from all invited providers, share best terms
        if not space_state.get("best_terms_shared") and num_proposals >= num_providers:
            self._share_best_terms(space_id)

    def _update_best_proposal(self, space_id: str):
        """更新最优报价"""
        proposals = self.proposals_received.get(space_id, [])
        if not proposals:
            return

        # Find best proposal (lowest price, then highest quality)
        best = proposals[0]
        for p in proposals[1:]:
            if self._is_better_proposal(p, best):
                best = p

        space_state = self.active_spaces.get(space_id, {})
        space_state["best_proposal"] = best

        logger.info(f"Best proposal updated: {best.get('dimensions')}")

    def _is_better_proposal(self, proposal_a: Dict, proposal_b: Dict) -> bool:
        """比较两个报价，判断 a 是否比 b 更好"""
        dims_a = proposal_a.get("dimensions", {})
        dims_b = proposal_b.get("dimensions", {})

        price_a = dims_a.get("price", float("inf"))
        price_b = dims_b.get("price", float("inf"))

        # Lower price is better
        if price_a != price_b:
            return price_a < price_b

        # If same price, higher quality is better
        quality_a = dims_a.get("quality_score", 0)
        quality_b = dims_b.get("quality_score", 0)
        return quality_a > quality_b

    def _share_best_terms(self, space_id: str):
        """分享最优条款（匿名）"""
        space_state = self.active_spaces.get(space_id, {})
        best_proposal = space_state.get("best_proposal")

        if not best_proposal:
            logger.warning(f"No best proposal to share for {space_id}")
            return

        best_dimensions = best_proposal.get("dimensions", {})

        self.send({
            "type": "share_best_terms",
            "payload": {
                "space_id": space_id,
                "best_dimensions": best_dimensions
            }
        })

        space_state["best_terms_shared"] = True
        logger.info(f"Shared best terms for {space_id}: {best_dimensions}")

        # Wait for counter proposals, then accept best
        def delayed_accept():
            time.sleep(10)  # Wait for counters
            self._accept_best_proposal(space_id)

        threading.Thread(target=delayed_accept, daemon=True).start()

    def _handle_best_terms_shared(self, data: Dict[str, Any]):
        """处理其他人分享的最优条款"""
        # For buyer, we initiated this, but log it for visibility
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        best_dimensions = payload.get("best_dimensions", {})

        logger.info(f"Best terms confirmed in {space_id}: {best_dimensions}")

    def _accept_best_proposal(self, space_id: str):
        """接受最优报价"""
        space_state = self.active_spaces.get(space_id, {})
        best_proposal = space_state.get("best_proposal")

        if not best_proposal:
            logger.warning(f"No best proposal to accept for {space_id}")
            return

        proposal_id = best_proposal.get("id")
        dimensions = best_proposal.get("dimensions", {})

        self.send({
            "type": "respond_to_proposal",
            "payload": {
                "space_id": space_id,
                "proposal_id": proposal_id,
                "action": "accept",
                "counter_dimensions": None
            }
        })

        logger.info(f"Accepted proposal {proposal_id}: {dimensions}")

        # Send message
        self.send({
            "type": "send_message",
            "space_id": space_id,
            "payload": {
                "msg_type": "text",
                "content": f"Thank you! We accept your proposal with price {dimensions.get('price')}."
            }
        })

        # Close space
        def delayed_close():
            time.sleep(2)
            self.send({
                "type": "close_space",
                "space_id": space_id,
                "payload": {"conclusion": "concluded"}
            })
            logger.info(f"Closed space {space_id}")

        threading.Thread(target=delayed_close, daemon=True).start()

    def _handle_proposal_update(self, data: Dict[str, Any]):
        """处理报价状态更新"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        proposal_id = payload.get("proposal_id")
        status = payload.get("status")
        action = payload.get("action")

        logger.info(f"Proposal {proposal_id} in {space_id}: status={status}, action={action}")

    def _handle_space_status_changed(self, data: Dict[str, Any]):
        """处理空间状态变更"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        old_status = payload.get("old_status")
        new_status = payload.get("new_status")

        logger.info(f"Space {space_id} status: {old_status} -> {new_status}")

        if space_id in self.active_spaces:
            self.active_spaces[space_id]["status"] = new_status

    def run_negotiation_round(self, rfp_name: str = None):
        """执行一轮谈判（用于 --once 模式）"""
        logger.info("Starting negotiation round...")

        # Search for sellers
        sellers = self.search_sellers(limit=5)
        if not sellers:
            logger.warning("No sellers found, waiting...")
            return

        provider_ids = [s.get("id") for s in sellers if s.get("id")]
        if not provider_ids:
            logger.warning("No valid provider IDs")
            return

        # Create RFP
        name = rfp_name or f"Purchase Request {int(time.time())}"
        context = {
            "item": "Office Supplies",
            "quantity": 100,
            "deadline": "2025-12-31"
        }

        self.create_rfp(name, provider_ids, context)

    def run_persistent(self, rfp_name: str = None, interval: int = 60):
        """持久模式：定期创建新的 RFP"""
        logger.info(f"Starting persistent buyer mode (interval: {interval}s)")

        def round_loop():
            while self._running:
                try:
                    # Clean up old spaces
                    self._cleanup_old_spaces()

                    # Start new round if no active spaces
                    if not self.active_spaces:
                        self.run_negotiation_round(rfp_name)

                    time.sleep(interval)
                except Exception as e:
                    logger.error(f"Error in round loop: {e}")
                    time.sleep(10)

        thread = threading.Thread(target=round_loop, daemon=True)
        thread.start()

        # Run base agent (WS connection)
        super().run()

    def _cleanup_old_spaces(self):
        """清理已结束的空间"""
        to_remove = []
        for space_id, state in self.active_spaces.items():
            if state.get("status") in ("concluded", "cancelled", "failed"):
                to_remove.append(space_id)

        for space_id in to_remove:
            logger.info(f"Cleaning up space {space_id}")
            del self.active_spaces[space_id]
            if space_id in self.proposals_received:
                del self.proposals_received[space_id]


def main():
    parser = argparse.ArgumentParser(description="Gaggle Always-On Buyer Agent")
    parser.add_argument(
        "--name",
        default="AlwaysOn-Buyer-1",
        help="Agent name (default: AlwaysOn-Buyer-1)"
    )
    parser.add_argument(
        "--server",
        required=True,
        help="Server address (e.g., 106.15.228.101)"
    )
    parser.add_argument(
        "--description",
        default="",
        help="Agent description"
    )
    parser.add_argument(
        "--once",
        action="store_true",
        help="Run one negotiation round and exit"
    )
    parser.add_argument(
        "--rfp-name",
        default=None,
        help="Name for the RFP (default: auto-generated)"
    )
    parser.add_argument(
        "--interval",
        type=int,
        default=60,
        help="Interval between rounds in persistent mode (default: 60s)"
    )

    args = parser.parse_args()

    agent = AlwaysOnBuyer(
        name=args.name,
        server=args.server,
        description=args.description
    )

    if args.once:
        logger.info(f"Starting Buyer Agent (one-shot mode): {args.name}")
        # Connect first
        agent.connect()

        # Start WS in background
        def run_ws():
            agent.ws.run_forever(ping_interval=0)

        ws_thread = threading.Thread(target=run_ws, daemon=True)
        ws_thread.start()

        # Wait a bit for connection
        time.sleep(2)

        # Run one round
        agent.run_negotiation_round(args.rfp_name)

        # Wait for results
        logger.info("Waiting for negotiation to complete (30s)...")
        time.sleep(30)

        logger.info("One-shot mode complete, exiting...")
        agent.stop()
    else:
        logger.info(f"Starting Buyer Agent (persistent mode): {args.name}")
        agent.run_persistent(
            rfp_name=args.rfp_name,
            interval=args.interval
        )


if __name__ == "__main__":
    main()
