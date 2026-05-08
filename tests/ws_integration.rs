//! Integration tests for Gaggle WebSocket API.

mod common;

use common::spawn_test_server;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type WsRx = futures_util::stream::SplitStream<WsStream>;

/// Register a user, return the user API key (usr_*).
async fn register_user(
    client: &reqwest::Client,
    base_url: &str,
    email: &str,
) -> String {
    let resp = client
        .post(format!("{base_url}/api/v1/users/register"))
        .json(&serde_json::json!({
            "email": email,
            "password": "test_password_123",
            "display_name": email.split('@').next().unwrap(),
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "user register failed: {} {}", resp.status(), resp.text().await.unwrap_or_default());
    let body: serde_json::Value = resp.json().await.unwrap();
    body["api_key"].as_str().unwrap().to_string()
}

/// Register agent via REST under a user, return (agent_id, api_key).
async fn register_agent(
    client: &reqwest::Client,
    base_url: &str,
    user_api_key: &str,
    name: &str,
) -> (String, String) {
    let resp = client
        .post(format!("{base_url}/api/v1/agents/register"))
        .bearer_auth(user_api_key)
        .json(&serde_json::json!({"agent_type": "consumer", "name": name}))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    (
        body["id"].as_str().unwrap().to_string(),
        body["api_key"].as_str().unwrap().to_string(),
    )
}

/// Connect WS for a registered agent, returns (sink, stream).
async fn connect_ws(base_url: &str, agent_id: &str, api_key: &str) -> (WsSink, WsRx) {
    let ws_url = format!(
        "ws://{}/ws/v1/agents/{}?token={}",
        base_url.trim_start_matches("http://"),
        agent_id,
        api_key
    );
    let (stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS connect failed");
    stream.split()
}

/// Read next text JSON message from WS.
async fn read_json(rx: &mut WsRx) -> serde_json::Value {
    loop {
        match rx.next().await {
            Some(Ok(Message::Text(text))) => {
                return serde_json::from_str(&text).expect("invalid JSON from WS");
            }
            Some(Ok(Message::Close(_))) => panic!("WS closed unexpectedly"),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            Some(Err(e)) => panic!("WS error: {e}"),
            None => panic!("WS stream ended"),
            _ => continue,
        }
    }
}

/// Drain pending WS messages within timeout.
async fn drain_messages(rx: &mut WsRx, max: usize) -> Vec<serde_json::Value> {
    let mut msgs = Vec::new();
    for _ in 0..max {
        match tokio::time::timeout(std::time::Duration::from_millis(200), rx.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    msgs.push(v);
                }
            }
            _ => break,
        }
    }
    msgs
}

#[tokio::test]
async fn test_ws_connect_and_ping_pong() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    let user_key = register_user(&client, &base_url, "ws-ping@example.com").await;
    let (agent_id, api_key) = register_agent(&client, &base_url, &user_key, "WSPingAgent").await;
    let (mut tx, mut rx) = connect_ws(&base_url, &agent_id, &api_key).await;

    // Drain any initial messages
    let _ = drain_messages(&mut rx, 5).await;

    // Send Ping
    let ping = serde_json::json!({"type": "ping", "timestamp": 12345});
    tx.send(Message::Text(ping.to_string())).await.unwrap();

    // Read Pong
    let msg = read_json(&mut rx).await;
    assert_eq!(
        msg["type"], "pong",
        "expected pong, got: {msg:?}"
    );
    assert_eq!(msg["timestamp"], 12345);
}

#[tokio::test]
async fn test_ws_create_space() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    let user_key = register_user(&client, &base_url, "ws-space@example.com").await;
    let (agent_id, api_key) = register_agent(&client, &base_url, &user_key, "WSCreator").await;
    let (mut tx, mut rx) = connect_ws(&base_url, &agent_id, &api_key).await;

    // Drain initial messages
    let _ = drain_messages(&mut rx, 5).await;

    // Create space via WS
    let create = serde_json::json!({
        "type": "create_space",
        "request_id": "req_1",
        "payload": {
            "name": "WS Test Space",
            "invitee_ids": [],
            "context": {"source": "ws_test"}
        }
    });
    tx.send(Message::Text(create.to_string())).await.unwrap();

    // Read ack then broadcast
    let ack = read_json(&mut rx).await;
    assert_eq!(ack["type"], "ack", "expected ack first, got: {ack:?}");

    let msg = read_json(&mut rx).await;
    assert_eq!(
        msg["type"], "space_created",
        "expected space_created broadcast, got: {msg:?}"
    );
    assert_eq!(msg["payload"]["space"]["name"], "WS Test Space");
}

