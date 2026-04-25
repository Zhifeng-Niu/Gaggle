#!/usr/bin/env python3
"""
Gaggle 消费者 Agent

支持本地和远程 ECS 服务器，带自动注册和断线重连。

用法:
  python scripts/consumer_agent.py --server localhost:8080
  python scripts/consumer_agent.py --server 106.15.228.101
  GAGGLE_SERVER=106.15.228.101 python scripts/consumer_agent.py
"""

import argparse
import asyncio
import json
import logging
import os
import sys
import time
from typing import Optional

import aiohttp
import websockets

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(name)s] %(levelname)s %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger("consumer")

# ─── 配置 ──────────────────────────────────────────

DEFAULT_SERVER = os.environ.get("GAGGLE_SERVER", "localhost:8080")
MAX_RECONNECT_ATTEMPTS = 10
BASE_RECONNECT_DELAY = 1.0      # 秒
MAX_RECONNECT_DELAY = 30.0      # 秒


def build_urls(server: str) -> tuple[str, str]:
    """根据 server 地址生成 HTTP 和 WS URL。

    支持格式:
      - localhost:8080        -> http://localhost:8080, ws://localhost:8080
      - 106.15.228.101        -> http://106.15.228.101, ws://106.15.228.101
      - http://example.com    -> http://example.com, ws://example.com
    """
    server = server.strip().rstrip("/")
    if not server.startswith(("http://", "https://")):
        return f"http://{server}", f"ws://{server}"
    ws = server.replace("https://", "wss://").replace("http://", "ws://")
    return server, ws


# ─── Agent ─────────────────────────────────────────


