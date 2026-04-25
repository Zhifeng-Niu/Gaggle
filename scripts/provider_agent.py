#!/usr/bin/env python3
"""
Gaggle 服务商 Agent

支持本地和远程 ECS 服务器，带自动注册和断线重连。

用法:
  python scripts/provider_agent.py --name "设计服务商A"
  python scripts/provider_agent.py --server 106.15.228.101 --name "远程服务商"
  GAGGLE_SERVER=106.15.228.101 python scripts/provider_agent.py
"""

import argparse
import json
import logging
import os
import sys
import time
import threading
from typing import Optional, Dict, Any

import requests
import websocket

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(name)s] %(levelname)s %(message)s",
    datefmt="%H:%M:%S",
)

# ─── 配置 ──────────────────────────────────────────

DEFAULT_SERVER = os.environ.get("GAGGLE_SERVER", "localhost:8080")
MAX_RECONNECT_ATTEMPTS = 10
BASE_RECONNECT_DELAY = 1.0
MAX_RECONNECT_DELAY = 30.0


def build_urls(server: str) -> tuple[str, str]:
    """根据 server 地址生成 HTTP 和 WS URL。"""
    server = server.strip().rstrip("/")
    if not server.startswith(("http://", "https://")):
        return f"http://{server}", f"ws://{server}"
    ws = server.replace("https://", "wss://").replace("http://", "ws://")
    return server, ws


# ─── Provider Agent ────────────────────────────────


