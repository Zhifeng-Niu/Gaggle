#!/usr/bin/env python3
"""
Gaggle Seller Agent
实现 Seller 谈判状态机：自动加入 RFP、提交报价、响应反报价
"""

import argparse
import json
import logging
import random
import time
from enum import Enum
from typing import Dict, Any, Optional

from gaggle_agent_base import GaggleAgent

logger = logging.getLogger("gaggle.seller")


class SellerState(Enum):
    """Seller 谈判状态"""
    IDLE = "idle"
    JOINED = "joined"
    PROPOSAL_SENT = "proposal_sent"
    DONE = "done"


class AlwaysOnSeller(GaggleAgent):
    """
    持久在线 Seller Agent

    状态机流转（per-space）：
    idle -> rfp_created -> join_space -> submit_proposal -> wait_response
    wait_response -> (best_terms_shared) -> counter_proposal -> wait_response
    wait_response -> (proposal accepted) -> done
    wait_response -> (proposal rejected) -> submit_another or done
    """

    def __init__(self, name: str, server: str, description: str = ""):
        super().__init__(
            name=name,
            server=server,
            agent_type="provider",
            description=description or "Always-on Seller Agent"
        )

        # Per-space negotiation state
        self.spaces: Dict[str, Dict[str, Any]] = {}

    def _get_space_state(self, space_id: str) -> Dict[str, Any]:
        """获取或创建空间状态"""
        if space_id not in self.spaces:
            self.spaces[space_id] = {
                "state": SellerState.IDLE,
                "my_proposals": [],
                "best_terms_seen": None,
                "assigned_role": None,
            }
        return self.spaces[space_id]

    def on_event(self, event_type: str, data: Dict[str, Any]):
        """处理服务器事件"""
        logger.info(f"Received event: {event_type}")

        # Handle different event types
        if event_type == "rfp_created":
            self._handle_rfp_created(data)
        elif event_type == "space_joined":
            self._handle_space_joined(data)
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
        elif event_type in ("pong", "spaces_list", "replayed_event", "resume_ack"):
            pass  # Ignore these events
        else:
            logger.debug(f"Unhandled event type: {event_type}")

    def _handle_rfp_created(self, data: Dict[str, Any]):
        """处理 RFP 创建事件 - 自动加入空间"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        space_info = payload.get("space", {})

        logger.info(f"RFP created: {space_info.get('name')} ({space_id})")

        # Auto-join the space
        self.send({
            "type": "join_space",
            "payload": {
                "space_id": space_id
            }
        })
        logger.info(f"Sent join_space for {space_id}")

    def _handle_space_joined(self, data: Dict[str, Any]):
        """处理成功加入空间事件"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        assigned_role = payload.get("assigned_role", "seller")

        state = self._get_space_state(space_id)
        state["state"] = SellerState.JOINED
        state["assigned_role"] = assigned_role

        logger.info(f"Joined space {space_id} as {assigned_role}")

        # Delay 2 seconds then submit initial proposal
        def delayed_submit():
            time.sleep(2)
            self._submit_initial_proposal(space_id)

        # Start thread for delayed submission
        import threading
        threading.Thread(target=delayed_submit, daemon=True).start()

    def _submit_initial_proposal(self, space_id: str):
        """提交初始报价"""
        state = self._get_space_state(space_id)

        # Generate random dimensions
        dimensions = {
            "price": round(random.uniform(80, 120), 2),
            "delivery_days": random.randint(3, 14),
            "quality_score": random.randint(70, 99),
        }

        logger.info(f"Submitting initial proposal for {space_id}: {dimensions}")

        self.send({
            "type": "submit_proposal",
            "payload": {
                "space_id": space_id,
                "proposal_type": "initial",
                "dimensions": dimensions
            }
        })

        state["state"] = SellerState.PROPOSAL_SENT
        state["my_proposals"].append(dimensions)

    def _handle_new_proposal(self, data: Dict[str, Any]):
        """处理收到的新报价（可能是反报价）"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        proposal = payload.get("proposal", {})

        proposal_id = proposal.get("id")
        dimensions = proposal.get("dimensions", {})
        agent_id = proposal.get("agent_id")

        logger.info(f"New proposal {proposal_id} in {space_id}: {dimensions}")

        state = self._get_space_state(space_id)

        # If this is a counter proposal (not from me), evaluate it
        if agent_id != self.agent_id and state["state"] == SellerState.PROPOSAL_SENT:
            # Store for reference
            state["last_counter_proposal"] = {
                "id": proposal_id,
                "dimensions": dimensions
            }
            # State machine will wait for proposal_update for response

    def _handle_best_terms_shared(self, data: Dict[str, Any]):
        """处理最优条款分享事件 - 决定是否反报价"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        best_dimensions = payload.get("best_dimensions", {})

        logger.info(f"Best terms shared in {space_id}: {best_dimensions}")

        state = self._get_space_state(space_id)
        state["best_terms_seen"] = best_dimensions

        # Decision: if best terms are much better than our offer, counter
        # Otherwise, we might accept
        my_last_proposal = state["my_proposals"][-1] if state["my_proposals"] else {}
        my_price = my_last_proposal.get("price", 100)

        best_price = best_dimensions.get("price", my_price)

        # If best price is 10% lower than ours, counter with a better offer
        if best_price < my_price * 0.9:
            logger.info(f"Best price {best_price} is much lower, countering...")

            counter_price = my_price * 0.95  # Match halfway
            counter_dimensions = {
                "price": round(counter_price, 2),
                "delivery_days": my_last_proposal.get("delivery_days", 7),
                "quality_score": my_last_proposal.get("quality_score", 85),
            }

            def delayed_counter():
                time.sleep(2)
                self.send({
                    "type": "submit_proposal",
                    "payload": {
                        "space_id": space_id,
                        "proposal_type": "counter",
                        "dimensions": counter_dimensions
                    }
                })
                logger.info(f"Sent counter proposal: {counter_dimensions}")
                state["my_proposals"].append(counter_dimensions)

            import threading
            threading.Thread(target=delayed_counter, daemon=True).start()
        else:
            logger.info(f"Best price {best_price} is close to ours, waiting...")

    def _handle_proposal_update(self, data: Dict[str, Any]):
        """处理报价状态更新"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        proposal_id = payload.get("proposal_id")
        status = payload.get("status")
        action = payload.get("action")

        logger.info(f"Proposal {proposal_id} in {space_id}: status={status}, action={action}")

        state = self._get_space_state(space_id)

        # Check if our proposal was accepted
        if status == "accepted" and action == "responded":
            logger.info(f"Our proposal was accepted! Space {space_id} concluded.")
            state["state"] = SellerState.DONE

            # Send a message
            self.send({
                "type": "send_message",
                "space_id": space_id,
                "payload": {
                    "msg_type": "text",
                    "content": f"Great! Deal concluded. Thank you for the opportunity."
                }
            })

            # Leave the space after a delay
            def delayed_leave():
                time.sleep(3)
                self.send({
                    "type": "leave_space",
                    "payload": {"space_id": space_id}
                })
                logger.info(f"Left space {space_id}")

            import threading
            threading.Thread(target=delayed_leave, daemon=True).start()

    def _handle_space_status_changed(self, data: Dict[str, Any]):
        """处理空间状态变更"""
        space_id = data.get("space_id")
        payload = data.get("payload", {})
        old_status = payload.get("old_status")
        new_status = payload.get("new_status")

        logger.info(f"Space {space_id} status: {old_status} -> {new_status}")

        state = self._get_space_state(space_id)

        if new_status in ("concluded", "cancelled", "failed"):
            state["state"] = SellerState.DONE


def main():
    parser = argparse.ArgumentParser(description="Gaggle Always-On Seller Agent")
    parser.add_argument(
        "--name",
        default="AlwaysOn-Seller-1",
        help="Agent name (default: AlwaysOn-Seller-1)"
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

    args = parser.parse_args()

    agent = AlwaysOnSeller(
        name=args.name,
        server=args.server,
        description=args.description
    )

    logger.info(f"Starting Seller Agent: {args.name}")
    agent.run()


if __name__ == "__main__":
    main()