class ConsumerAgent:
    """自主消费者 Agent：注册 → 连接 WS → 谈判循环。"""

    def __init__(self, server: str):
        self.http_url, self.ws_url = build_urls(server)
        self.agent_id: Optional[str] = None
        self.api_key: Optional[str] = None
        self.api_secret: Optional[str] = None
        self.websocket = None
        self.space_id: Optional[str] = None
        self._reconnect_attempts = 0
        self._running = False

    # ── 注册 ──

    async def register(self, name: str = "测试消费者Agent") -> bool:
        async with aiohttp.ClientSession() as session:
            async with session.post(
                f"{self.http_url}/api/v1/agents/register",
                json={
                    "agent_type": "consumer",
                    "name": name,
                    "metadata": {
                        "budget": "3000-5000",
                        "constraints": {"deadline": "2周", "style": "现代简约"},
                    },
                },
            ) as resp:
                if resp.status == 201:
                    data = await resp.json()
                    self.agent_id = data["id"]
                    self.api_key = data["api_key"]
                    self.api_secret = data.get("api_secret")
                    log.info("Registered: %s", self.agent_id)
                    return True
                log.error("Registration failed (%d): %s", resp.status, await resp.text())
                return False

    # ── WebSocket 连接 + 重连 ──

    async def connect_websocket(self) -> bool:
        url = f"{self.ws_url}/ws/v1/agents/{self.agent_id}"
        try:
            self.websocket = await websockets.connect(url)
            self._reconnect_attempts = 0
            log.info("WebSocket connected to %s", url)
            return True
        except Exception as exc:
            log.error("WebSocket connect failed: %s", exc)
            return False

    async def _reconnect_loop(self) -> bool:
        """指数退避重连，返回 True 表示成功重连。"""
        while self._reconnect_attempts < MAX_RECONNECT_ATTEMPTS:
            delay = min(
                BASE_RECONNECT_DELAY * (2 ** self._reconnect_attempts),
                MAX_RECONNECT_DELAY,
            )
            self._reconnect_attempts += 1
            log.warning(
                "Reconnecting in %.1fs (attempt %d/%d)",
                delay,
                self._reconnect_attempts,
                MAX_RECONNECT_ATTEMPTS,
            )
            await asyncio.sleep(delay)
            if await self.connect_websocket():
                return True
        log.error("Max reconnect attempts reached, giving up")
        return False

    # ── 消息发送 ──

    async def send(self, msg: dict) -> None:
        if self.websocket:
            await self.websocket.send(json.dumps(msg))

    async def create_space(self, name: str, invitee_ids: list, context: dict) -> None:
        await self.send({
            "type": "create_space",
            "payload": {"name": name, "invitee_ids": invitee_ids, "context": context},
        })
        log.info("Space creation requested: %s", name)

    async def send_counter(self, space_id: str, content: str, price: int) -> None:
        await self.send({
            "type": "message",
            "space_id": space_id,
            "payload": {
                "msg_type": "counter_proposal",
                "content": content,
                "metadata": {"price": price},
            },
        })
        log.info("Counter-proposal: %s", content[:60])

    async def accept(self, space_id: str) -> None:
        await self.send({
            "type": "message",
            "space_id": space_id,
            "payload": {"msg_type": "acceptance", "content": "接受报价，成交！"},
        })
        log.info("Acceptance sent")

    async def close_space(self, space_id: str, conclusion: str = "concluded",
                          final_terms: Optional[dict] = None) -> None:
        await self.send({
            "type": "close_space",
            "space_id": space_id,
            "payload": {"conclusion": conclusion, "final_terms": final_terms or {}},
        })
        log.info("Space close: %s", conclusion)

    # ── 消息处理 ──

    async def handle_message(self, data: dict) -> None:
        msg_type = data.get("type")
        payload = data.get("payload", {})
        space_id = data.get("space_id", "")

        if msg_type == "space_created":
            self.space_id = space_id
            space = payload.get("space", {})
            log.info("Space created: %s (id=%s)", space.get("name"), space_id)

        elif msg_type == "space_joined":
            agent_id = payload.get("agent_id", "")
            if agent_id != self.agent_id:
                await asyncio.sleep(1)
                await self.send_counter(
                    space_id, "我们预算有限，能否3500元成交？", 3500,
                )

        elif msg_type == "message":
            msg = payload.get("message", {})
            sender = msg.get("sender_id", "")
            msg_type_str = msg.get("msg_type", "")
            metadata = msg.get("metadata", {})
            if sender == self.agent_id:
                return
            price = metadata.get("price", 0)

            if msg_type_str == "proposal":
                await asyncio.sleep(1)
                await self.send_counter(
                    space_id, f"报价{price}元有点高，能否{price - 500}元？", price - 500,
                )
            elif msg_type_str == "counter_proposal":
                if price <= 4200:
                    await asyncio.sleep(1)
                    await self.accept(space_id)
                    await asyncio.sleep(0.5)
                    await self.close_space(space_id, "concluded", {"price": price})
                else:
                    await asyncio.sleep(1)
                    await self.send_counter(
                        space_id, f"{price}还是太高了，4000元如何？", 4000,
                    )
            elif msg_type_str == "acceptance":
                await asyncio.sleep(0.5)
                await self.close_space(space_id, "concluded")

        elif msg_type == "space_closed":
            log.info("Space %s closed: %s", space_id, payload.get("conclusion"))
            log.info("=" * 50)
            log.info("Negotiation complete!")
            log.info("=" * 50)
            self._running = False

        elif msg_type == "error":
            log.error("Error: %s - %s", payload.get("code"), payload.get("message"))

    # ── 主循环 ──

    async def listen(self) -> None:
        """带自动重连的消息监听循环。"""
        self._running = True
        while self._running:
            try:
                async for message in self.websocket:
                    data = json.loads(message)
                    await self.handle_message(data)
            except websockets.exceptions.ConnectionClosed:
                log.warning("WebSocket disconnected")
                if self._running:
                    if not await self._reconnect_loop():
                        self._running = False
            except Exception as exc:
                log.error("Listen error: %s", exc)
                if self._running:
                    if not await self._reconnect_loop():
                        self._running = False

    async def run(self, provider_id: str, timeout: int = 120) -> None:
        """完整流程：注册 → 连接 → 创建 Space → 谈判。"""
        if not await self.register():
            return

        if not await self.connect_websocket():
            return

        listener = asyncio.create_task(self.listen())
        await asyncio.sleep(1)

        await self.create_space(
            name="Logo设计项目",
            invitee_ids=[provider_id],
            context={
                "requirement": "设计一个现代感的科技公司logo",
                "budget": "3000-5000",
                "deadline": "2周",
                "style": "现代简约",
            },
        )

        try:
            await asyncio.wait_for(listener, timeout=timeout)
        except asyncio.TimeoutError:
            log.warning("Negotiation timed out after %ds", timeout)
        except KeyboardInterrupt:
            pass
        finally:
            self._running = False
            listener.cancel()
            try:
                await listener
            except asyncio.CancelledError:
                pass


# ─── CLI ───────────────────────────────────────────


async def _main() -> None:
    parser = argparse.ArgumentParser(description="Gaggle 消费者 Agent")
    parser.add_argument(
        "--server", default=DEFAULT_SERVER,
        help="服务器地址 (默认: $GAGGLE_SERVER 或 localhost:8080)",
    )
    parser.add_argument("--provider-id", help="Provider Agent ID（若省略则从文件读取）")
    parser.add_argument("--timeout", type=int, default=120, help="谈判超时（秒）")
    args = parser.parse_args()

    provider_id = args.provider_id
    if not provider_id:
        id_file = "/tmp/gaggle_provider_id.txt"
        if os.path.exists(id_file):
            with open(id_file) as f:
                provider_id = f.read().strip()
            log.info("Found provider ID: %s", provider_id)
        else:
            log.error("Provider ID not found. Use --provider-id or start provider first.")
            sys.exit(1)

    agent = ConsumerAgent(server=args.server)
    await agent.run(provider_id=provider_id, timeout=args.timeout)


if __name__ == "__main__":
    print("=" * 50)
    print("Gaggle 消费者 Agent")
    print("=" * 50)
    asyncio.run(_main())