class ProviderAgent:
    """自主服务商 Agent：注册 → 更新 Profile → 连接 WS → 等待 RFP → 自主谈判。"""

    def __init__(self, name: str, server: str,
                 user_email: Optional[str] = None,
                 user_password: Optional[str] = None):
        self.name = name
        self.http_url, self.ws_url = build_urls(server)
        self.user_email = user_email
        self.user_password = user_password

        self.user_api_key: Optional[str] = None
        self.agent_id: Optional[str] = None
        self.agent_api_key: Optional[str] = None
        self.ws: Optional[websocket.WebSocketApp] = None
        self.running = False

        # 谈判状态
        self.current_space_id: Optional[str] = None
        self.proposed_price: Optional[float] = None
        self.best_terms_received: Optional[Dict] = None

        # 重连状态
        self._reconnect_attempts = 0
        self._log = logging.getLogger(f"provider-{name}")

    # ── 用户 & Agent 注册 ──

    def register_user(self) -> bool:
        email = self.user_email or f"provider_{self.name.replace(' ', '_').lower()}@example.com"
        password = self.user_password or "password123"

        # 先尝试登录
        try:
            resp = requests.post(
                f"{self.http_url}/api/v1/users/login",
                json={"email": email, "password": password},
                timeout=10,
            )
            if resp.status_code == 200:
                self.user_api_key = resp.json()["api_key"]
                self._log.info("User logged in: %s", email)
                return True
        except requests.RequestException as exc:
            self._log.warning("Login request failed: %s", exc)

        # 注册新用户
        try:
            resp = requests.post(
                f"{self.http_url}/api/v1/users/register",
                json={"email": email, "password": password, "display_name": self.name},
                timeout=10,
            )
            if resp.status_code == 201:
                self.user_api_key = resp.json()["api_key"]
                self._log.info("User registered: %s", email)
                return True
            self._log.error("Registration failed (%d): %s", resp.status_code, resp.text)
        except requests.RequestException as exc:
            self._log.error("Registration request failed: %s", exc)
        return False

    def register_agent(self) -> bool:
        headers = {"Authorization": f"Bearer {self.user_api_key}"}
        try:
            resp = requests.post(
                f"{self.http_url}/api/v1/agents/register",
                headers=headers,
                json={
                    "agent_type": "provider",
                    "name": self.name,
                    "metadata": {
                        "category": "design",
                        "capabilities": ["logo", "branding", "ui_design"],
                    },
                },
                timeout=10,
            )
            if resp.status_code == 201:
                data = resp.json()
                self.agent_id = data["id"]
                self.agent_api_key = data["api_key"]
                self._log.info("Agent registered: %s", self.agent_id)
                # 持久化 agent_id 供 consumer 发现
                id_file = f"/tmp/gaggle_provider_id_{self.name.replace(' ', '_')}.txt"
                try:
                    with open(id_file, "w") as f:
                        f.write(self.agent_id)
                except OSError:
                    pass
                return True
            self._log.error("Agent registration failed (%d): %s", resp.status_code, resp.text)
        except requests.RequestException as exc:
            self._log.error("Agent registration request failed: %s", exc)
        return False

    def update_discovery_profile(self) -> bool:
        if not self.agent_api_key:
            return False
        headers = {"Authorization": f"Bearer {self.agent_api_key}"}
        profile_data = {
            "display_name": self.name,
            "description": f"专业{self.name}，提供高质量设计服务",
            "skills": ["logo设计", "品牌设计", "UI/UX设计"],
            "capabilities": {"category": "design", "tags": ["creative", "modern", "professional"]},
            "pricing_model": "negotiated",
            "availability_status": "available",
            "min_price": 3000,
            "max_price": 10000,
        }
        try:
            resp = requests.put(
                f"{self.http_url}/api/v1/providers/me/profile",
                headers=headers,
                json=profile_data,
                timeout=10,
            )
            if resp.status_code == 200:
                self._log.info("Discovery profile updated")
                return True
            self._log.error("Profile update failed (%d): %s", resp.status_code, resp.text)
        except requests.RequestException as exc:
            self._log.error("Profile update request failed: %s", exc)
        return False

    # ── WebSocket + 重连 ──

    def connect_websocket(self) -> bool:
        url = f"{self.ws_url}/ws/v1/agents/{self.agent_id}"

        self.ws = websocket.WebSocketApp(
            url,
            on_open=self._on_ws_open,
            on_message=self._on_ws_message,
            on_error=self._on_ws_error,
            on_close=self._on_ws_close,
        )

        self.running = True
        ws_thread = threading.Thread(target=self._ws_run_with_reconnect, daemon=True)
        ws_thread.start()
        time.sleep(1)
        return True

    def _ws_run_with_reconnect(self) -> None:
        """带指数退避重连的 WebSocket 运行循环。"""
        while self.running:
            try:
                self.ws.run_forever()
            except Exception as exc:
                self._log.error("WebSocket run error: %s", exc)

            if not self.running:
                break

            # 断线重连
            self._reconnect_attempts = 0
            while self._reconnect_attempts < MAX_RECONNECT_ATTEMPTS and self.running:
                delay = min(
                    BASE_RECONNECT_DELAY * (2 ** self._reconnect_attempts),
                    MAX_RECONNECT_DELAY,
                )
                self._reconnect_attempts += 1
                self._log.warning(
                    "Reconnecting in %.1fs (attempt %d/%d)",
                    delay, self._reconnect_attempts, MAX_RECONNECT_ATTEMPTS,
                )
                time.sleep(delay)

                # 重建 WebSocket 实例
                url = f"{self.ws_url}/ws/v1/agents/{self.agent_id}"
                self.ws = websocket.WebSocketApp(
                    url,
                    on_open=self._on_ws_open,
                    on_message=self._on_ws_message,
                    on_error=self._on_ws_error,
                    on_close=self._on_ws_close,
                )
                try:
                    self.ws.run_forever()
                    break  # 正常关闭
                except Exception:
                    continue

            if self._reconnect_attempts >= MAX_RECONNECT_ATTEMPTS:
                self._log.error("Max reconnect attempts reached")
                self.running = False
                break

    def _on_ws_open(self, ws):
        self._reconnect_attempts = 0
        self._log.info("WebSocket connected")

    def _on_ws_message(self, ws, message):
        try:
            data = json.loads(message)
            self.handle_message(data)
        except json.JSONDecodeError as exc:
            self._log.error("JSON decode error: %s", exc)
        except Exception as exc:
            self._log.error("Message handling error: %s", exc)

    def _on_ws_error(self, ws, error):
        self._log.error("WebSocket error: %s", error)

    def _on_ws_close(self, ws, close_status_code, close_msg):
        self._log.info("WebSocket closed (code=%s)", close_status_code)

    # ── 消息发送 ──

    def send_ws_message(self, msg: Dict) -> None:
        if self.ws:
            self.ws.send(json.dumps(msg))

    # ── 消息处理（自主谈判逻辑） ──

    def handle_message(self, data: Dict[str, Any]) -> None:
        msg_type = data.get("type")
        payload = data.get("payload", {})
        space_id = data.get("space_id", "")

        handler = {
            "rfp_created": self._handle_rfp_created,
            "space_joined": self._handle_space_joined,
            "new_proposal": self._handle_new_proposal,
            "proposal_update": self._handle_proposal_update,
            "best_terms_shared": self._handle_best_terms_shared,
            "space_closed": self._handle_space_closed,
            "error": self._handle_error,
        }.get(msg_type)

        if handler:
            handler(space_id, payload)
        else:
            self._log.info("Unknown message type: %s", msg_type)

    def _handle_rfp_created(self, space_id: str, payload: Dict) -> None:
        space = payload.get("space", {})
        self._log.info("RFP Created: %s (id=%s)", space.get("name"), space_id)
        self.current_space_id = space_id
        time.sleep(0.5)
        self.join_space(space_id)

    def _handle_space_joined(self, space_id: str, payload: Dict) -> None:
        agent_id = payload.get("agent_id", "")
        if agent_id == self.agent_id:
            time.sleep(0.5)
            self.submit_initial_proposal(space_id)

    def _handle_new_proposal(self, space_id: str, payload: Dict) -> None:
        proposal = payload.get("proposal", {})
        sender_id = proposal.get("sender_id", "")
        if sender_id != self.agent_id:
            dims = proposal.get("dimensions", {})
            price = dims.get("price")
            self._log.info("Proposal from %s — price: %s", sender_id, price)

    def _handle_proposal_update(self, space_id: str, payload: Dict) -> None:
        status = payload.get("status", "")
        self._log.info("Proposal %s: %s", payload.get("proposal_id"), status)
        if status == "accepted":
            self._log.info("Proposal accepted!")

    def _handle_best_terms_shared(self, space_id: str, payload: Dict) -> None:
        best = payload.get("best_dimensions", {})
        self._log.info("Best terms shared — price: $%s", best.get("price"))
        self.best_terms_received = best
        time.sleep(1)
        self.submit_counter_proposal(space_id, best)

    def _handle_space_closed(self, space_id: str, payload: Dict) -> None:
        conclusion = payload.get("conclusion", "")
        self._log.info("Space closed: %s", conclusion)
        self.current_space_id = None
        self.proposed_price = None
        self.best_terms_received = None

    def _handle_error(self, space_id: str, payload: Dict) -> None:
        self._log.error("Error: %s - %s", payload.get("code"), payload.get("message"))

    # ── 谈判动作 ──

    def join_space(self, space_id: str) -> None:
        self.send_ws_message({"type": "join_space", "payload": {"space_id": space_id}})
        self._log.info("Joining space: %s", space_id)

    def submit_initial_proposal(self, space_id: str) -> None:
        base_prices = {
            "专业设计服务商": 5000,
            "创意工作室": 5500,
            "高端设计公司": 6000,
            "经济设计坊": 4000,
        }
        base_price = base_prices.get(self.name, 4500)
        self.proposed_price = base_price

        self.send_ws_message({
            "type": "submit_proposal",
            "payload": {
                "space_id": space_id,
                "proposal_type": "initial",
                "dimensions": {
                    "price": base_price,
                    "timeline_days": 14,
                    "quality_tier": "premium",
                    "terms": {
                        "revisions": 3,
                        "deliverables": ["source_files", "brand_guidelines"],
                        "payment_terms": "50% upfront, 50% on delivery",
                    },
                },
                "parent_proposal_id": None,
            },
        })
        self._log.info("Initial proposal submitted: $%d", base_price)

    def submit_counter_proposal(self, space_id: str, best_dimensions: Dict) -> None:
        best_price = best_dimensions.get("price", 0)
        best_timeline = best_dimensions.get("timeline_days", 14)
        my_price = round(best_price * 1.1, 2)
        my_timeline = max(best_timeline - 2, 7) if my_price > best_price * 1.2 else best_timeline

        self.send_ws_message({
            "type": "submit_proposal",
            "payload": {
                "space_id": space_id,
                "proposal_type": "counter",
                "dimensions": {
                    "price": my_price,
                    "timeline_days": my_timeline,
                    "quality_tier": "premium",
                    "terms": {
                        "revisions": 5,
                        "deliverables": ["source_files", "brand_guidelines", "social_media_kit"],
                        "payment_terms": "40% upfront, 60% on delivery",
                        "support": "1 month free support",
                    },
                },
                "parent_proposal_id": None,
            },
        })
        self._log.info("Counter proposal: $%.2f (%d days)", my_price, my_timeline)

    # ── 等待 & 主运行 ──

    def wait(self, timeout: int = 3600) -> None:
        self._log.info("Waiting for RFP invitations...")
        try:
            while self.running and timeout > 0:
                time.sleep(1)
                timeout -= 1
        except KeyboardInterrupt:
            self._log.info("Shutting down...")
            self.running = False
            if self.ws:
                self.ws.close()

    def run(self) -> bool:
        self._log.info("=" * 50)
        self._log.info("Gaggle 服务商 Agent: %s", self.name)
        self._log.info("=" * 50)

        if not self.register_user():
            return False
        if not self.register_agent():
            return False
        if not self.update_discovery_profile():
            return False
        if not self.connect_websocket():
            return False

        self.wait()
        return True


# ─── CLI ───────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(description="Gaggle 服务商 Agent")
    parser.add_argument("--name", default="专业设计服务商", help="服务商名称")
    parser.add_argument("--server", default=DEFAULT_SERVER,
                        help="服务器地址 ($GAGGLE_SERVER 或 localhost:8080)")
    parser.add_argument("--user-email", help="用户邮箱")
    parser.add_argument("--user-password", help="用户密码")
    args = parser.parse_args()

    agent = ProviderAgent(
        name=args.name,
        server=args.server,
        user_email=args.user_email,
        user_password=args.user_password,
    )

    try:
        agent.run()
    except KeyboardInterrupt:
        print("\n[*] Provider agent stopped")
        sys.exit(0)


if __name__ == "__main__":
    main()
