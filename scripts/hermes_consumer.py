#!/usr/bin/env python3
"""
Gaggle Hermes Consumer Agent 测试脚本
Hermes 是一个智能消费者 Agent，负责发现 Provider、创建 RFP、管理谈判流程

使用方式:
  1. 启动 server: cargo run
  2. 启动多个 provider: python scripts/provider_agent.py --name "..."
  3. 启动 hermes: python scripts/hermes_consumer.py --rfp-name "Logo设计项目"
"""

import argparse
import json
import sys
import time
import threading
from typing import Optional, Dict, Any, List
import requests
import websocket


class HermesConsumer:
    def __init__(self, server_url: str = "localhost:3000", user_email: str = None, user_password: str = None):
        self.server_url = server_url
        self.base_url = f"http://{server_url}"
        self.ws_url = f"ws://{server_url}"
        self.user_email = user_email
        self.user_password = user_password

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

    def register_user(self) -> bool:
        """注册并登录用户，获取 usr_ API key"""
        email = self.user_email or "hermes@example.com"
        password = self.user_password or "hermes123"
        display_name = "Hermes"

        # 先尝试登录
        login_resp = requests.post(
            f"{self.base_url}/api/v1/users/login",
            json={"email": email, "password": password},
            timeout=10
        )

        if login_resp.status_code == 200:
            data = login_resp.json()
            self.user_api_key = data["api_key"]
            print(f"[Hermes] User logged in: {email}")
            return True

        # 登录失败则注册
        register_resp = requests.post(
            f"{self.base_url}/api/v1/users/register",
            json={
                "email": email,
                "password": password,
                "display_name": display_name
            },
            timeout=10
        )

        if register_resp.status_code == 201:
            data = register_resp.json()
            self.user_api_key = data["api_key"]
            print(f"[Hermes] User registered: {email}")
            return True
        else:
            print(f"[Hermes] User registration failed: {register_resp.text}")
            return False

    def register_agent(self) -> bool:
        """注册为消费者 Agent"""
        headers = {"Authorization": f"Bearer {self.user_api_key}"}

        resp = requests.post(
            f"{self.base_url}/api/v1/agents/register",
            headers=headers,
            json={
                "agent_type": "consumer",
                "name": "Hermes",
                "metadata": {
                    "role": "intelligent_buyer",
                    "budget_range": "3000-6000",
                    "preferences": ["quality", "timely_delivery"]
                }
            },
            timeout=10
        )

        if resp.status_code == 201:
            data = resp.json()
            self.agent_id = data["id"]
            self.agent_api_key = data["api_key"]
            print(f"[Hermes] Agent registered: {self.agent_id}")
            print(f"[Hermes] API Key: {self.agent_api_key}")
            return True
        else:
            print(f"[Hermes] Agent registration failed: {resp.text}")
            return False

    def search_providers(self, skills: Optional[str] = None) -> List[Dict]:
        """搜索 Provider"""
        headers = {"Authorization": f"Bearer {self.user_api_key}"}

        params = {}
        if skills:
            params["skills"] = skills

        resp = requests.get(
            f"{self.base_url}/api/v1/providers/search",
            headers=headers,
            params=params,
            timeout=10
        )

        if resp.status_code == 200:
            providers = resp.json()
            print(f"[Hermes] Found {len(providers)} providers")
            for i, provider in enumerate(providers):
                profile = provider.get("profile", {})
                print(f"[Hermes]   {i+1}. {profile.get('display_name')} - {profile.get('description')}")
                print(f"[Hermes]      Skills: {', '.join(profile.get('skills', []))}")
                print(f"[Hermes]      Pricing: {profile.get('pricing_model')} (${profile.get('min_price')}-${profile.get('max_price')})")
            return providers
        else:
            print(f"[Hermes] Provider search failed: {resp.text}")
            return []

    def select_providers(self, providers: List[Dict], count: int = 3) -> List[str]:
        """选择 Provider 参与 RFP"""
        selected = providers[:count]
        provider_ids = [p.get("id") for p in selected if p.get("id")]

        print(f"[Hermes] Selected {len(provider_ids)} providers for RFP:")
        for p in selected:
            profile = p.get("profile", {})
            print(f"[Hermes]   - {profile.get('display_name')} ({p.get('id')})")

        return provider_ids

    def get_provider_reputation(self, provider_id: str) -> Optional[Dict]:
        """获取 Provider 信誉评分"""
        resp = requests.get(
            f"{self.base_url}/api/v1/agents/{provider_id}/reputation",
            timeout=10
        )

        if resp.status_code == 200:
            return resp.json()
        return None

    def on_ws_message(self, ws, message):
        """处理 WebSocket 消息"""
        try:
            data = json.loads(message)
            self.handle_message(data)
        except json.JSONDecodeError as e:
            print(f"[Hermes] JSON decode error: {e}")
        except Exception as e:
            print(f"[Hermes] Message handling error: {e}")

    def on_ws_error(self, ws, error):
        """处理 WebSocket 错误"""
        print(f"[Hermes] WebSocket error: {error}")

    def on_ws_close(self, ws, close_status_code, close_msg):
        """处理 WebSocket 关闭"""
        print(f"[Hermes] WebSocket closed")

    def on_ws_open(self, ws):
        """WebSocket 连接建立"""
        print(f"[Hermes] WebSocket connected")

    def handle_message(self, data: Dict[str, Any]):
        """处理接收到的消息"""
        msg_type = data.get("type")
        payload = data.get("payload", {})
        space_id = data.get("space_id", "")

        if msg_type == "rfp_created":
            self.handle_rfp_created(space_id, payload)

        elif msg_type == "space_joined":
            self.handle_space_joined(space_id, payload)

        elif msg_type == "new_proposal":
            self.handle_new_proposal(space_id, payload)

        elif msg_type == "proposal_update":
            self.handle_proposal_update(space_id, payload)

        elif msg_type == "space_closed":
            self.handle_space_closed(space_id, payload)

        elif msg_type == "error":
            self.handle_error(payload)

        else:
            print(f"[Hermes] Unknown message type: {msg_type}")

    def handle_rfp_created(self, space_id: str, payload: Dict):
        """处理 RFP 创建事件"""
        space = payload.get("space", {})
        print(f"[Hermes] RFP Created: {space.get('name')} (id={space_id})")
        self.rfp_space_id = space_id

    def handle_space_joined(self, space_id: str, payload: Dict):
        """处理 Space 加入事件"""
        agent_id = payload.get("agent_id", "")
        print(f"[Hermes] Agent {agent_id} joined space {space_id}")

        if agent_id != self.agent_id and agent_id not in self.provider_agents:
            self.provider_agents.append(agent_id)

        # 检查是否所有 Provider 都已加入
        if len(self.provider_agents) >= len(self.provider_agents):
            print(f"[Hermes] All providers joined. Waiting for proposals...")

    def handle_new_proposal(self, space_id: str, payload: Dict):
        """处理新提案事件"""
        proposal = payload.get("proposal", {})
        sender_id = proposal.get("sender_id", "")
        proposal_type = proposal.get("proposal_type", "")
        proposal_id = proposal.get("id", "")
        dimensions = proposal.get("dimensions", {})

        if sender_id != self.agent_id:
            print(f"[Hermes] New proposal from {sender_id}")
            print(f"[Hermes]   Type: {proposal_type}")
            print(f"[Hermes]   Price: ${dimensions.get('price')}")
            print(f"[Hermes]   Timeline: {dimensions.get('timeline_days')} days")
            print(f"[Hermes]   Quality: {dimensions.get('quality_tier')}")

            self.proposals_received.append(proposal)

            # 收集了足够的提案后，评估并分享最优条款
            if len(self.proposals_received) >= len(self.provider_agents):
                time.sleep(1)
                self.evaluate_and_share_best_terms(space_id)

    def handle_proposal_update(self, space_id: str, payload: Dict):
        """处理提案更新事件"""
        proposal_id = payload.get("proposal_id", "")
        status = payload.get("status", "")
        action = payload.get("action", "")

        print(f"[Hermes] Proposal {proposal_id} updated: {status} ({action})")

        # 如果提案被接受，关闭 Space
        if status == "accepted":
            time.sleep(1)
            self.close_space(space_id, concluded=True)

    def handle_space_closed(self, space_id: str, payload: Dict):
        """处理 Space 关闭事件"""
        conclusion = payload.get("conclusion", "")
        print(f"[Hermes] Space {space_id} closed: {conclusion}")
        print("=" * 60)
        print("[Hermes] RFP Negotiation complete!")
        print("=" * 60)

        # 获取 Provider 信誉评分
        for provider_id in self.provider_agents:
            self.print_reputation(provider_id)

        self.running = False

    def handle_error(self, payload: Dict):
        """处理错误消息"""
        code = payload.get("code", "")
        message = payload.get("message", "")
        print(f"[Hermes] Error: {code} - {message}")

    def send_ws_message(self, msg: Dict):
        """发送 WebSocket 消息"""
        if self.ws:
            self.ws.send(json.dumps(msg))

    def create_rfp(self, name: str, provider_ids: List[str], context: Dict):
        """创建 RFP Space"""
        # 计算 deadline（当前时间 + 7 天）
        deadline = int(time.time()) + (7 * 24 * 60 * 60)

        msg = {
            "type": "create_rfp",
            "payload": {
                "name": name,
                "provider_ids": provider_ids,
                "allowed_rounds": self.max_rounds,
                "evaluation_criteria": ["price", "timeline_days", "quality_tier"],
                "deadline": deadline,
                "share_best_terms": True,
                "context": context
            }
        }
        self.send_ws_message(msg)
        print(f"[Hermes] Creating RFP: {name}")
        print(f"[Hermes] Invited providers: {provider_ids}")

    def evaluate_and_share_best_terms(self, space_id: str):
        """评估提案并分享最优条款"""
        if not self.proposals_received:
            print("[Hermes] No proposals to evaluate")
            return

        # 找出最优价格
        best_proposal = min(
            self.proposals_received,
            key=lambda p: p.get("dimensions", {}).get("price", float("inf"))
        )

        best_dimensions = best_proposal.get("dimensions", {})
        self.best_terms = best_dimensions

        print(f"[Hermes] Evaluating proposals...")
        print(f"[Hermes] Best price: ${best_dimensions.get('price')}")
        print(f"[Hermes] Best timeline: {best_dimensions.get('timeline_days')} days")
        print(f"[Hermes] Best quality: {best_dimensions.get('quality_tier')}")

        # 匿名分享最优条款
        msg = {
            "type": "share_best_terms",
            "payload": {
                "space_id": space_id,
                "best_dimensions": best_dimensions
            }
        }
        self.send_ws_message(msg)
        print(f"[Hermes] Shared best terms (anonymous)")

        # 等待 Provider 调整后，选择最优提案
        time.sleep(3)
        self.select_best_proposal(space_id)

    def select_best_proposal(self, space_id: str):
        """选择最优提案"""
        # 综合评分：价格（60%）+ 时间线（20%）+ 质量（20%）
        def score_proposal(proposal):
            dims = proposal.get("dimensions", {})
            price = dims.get("price", float("inf"))
            timeline = dims.get("timeline_days", 999)
            quality_map = {"basic": 1, "standard": 2, "premium": 3}
            quality = quality_map.get(dims.get("quality_tier", "standard"), 2)

            # 价格越低越好
            price_score = 10000 / max(price, 1)
            # 时间越短越好
            timeline_score = 100 / max(timeline, 1)
            # 质量越高越好
            quality_score = quality * 10

            return price_score * 0.6 + timeline_score * 0.2 + quality_score * 0.2

        best_proposal = max(self.proposals_received, key=score_proposal)
        self.selected_proposal_id = best_proposal.get("id")

        print(f"[Hermes] Selecting best proposal...")
        print(f"[Hermes] Selected: {self.selected_proposal_id}")
        print(f"[Hermes] Price: ${best_proposal.get('dimensions', {}).get('price')}")

        # 接受提案
        self.respond_to_proposal(space_id, self.selected_proposal_id, "accept")

    def respond_to_proposal(self, space_id: str, proposal_id: str, action: str):
        """响应提案"""
        msg = {
            "type": "respond_to_proposal",
            "payload": {
                "space_id": space_id,
                "proposal_id": proposal_id,
                "action": action
            }
        }
        self.send_ws_message(msg)
        print(f"[Hermes] Responded to proposal {proposal_id}: {action}")

    def close_space(self, space_id: str, concluded: bool = True):
        """关闭 Space"""
        conclusion = "concluded" if concluded else "cancelled"

        # 获取最终条款
        final_terms = {}
        if self.best_terms:
            final_terms = self.best_terms

        msg = {
            "type": "close_space",
            "space_id": space_id,
            "payload": {
                "conclusion": conclusion,
                "final_terms": final_terms
            }
        }
        self.send_ws_message(msg)
        print(f"[Hermes] Closing space: {conclusion}")

    def rate_provider(self, provider_id: str, space_id: str, rating: int = 5):
        """为 Provider 评分"""
        headers = {"Authorization": f"Bearer {self.user_api_key}"}

        data = {
            "agent_id": provider_id,
            "space_id": space_id,
            "event_type": "concluded",
            "outcome": "success",
            "rating": rating,
            "counterparty_id": self.agent_id
        }

        resp = requests.post(
            f"{self.base_url}/api/v1/spaces/{space_id}/rate",
            headers=headers,
            json=data,
            timeout=10
        )

        if resp.status_code == 201:
            result = resp.json()
            print(f"[Hermes] Rated provider {provider_id}: {rating}/5")
            print(f"[Hermes] New reputation score: {result.get('new_reputation_score')}")
        else:
            print(f"[Hermes] Failed to rate provider: {resp.text}")

    def print_reputation(self, agent_id: str):
        """打印信誉信息"""
        resp = requests.get(
            f"{self.base_url}/api/v1/agents/{agent_id}/reputation",
            timeout=10
        )

        if resp.status_code == 200:
            data = resp.json()
            summary = data.get("summary", {})
            print(f"[Hermes] Reputation for {agent_id}:")
            print(f"[Hermes]   Score: {summary.get('reputation_score', 0):.2f}")
            print(f"[Hermes]   Total negotiations: {summary.get('total_negotiations', 0)}")
            print(f"[Hermes]   Success rate: {summary.get('fulfillment_rate', 0) * 100:.1f}%")
            print(f"[Hermes]   Avg rating: {summary.get('avg_rating', 'N/A')}")
        else:
            print(f"[Hermes] No reputation data for {agent_id}")

    def connect_websocket(self) -> bool:
        """连接 WebSocket"""
        ws_url = f"{self.ws_url}/ws/v1/agents/{self.agent_id}"

        self.ws = websocket.WebSocketApp(
            ws_url,
            on_open=self.on_ws_open,
            on_message=self.on_ws_message,
            on_error=self.on_ws_error,
            on_close=self.on_ws_close
        )

        # 在新线程中运行 WebSocket
        self.running = True
        ws_thread = threading.Thread(target=self.ws.run_forever)
        ws_thread.daemon = True
        ws_thread.start()

        # 等待连接建立
        time.sleep(1)
        return True

    def run_rfp(self, rfp_name: str, rfp_context: Dict):
        """运行完整 RFP 流程"""
        print("=" * 60)
        print("Hermes Consumer Agent - RFP Negotiation")
        print("=" * 60)

        # 1. 注册用户
        if not self.register_user():
            return False

        # 2. 注册 Agent
        if not self.register_agent():
            return False

        # 3. 连接 WebSocket
        if not self.connect_websocket():
            return False

        # 4. 搜索 Provider
        providers = self.search_providers(skills="logo设计")
        if not providers:
            print("[Hermes] No providers found. Please start provider agents first.")
            return False

        # 5. 选择 Provider
        provider_ids = self.select_providers(providers, count=3)
        self.provider_agents = provider_ids

        # 6. 等待 WebSocket 连接稳定
        time.sleep(1)

        # 7. 创建 RFP
        self.create_rfp(rfp_name, provider_ids, rfp_context)

        # 8. 等待谈判完成
        print("[Hermes] Waiting for negotiation to complete...")
        try:
            while self.running:
                time.sleep(1)
        except KeyboardInterrupt:
            print("\n[Hermes] Interrupted by user")
            if self.rfp_space_id:
                self.close_space(self.rfp_space_id, concluded=False)

        # 9. 为 Provider 评分
        if self.rfp_space_id and self.provider_agents:
            for provider_id in self.provider_agents:
                self.rate_provider(provider_id, self.rfp_space_id, rating=5)

        return True

    def wait(self, timeout: int = 300):
        """等待事件"""
        try:
            start = time.time()
            while self.running and (time.time() - start) < timeout:
                time.sleep(1)
        except KeyboardInterrupt:
            print("\n[Hermes] Interrupted")
            self.running = False
            if self.ws:
                self.ws.close()


def main():
    parser = argparse.ArgumentParser(description="Gaggle Hermes Consumer Agent")
    parser.add_argument("--server", default="localhost:3000", help="服务器地址")
    parser.add_argument("--user-email", default="hermes@example.com", help="用户邮箱")
    parser.add_argument("--user-password", default="hermes123", help="用户密码")
    parser.add_argument("--rfp-name", default="Logo设计RFP", help="RFP名称")
    parser.add_argument("--rfp-description", default="需要为一个科技初创公司设计Logo，要求现代简约风格", help="RFP描述")

    args = parser.parse_args()

    hermes = HermesConsumer(
        server_url=args.server,
        user_email=args.user_email,
        user_password=args.user_password
    )

    rfp_context = {
        "description": args.rfp_description,
        "budget_range": "3000-6000",
        "deadline": "2周",
        "requirements": [
            "现代简约风格",
            "提供源文件",
            "包含品牌指南",
            "支持3轮修改"
        ]
    }

    hermes.run_rfp(args.rfp_name, rfp_context)


if __name__ == "__main__":
    main()
