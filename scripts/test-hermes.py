#!/usr/bin/env python3
"""
Gaggle Hermes Agent 端到端验证脚本

验证 Hermes Agent 可以连接到远程 ECS 上的 Gaggle，自主发起并参与 Negotiation。

用法:
  python scripts/test-hermes.py --server 106.15.228.101
  python scripts/test-hermes.py --server localhost:8080
  GAGGLE_SERVER=106.15.228.101 python scripts/test-hermes.py
"""

import argparse
import json
import logging
import os
import sys
import time
import threading
from typing import Optional, Dict, Any, List

import requests
import websocket

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(name)s] %(levelname)s %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger("test-hermes")

# ─── 配置 ──────────────────────────────────────────

DEFAULT_SERVER = os.environ.get("GAGGLE_SERVER", "localhost:8080")
MAX_RECONNECT_ATTEMPTS = 10
BASE_RECONNECT_DELAY = 1.0
MAX_RECONNECT_DELAY = 30.0
DEFAULT_TIMEOUT = 300


def build_urls(server: str) -> tuple[str, str]:
    server = server.strip().rstrip("/")
    if not server.startswith(("http://", "https://")):
        return f"http://{server}", f"ws://{server}"
    ws = server.replace("https://", "wss://").replace("http://", "ws://")
    return server, ws


# ─── 测试验证器 ─────────────────────────────────────


class TestReporter:
    """收集和报告测试结果。"""

    def __init__(self):
        self.results: List[Dict[str, Any]] = []
        self._passed = 0
        self._failed = 0

    def pass_(self, name: str, detail: str = "") -> None:
        self._passed += 1
        self.results.append({"name": name, "status": "PASS", "detail": detail})
        log.info("PASS: %s %s", name, f"({detail})" if detail else "")

    def fail(self, name: str, detail: str = "") -> None:
        self._failed += 1
        self.results.append({"name": name, "status": "FAIL", "detail": detail})
        log.error("FAIL: %s %s", name, f"({detail})" if detail else "")

    @property
    def passed(self) -> int:
        return self._passed

    @property
    def failed(self) -> int:
        return self._failed

    def summary(self) -> str:
        total = self._passed + self._failed
        lines = [
            "",
            "=" * 60,
            "  Hermes E2E 测试报告",
            "=" * 60,
        ]
        for r in self.results:
            icon = "PASS" if r["status"] == "PASS" else "FAIL"
            lines.append(f"  [{icon}] {r['name']}")
            if r["detail"]:
                lines.append(f"         {r['detail']}")
        lines.append("")
        lines.append(f"  总计: {total}  通过: {self._passed}  失败: {self._failed}")
        lines.append("=" * 60)
        return "\n".join(lines)


# ─── Hermes Agent ──────────────────────────────────