#[tokio::test]
async fn test_ws_message_broadcast() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    // Register user and two agents
    let user_key = register_user(&client, &base_url, "ws-bcast@example.com").await;
    let (agent_a_id, agent_a_key) = register_agent(&client, &base_url, &user_key, "AgentA").await;
    let (agent_b_id, agent_b_key) = register_agent(&client, &base_url, &user_key, "AgentB").await;

    // Connect both via WS
    let (mut tx_a, mut rx_a) = connect_ws(&base_url, &agent_a_id, &agent_a_key).await;
    let (_tx_b, mut rx_b) = connect_ws(&base_url, &agent_b_id, &agent_b_key).await;

    // Drain initial messages
    let _ = drain_messages(&mut rx_a, 5).await;
    let _ = drain_messages(&mut rx_b, 5).await;

    // Agent A creates space with B as invitee
    let create = serde_json::json!({
        "type": "create_space",
        "request_id": "req_create",
        "payload": {
            "name": "Broadcast Test",
            "invitee_ids": [agent_b_id],
            "context": {}
        }
    });
    tx_a.send(Message::Text(create.to_string())).await.unwrap();

    // Read ack then SpaceCreated on A
    let ack = read_json(&mut rx_a).await;
    assert_eq!(ack["type"], "ack", "expected ack, got: {ack:?}");

    let msg_a = read_json(&mut rx_a).await;
    assert_eq!(msg_a["type"], "space_created", "A should get space_created");
    let space_id = msg_a["space_id"].as_str().unwrap().to_string();

    // Drain B's SpaceCreated broadcast
    let _ = drain_messages(&mut rx_b, 5).await;

    // Agent B joins via REST
    client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/join"))
        .bearer_auth(&agent_b_key)
        .send()
        .await
        .unwrap();

    // Drain join broadcasts
    let _ = drain_messages(&mut rx_a, 5).await;
    let _ = drain_messages(&mut rx_b, 5).await;

    // Agent A sends message via WS
    let send_msg = serde_json::json!({
        "type": "send_message",
        "request_id": "req_msg",
        "space_id": space_id,
        "payload": {
            "msg_type": "text",
            "content": "Hello from A!"
        }
    });
    tx_a.send(Message::Text(send_msg.to_string())).await.unwrap();

    // A gets ack then NewMessage; B gets NewMessage broadcast
    let ack_a = read_json(&mut rx_a).await;
    if ack_a["type"] == "error" {
        panic!("send_message error: {:?}", ack_a["payload"]);
    }
    assert_eq!(ack_a["type"], "ack", "A should get ack for send");

    let msg_a2 = read_json(&mut rx_a).await;
    assert_eq!(
        msg_a2["type"], "new_message",
        "A should receive new_message, got: {msg_a2:?}"
    );
    assert_eq!(msg_a2["payload"]["message"]["content"], "Hello from A!");

    let msg_b = read_json(&mut rx_b).await;
    assert_eq!(
        msg_b["type"], "new_message",
        "B should receive new_message, got: {msg_b:?}"
    );
    assert_eq!(msg_b["payload"]["message"]["content"], "Hello from A!");
}

#[tokio::test]
async fn test_ws_request_id_correlation() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    let user_key = register_user(&client, &base_url, "ws-corr@example.com").await;
    let (agent_id, api_key) = register_agent(&client, &base_url, &user_key, "CorrAgent").await;
    let (mut tx, mut rx) = connect_ws(&base_url, &agent_id, &api_key).await;

    // Drain initial messages
    let _ = drain_messages(&mut rx, 5).await;

    // 1. Ping with request_id → Pong should echo it back
    let ping = serde_json::json!({
        "type": "ping",
        "request_id": "ping_42",
        "timestamp": 99999
    });
    tx.send(Message::Text(ping.to_string())).await.unwrap();
    let pong = read_json(&mut rx).await;
    assert_eq!(pong["type"], "pong", "expected pong, got: {pong:?}");
    assert_eq!(pong["request_id"], "ping_42", "pong should echo request_id");

    // 2. ListSpaces with request_id → SpacesList should echo it back
    let list = serde_json::json!({
        "type": "list_spaces",
        "request_id": "list_99"
    });
    tx.send(Message::Text(list.to_string())).await.unwrap();
    let spaces = read_json(&mut rx).await;
    assert_eq!(spaces["type"], "spaces_list", "expected spaces_list, got: {spaces:?}");
    assert_eq!(spaces["request_id"], "list_99", "spaces_list should echo request_id");

    // 3. CreateSpace with request_id → Ack should echo it back
    let create = serde_json::json!({
        "type": "create_space",
        "request_id": "create_007",
        "payload": {
            "name": "Correlation Test",
            "invitee_ids": [],
            "context": {}
        }
    });
    tx.send(Message::Text(create.to_string())).await.unwrap();
    let ack = read_json(&mut rx).await;
    assert_eq!(ack["type"], "ack", "expected ack, got: {ack:?}");
    assert_eq!(ack["request_id"], "create_007", "ack should echo request_id");
    assert_eq!(ack["result"], "ok");
    let space_id = ack["space_id"].as_str().unwrap().to_string();

    // Drain space_created broadcast
    let _ = drain_messages(&mut rx, 5).await;

    // 4. SyncState with request_id → StateDelta should echo it back
    let sync = serde_json::json!({
        "type": "sync_state",
        "request_id": "sync_001",
        "space_id": space_id,
        "last_known_version": 0
    });
    tx.send(Message::Text(sync.to_string())).await.unwrap();
    let delta = read_json(&mut rx).await;
    assert_eq!(delta["type"], "state_delta", "expected state_delta, got: {delta:?}");
    assert_eq!(delta["request_id"], "sync_001", "state_delta should echo request_id");
}

#[tokio::test]
async fn test_ws_error_includes_request_id() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    let user_key = register_user(&client, &base_url, "ws-errcorr@example.com").await;
    let (agent_id, api_key) = register_agent(&client, &base_url, &user_key, "ErrCorrAgent").await;
    let (mut tx, mut rx) = connect_ws(&base_url, &agent_id, &api_key).await;

    // Drain initial messages
    let _ = drain_messages(&mut rx, 5).await;

    // Send invalid JSON with request_id — parse error should NOT include request_id
    // (parsing happens before request_id extraction)
    // Instead, send a valid but failing command with request_id
    let bad_sync = serde_json::json!({
        "type": "sync_state",
        "request_id": "err_sync_42",
        "space_id": "nonexistent_space",
        "last_known_version": 0
    });
    tx.send(Message::Text(bad_sync.to_string())).await.unwrap();
    let err = read_json(&mut rx).await;
    assert_eq!(err["type"], "error", "expected error, got: {err:?}");
    assert_eq!(
        err["request_id"], "err_sync_42",
        "error should echo request_id for correlation"
    );
    assert_eq!(err["payload"]["code"], "NOT_FOUND");
}
