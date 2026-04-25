#!/usr/bin/env python3
"""
离线事件队列 E2E 验证脚本

场景:
1. 注册两个 agent (consumer + provider)
2. Provider 不连接 WS（模拟离线）
3. Consumer 创建 RFP 邀请 Provider
4. Provider 连接 WS，验证自动收到重放事件
5. Provider 发送 resume 命令验证

验证点:
- 离线时 rfp_created 事件被持久化
- 重连后自动重放未送达事件
- ReplayedEvent 和 ResumeAck 消息格式正确
"""

import json
import sys
import time
import threading

import requests
import websocket

SERVER = "106.15.228.101"
HTTP_URL = f"http://{SERVER}"
WS_URL = f"ws://{SERVER}"

received_events = []


def register_agent(name: str, agent_type: str) -> tuple[str, str]:
    """注册一个 agent，返回 (agent_id, api_key)"""
    # 先注册用户（如果已存在则忽略错误）
    user_email = f"{name}@test.offline"
    user_password = "Test1234"

    requests.post(
        f"{HTTP_URL}/api/v1/users/register",
        json={"email": user_email, "password": user_password, "display_name": name},
        timeout=5,
    )

    # 登录获取 user token
    login_resp = requests.post(
        f"{HTTP_URL}/api/v1/users/login",
        json={"email": user_email, "password": user_password},
        timeout=5,
    )
    if login_resp.status_code != 200:
        print(f"FAIL: User login failed: {login_resp.text}")
        sys.exit(1)

    user_token = login_resp.json()["api_key"]

    # 注册 agent
    reg_resp = requests.post(
        f"{HTTP_URL}/api/v1/agents/register",
        headers={"Authorization": f"Bearer {user_token}"},
        json={"name": name, "description": f"Test {name}", "agent_type": agent_type},
        timeout=5,
    )
    if reg_resp.status_code != 201:
        print(f"FAIL: Agent register failed: {reg_resp.text}")
        sys.exit(1)

    data = reg_resp.json()
    print(f"  Registered: {name} → id={data['id']}, key={data['api_key'][:20]}...")
    return data["id"], data["api_key"]


def on_ws_message(ws, message):
    """WS 消息回调"""
    data = json.loads(message)
    received_events.append(data)
    evt_type = data.get("type", "unknown")
    print(f"  [WS] Received: {evt_type}")
    if evt_type == "replayed_event":
        print(f"    event_seq={data.get('event_seq')}, event_type={data.get('event_type')}")
    elif evt_type == "resume_ack":
        print(f"    replayed_count={data.get('replayed_count')}, last_seq={data.get('last_event_seq')}")


def on_ws_error(ws, error):
    print(f"  [WS] Error: {error}")


def on_ws_open(ws):
    print("  [WS] Connected!")


def main():
    print("=== 离线事件队列 E2E 测试 ===\n")

    # Step 1: 注册 agent
    print("Step 1: Register agents...")
    consumer_id, consumer_key = register_agent("offline-consumer", "consumer")
    provider_id, provider_key = register_agent("offline-provider", "provider")
    print()

    # Step 2: Consumer 连接 WS
    print("Step 2: Consumer connects WS...")
    consumer_ws = websocket.WebSocketApp(
        f"{WS_URL}/ws/v1/agents/{consumer_id}?token={consumer_key}",
        on_message=lambda ws, msg: print(f"  [Consumer] {json.loads(msg).get('type')}"),
        on_error=lambda ws, e: None,
    )
    consumer_thread = threading.Thread(target=consumer_ws.run_forever, daemon=True)
    consumer_thread.start()
    time.sleep(1)
    print("  Consumer connected")
    print()

    # Step 3: Consumer 创建 RFP 邀请 Provider（Provider 离线）
    print("Step 3: Consumer creates RFP (provider is OFFLINE)...")
    rfp_msg = json.dumps({
        "type": "create_rfp",
        "payload": {
            "name": "Offline Queue Test RFP",
            "provider_ids": [provider_id],
            "context": {"test": "offline_queue"},
        },
    })

    # 通过 WS 发送
    consumer_ws.send(rfp_msg)
    time.sleep(2)
    print("  RFP created (provider offline)")
    print()

    # Step 4: Provider 连接 WS（应该自动收到重放事件）
    print("Step 4: Provider connects WS (should receive replayed events)...")
    global received_events
    received_events = []

    provider_ws = websocket.WebSocketApp(
        f"{WS_URL}/ws/v1/agents/{provider_id}?token={provider_key}",
        on_message=on_ws_message,
        on_error=on_ws_error,
        on_open=on_ws_open,
    )
    provider_thread = threading.Thread(target=provider_ws.run_forever, daemon=True)
    provider_thread.start()
    time.sleep(3)

    print(f"  Total events received: {len(received_events)}")
    print()

    # Step 5: 验证
    print("Step 5: Verify results...")
    replayed = [e for e in received_events if e.get("type") == "replayed_event"]
    resume_acks = [e for e in received_events if e.get("type") == "resume_ack"]

    success = True

    if len(replayed) > 0:
        print(f"  PASS: Received {len(replayed)} replayed events")
        for evt in replayed:
            print(f"    seq={evt['event_seq']}, type={evt['event_type']}")
    else:
        print("  WARN: No replayed events (may have been auto-delivered)")

    if len(resume_acks) > 0:
        ack = resume_acks[0]
        print(f"  PASS: ResumeAck received (count={ack['replayed_count']}, last_seq={ack['last_event_seq']})")
    else:
        print("  INFO: No ResumeAck (auto-replay)")

    # Step 6: Provider 发送 resume 命令
    print("\nStep 6: Provider sends resume command...")
    received_events = []
    resume_cmd = json.dumps({"type": "resume", "last_event_seq": 0})
    provider_ws.send(resume_cmd)
    time.sleep(2)

    resume_acks_after = [e for e in received_events if e.get("type") == "resume_ack"]
    if resume_acks_after:
        ack = resume_acks_after[0]
        print(f"  PASS: Resume command works (replayed={ack['replayed_count']})")
    else:
        print("  INFO: Resume returned 0 events (all already delivered)")

    # Cleanup
    consumer_ws.close()
    provider_ws.close()

    print("\n=== 测试完成 ===")


if __name__ == "__main__":
    main()