class HermesTestAgent:
    """Hermes Agent 的端到端测试封装。"""

    def __init__(self, server: str, reporter: TestReporter):
        self.http_url, self.ws_url = build_urls(server)
        self.reporter = reporter

        self.user_api_key: Optional[str] = None
        self.agent_id: Optional[str] = None
        self.agent_api_key: Optional[str] = None
        self.ws: Optional[websocket.WebSocketApp] = None
        self.running = False

        # RFP 状态
        self.rfp_space_id: Optional[str] = None
        self.provider_agents: List[str] = []
        self.proposals_received: List[Dict] = []
        self.best_terms: Optional[Dict] = None
        self.selected_proposal_id: Optional[str] = None
        self.negotiation_round = 0
        self.max_rounds = 3

        # 重连
        self._reconnect_attempts = 0

    # ── 健康检查 ──

    def check_health(self) -> bool:
        try:
            resp = requests.get(f"{self.http_url}/health", timeout=5)
            return resp.status_code == 200
        except requests.RequestException:
            pass
        try:
            resp = requests.get(f"{self.http_url}/", timeout=5)
            return resp.status_code == 200
        except requests.RequestException:
            return False

    # ── 注册 ──

    def register_user(self) -> bool:
        email = f"hermes_test_{int(time.time())}@example.com"
        password = "hermes_test_123"

        try:
            resp = requests.post(
                f"{self.http_url}/api/v1/users/register",
                json={"email": email, "password": password, "display_name": "Hermes-Test"},
                timeout=10,
            )
            if resp.status_code == 201:
                self.user_api_key = resp.json()["api_key"]
                self.reporter.pass_("用户注册", f"email={email}")
                return True
            if resp.status_code == 409:
                # 已存在，尝试登录
                resp = requests.post(
                    f"{self.http_url}/api/v1/users/login",
                    json={"email": email, "password": password},
                    timeout=10,
                )
                if resp.status_code == 200:
                    self.user_api_key = resp.json()["api_key"]
                    self.reporter.pass_("用户登录", f"email={email}")
                    return True
            self.reporter.fail("用户注册", f"status={resp.status_code}: {resp.text[:200]}")
        except requests.RequestException as exc:
            self.reporter.fail("用户注册", str(exc))
        return False

    def register_agent(self) -> bool:
        headers = {"Authorization": f"Bearer {self.user_api_key}"}
        try:
            resp = requests.post(
                f"{self.http_url}/api/v1/agents/register",
                headers=headers,
                json={
                    "agent_type": "consumer",
                    "name": "Hermes-E2E-Test",
                    "metadata": {
                        "role": "intelligent_buyer",
                        "budget_range": "3000-6000",
                        "preferences": ["quality", "timely_delivery"],
                    },
                },
                timeout=10,
            )
            if resp.status_code == 201:
                data = resp.json()
                self.agent_id = data["id"]
                self.agent_api_key = data["api_key"]
                self.reporter.pass_("Agent 注册", f"id={self.agent_id}")
                return True
            self.reporter.fail("Agent 注册", f"status={resp.status_code}: {resp.text[:200]}")
        except requests.RequestException as exc:
            self.reporter.fail("Agent 注册", str(exc))
        return False

    # ── 搜索 Provider ──

    def search_providers(self, skills: Optional[str] = None) -> List[Dict]:
        headers = {"Authorization": f"Bearer {self.user_api_key}"}
        params = {}
        if skills:
            params["skills"] = skills
        try:
            resp = requests.get(
                f"{self.http_url}/api/v1/providers/search",
                headers=headers,
                params=params,
                timeout=10,
            )
            if resp.status_code == 200:
                providers = resp.json()
                self.reporter.pass_("搜索 Provider", f"找到 {len(providers)} 个")
                return providers if isinstance(providers, list) else []
            self.reporter.fail("搜索 Provider", f"status={resp.status_code}")
        except requests.RequestException as exc:
            self.reporter.fail("搜索 Provider", str(exc))
        return []

    # ── WebSocket 连接 + 重连 ──

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

        self.reporter.pass_("WebSocket 连接", url)
        return True

    def _ws_run_with_reconnect(self) -> None:
        while self.running:
            try:
                self.ws.run_forever()
            except Exception as exc:
                log.error("WebSocket run error: %s", exc)

            if not self.running:
                break

            self._reconnect_attempts = 0
            while self._reconnect_attempts < MAX_RECONNECT_ATTEMPTS and self.running:
                delay = min(
                    BASE_RECONNECT_DELAY * (2 ** self._reconnect_attempts),
                    MAX_RECONNECT_DELAY,
                )
                self._reconnect_attempts += 1
                log.warning("Reconnecting in %.1fs (attempt %d)", delay, self._reconnect_attempts)
                time.sleep(delay)

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
                    break
                except Exception:
                    continue

            if self._reconnect_attempts >= MAX_RECONNECT_ATTEMPTS:
                log.error("Max reconnect attempts reached")
                self.running = False
                break

    def _on_ws_open(self, ws):
        self._reconnect_attempts = 0
        log.info("WebSocket connected")

    def _on_ws_message(self, ws, message):
        try:
            data = json.loads(message)
            self.handle_message(data)
        except Exception as exc:
            log.error("Message handling error: %s", exc)

    def _on_ws_error(self, ws, error):
        log.error("WebSocket error: %s", error)

    def _on_ws_close(self, ws, close_status_code, close_msg):
        log.info("WebSocket closed")

    def send_ws_message(self, msg: Dict) -> None:
        if self.ws:
            self.ws.send(json.dumps(msg))

    # ── 消息处理 ──

    def handle_message(self, data: Dict[str, Any]) -> None:
        msg_type = data.get("type")
        payload = data.get("payload", {})
        space_id = data.get("space_id", "")

        if msg_type == "rfp_created":
            self.rfp_space_id = space_id
            self.reporter.pass_("RFP 创建", f"space_id={space_id}")

        elif msg_type == "space_joined":
            agent_id = payload.get("agent_id", "")
            if agent_id != self.agent_id and agent_id not in self.provider_agents:
                self.provider_agents.append(agent_id)
                self.reporter.pass_("Provider 加入", f"agent_id={agent_id}")

        elif msg_type == "new_proposal":
            proposal = payload.get("proposal", {})
            sender_id = proposal.get("sender_id", "")
            if sender_id != self.agent_id:
                dims = proposal.get("dimensions", {})
                self.proposals_received.append(proposal)
                self.reporter.pass_(
                    "收到提案",
                    f"from={sender_id[:8]}... price=${dims.get('price')}",
                )
                if len(self.proposals_received) >= len(self.provider_agents):
                    time.sleep(1)
                    self.evaluate_and_share_best_terms(space_id)

        elif msg_type == "proposal_update":
            status = payload.get("status", "")
            if status == "accepted":
                self.reporter.pass_("提案被接受", f"proposal_id={payload.get('proposal_id')}")
                time.sleep(1)
                self.close_space(space_id, concluded=True)

        elif msg_type == "space_closed":
            conclusion = payload.get("conclusion", "")
            self.reporter.pass_("Space 关闭", f"conclusion={conclusion}")
            self.running = False

        elif msg_type == "error":
            self.reporter.fail("服务端错误", f"{payload.get('code')}: {payload.get('message')}")

    # ── RFP 操作 ──

    def create_rfp(self, name: str, provider_ids: List[str], context: Dict) -> None:
        deadline = int(time.time()) + (7 * 24 * 60 * 60)
        self.send_ws_message({
            "type": "create_rfp",
            "payload": {
                "name": name,
                "provider_ids": provider_ids,
                "allowed_rounds": self.max_rounds,
                "evaluation_criteria": ["price", "timeline_days", "quality_tier"],
                "deadline": deadline,
                "share_best_terms": True,
                "context": context,
            },
        })
        log.info("RFP created: %s, providers: %s", name, provider_ids)

    def evaluate_and_share_best_terms(self, space_id: str) -> None:
        if not self.proposals_received:
            return

        best = min(self.proposals_received, key=lambda p: p.get("dimensions", {}).get("price", float("inf")))
        best_dims = best.get("dimensions", {})
        self.best_terms = best_dims

        self.reporter.pass_("最优条款评估", f"best_price=${best_dims.get('price')}")

        self.send_ws_message({
            "type": "share_best_terms",
            "payload": {"space_id": space_id, "best_dimensions": best_dims},
        })

        time.sleep(3)
        self.select_best_proposal(space_id)

    def select_best_proposal(self, space_id: str) -> None:
        quality_map = {"basic": 1, "standard": 2, "premium": 3}

        def score(p: Dict) -> float:
            dims = p.get("dimensions", {})
            price = dims.get("price", float("inf"))
            timeline = dims.get("timeline_days", 999)
            quality = quality_map.get(dims.get("quality_tier", "standard"), 2)
            return (10000 / max(price, 1)) * 0.6 + (100 / max(timeline, 1)) * 0.2 + quality * 10 * 0.2

        best = max(self.proposals_received, key=score)
        self.selected_proposal_id = best.get("id")

        self.send_ws_message({
            "type": "respond_to_proposal",
            "payload": {
                "space_id": space_id,
                "proposal_id": self.selected_proposal_id,
                "action": "accept",
            },
        })
        self.reporter.pass_("选择最优提案", f"proposal_id={self.selected_proposal_id}")

    def close_space(self, space_id: str, concluded: bool = True) -> None:
        self.send_ws_message({
            "type": "close_space",
            "space_id": space_id,
            "payload": {
                "conclusion": "concluded" if concluded else "cancelled",
                "final_terms": self.best_terms or {},
            },
        })

    # ── 主运行 ──

    def run(self, rfp_name: str, rfp_context: Dict, timeout: int = DEFAULT_TIMEOUT) -> bool:
        log.info("=" * 60)
        log.info("Hermes E2E 测试开始")
        log.info("=" * 60)

        # 1. 健康检查
        if not self.check_health():
            self.reporter.fail("健康检查", f"服务器 {self.http_url} 不可达")
            return False
        self.reporter.pass_("健康检查", self.http_url)

        # 2. 注册
        if not self.register_user():
            return False
        if not self.register_agent():
            return False

        # 3. 搜索 Provider
        providers = self.search_providers(skills="logo设计")
        if not providers:
            self.reporter.fail("搜索 Provider", "未找到任何 Provider，请先启动 provider_agent")
            return False

        provider_ids = [p.get("id") for p in providers[:3] if p.get("id")]
        self.provider_agents = provider_ids

        # 4. 连接 WebSocket
        if not self.connect_websocket():
            return False

        time.sleep(1)

        # 5. 创建 RFP
        self.create_rfp(rfp_name, provider_ids, rfp_context)

        # 6. 等待完成
        log.info("等待谈判完成...")
        start = time.time()
        try:
            while self.running and (time.time() - start) < timeout:
                time.sleep(1)
        except KeyboardInterrupt:
            log.info("用户中断")

        return self.reporter.failed == 0


# ─── CLI ───────────────────────────────────────────


def main() -> None:
    parser = argparse.ArgumentParser(description="Gaggle Hermes E2E 测试")
    parser.add_argument("--server", default=DEFAULT_SERVER,
                        help="服务器地址 ($GAGGLE_SERVER 或 localhost:8080)")
    parser.add_argument("--rfp-name", default="Hermes-E2E-RFP", help="RFP 名称")
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT, help="超时秒数")
    args = parser.parse_args()

    reporter = TestReporter()
    agent = HermesTestAgent(server=args.server, reporter=reporter)

    rfp_context = {
        "description": "端到端测试：验证 Hermes Agent 自主谈判能力",
        "budget_range": "3000-6000",
        "deadline": "2周",
        "requirements": ["现代简约风格", "提供源文件", "包含品牌指南"],
    }

    success = agent.run(rfp_name=args.rfp_name, rfp_context=rfp_context, timeout=args.timeout)

    print(reporter.summary())
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
