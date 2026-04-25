#!/usr/bin/env python3
"""
Gaggle Agent 基类
提供凭证管理、WebSocket 连接、心跳和重连机制
"""

import json
import os
import time
import threading
import logging
from pathlib import Path
from typing import Optional, Dict, Any

import requests
import websocket

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(name)s] %(levelname)s: %(message)s")
logger = logging.getLogger("gaggle.agent")


class GaggleAgent:
    """Gaggle Agent 基类 — 凭证管理 + WS 连接 + 心跳 + 重连"""

    def __init__(
        self,
        name: str,
        server: str,
        agent_type: str = "provider",
        description: str = ""
    ):
        self.name = name
        self.server = server
        self.agent_type = agent_type
        self.description = description or f"Agent {name}"

        self.http_url = f"http://{server}"
        self.ws_url = f"ws://{server}"

        self.agent_id: Optional[str] = None
        self.api_key: Optional[str] = None
        self.ws: Optional[websocket.WebSocketApp] = None
        self._running = False
        self._reconnect_delay = 2
        self._max_reconnect_delay = 60
        self._last_event_seq = 0

        # Load or register
        self._load_or_register()

    # --- 凭证管理 ---

    def _cred_path(self) -> Path:
        """获取凭证文件路径"""
        p = Path.home() / ".gaggle" / "agents"
        p.mkdir(parents=True, exist_ok=True)
        return p / f"{self.name}.json"

    def _load_or_register(self):
        """加载现有凭证或注册新 Agent"""
        path = self._cred_path()
        if path.exists():
            data = json.loads(path.read_text())
            self.agent_id = data["agent_id"]
            self.api_key = data["api_key"]
            logger.info(f"Loaded credentials for {self.name}: id={self.agent_id[:8]}...")
            return

        logger.info(f"Registering new agent: {self.name}")

        # Register user
        email = f"{self.name}@gaggle.agent"
        password = "AgentPass123!"
        try:
            requests.post(
                f"{self.http_url}/api/v1/users/register",
                json={"email": email, "password": password, "display_name": self.name},
                timeout=5
            )
        except requests.exceptions.RequestException:
            # User may already exist, continue to login
            pass

        # Login
        login_resp = requests.post(
            f"{self.http_url}/api/v1/users/login",
            json={"email": email, "password": password},
            timeout=5
        )
        login_data = login_resp.json()
        user_token = login_data["api_key"]

        # Register agent
        reg_resp = requests.post(
            f"{self.http_url}/api/v1/agents/register",
            headers={"Authorization": f"Bearer {user_token}"},
            json={
                "name": self.name,
                "description": self.description,
                "agent_type": self.agent_type
            },
            timeout=5
        )
        reg_data = reg_resp.json()
        self.agent_id = reg_data["id"]
        self.api_key = reg_data["api_key"]

        # Save credentials
        path.write_text(json.dumps({
            "agent_id": self.agent_id,
            "api_key": self.api_key
        }))
        logger.info(f"Registered: {self.name} -> id={self.agent_id[:8]}...")

    # --- WS 连接 ---

    def connect(self):
        """建立 WebSocket 连接"""
        url = f"{self.ws_url}/ws/v1/agents/{self.agent_id}?token={self.api_key}"
        self.ws = websocket.WebSocketApp(
            url,
            on_open=self._on_open,
            on_message=self._on_message,
            on_error=self._on_error,
            on_close=self._on_close,
        )

    def _on_open(self, ws):
        """WebSocket 连接建立回调"""
        logger.info("WS Connected!")
        self._reconnect_delay = 2
        # Resume missed events
        resume_msg = json.dumps({
            "type": "resume",
            "last_event_seq": self._last_event_seq
        })
        ws.send(resume_msg)

    def _on_message(self, ws, message):
        """WebSocket 消息接收回调"""
        try:
            data = json.loads(message)
        except json.JSONDecodeError:
            logger.warning(f"Invalid JSON message: {message[:100]}")
            return

        event_type = data.get("type", "unknown")

        # Track event seq for resume
        seq = data.get("event_seq")
        last_seq = data.get("last_event_seq")
        if seq and isinstance(seq, int) and seq > self._last_event_seq:
            self._last_event_seq = seq
        elif last_seq and isinstance(last_seq, int) and last_seq > self._last_event_seq:
            self._last_event_seq = last_seq

        # Dispatch to subclass
        self.on_event(event_type, data)

    def _on_error(self, ws, error):
        """WebSocket 错误回调"""
        logger.error(f"WS Error: {error}")

    def _on_close(self, ws, code, msg):
        """WebSocket 关闭回调"""
        logger.warning(f"WS Closed: code={code}, msg={msg}")

    def on_event(self, event_type: str, data: Dict[str, Any]):
        """
        事件处理回调，子类应覆盖此方法

        Args:
            event_type: 事件类型 (rfp_created, new_proposal, etc.)
            data: 事件数据完整载荷
        """
        pass

    def send(self, message: Dict[str, Any]):
        """发送消息到服务器"""
        if self.ws and self.ws.sock:
            self.ws.send(json.dumps(message))
        else:
            logger.warning("Cannot send: WS not connected")

    # --- 心跳 ---

    def _heartbeat_loop(self):
        """心跳线程：每30秒发送一次 ping"""
        while self._running:
            time.sleep(30)
            if self.ws and self.ws.sock and self.ws.sock.connected:
                self.send({
                    "type": "ping",
                    "timestamp": int(time.time() * 1000)
                })

    # --- 运行 ---

    def run(self):
        """启动 Agent（阻塞运行）"""
        self._running = True

        # Start heartbeat thread
        heartbeat_thread = threading.Thread(
            target=self._heartbeat_loop,
            daemon=True
        )
        heartbeat_thread.start()

        # Main connection loop with reconnection
        while self._running:
            try:
                self.connect()
                self.ws.run_forever(ping_interval=0)
            except Exception as e:
                logger.error(f"WS exception: {e}")

            if not self._running:
                break

            logger.info(f"Reconnecting in {self._reconnect_delay}s...")
            time.sleep(self._reconnect_delay)
            self._reconnect_delay = min(
                self._reconnect_delay * 2,
                self._max_reconnect_delay
            )

    def stop(self):
        """停止 Agent"""
        self._running = False
        if self.ws:
            self.ws.close()

    # --- REST API 辅助方法 ---

    def _get_headers(self) -> Dict[str, str]:
        """获取带有认证的请求头"""
        return {"Authorization": f"Bearer {self.api_key}"}

    def http_get(self, path: str) -> Dict[str, Any]:
        """发送 GET 请求"""
        url = f"{self.http_url}{path}"
        resp = requests.get(url, headers=self._get_headers(), timeout=10)
        resp.raise_for_status()
        return resp.json()

    def http_post(self, path: str, data: Dict[str, Any]) -> Dict[str, Any]:
        """发送 POST 请求"""
        url = f"{self.http_url}{path}"
        resp = requests.post(
            url,
            headers=self._get_headers(),
            json=data,
            timeout=10
        )
        resp.raise_for_status()
        return resp.json()
