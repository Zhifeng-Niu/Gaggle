//! Integration tests for Gaggle REST API.
//!
//! Each test spawns an in-memory server and exercises real HTTP flows.

mod common;

use common::spawn_test_server;

/// Helper: register a user and return the user's API key (usr_*).
async fn register_test_user(
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

/// Helper: register an agent under a user account and return (agent_id, agent_api_key).
async fn register_test_agent(
    client: &reqwest::Client,
    base_url: &str,
    user_api_key: &str,
    name: &str,
    agent_type: &str,
) -> (String, String) {
    let resp = client
        .post(format!("{base_url}/api/v1/agents/register"))
        .bearer_auth(user_api_key)
        .json(&serde_json::json!({
            "agent_type": agent_type,
            "name": name,
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "agent register failed: {} {}", resp.status(), resp.text().await.unwrap_or_default());
    let body: serde_json::Value = resp.json().await.unwrap();
    (
        body["id"].as_str().unwrap().to_string(),
        body["api_key"].as_str().unwrap().to_string(),
    )
}

/// Helper: create a bilateral space and have the invitee join.
/// Returns (space_id, consumer_key, provider_key).
async fn setup_space_with_members(
    client: &reqwest::Client,
    base_url: &str,
    user_api_key: &str,
    space_name: &str,
) -> (String, String, String) {
    let (_consumer_id, consumer_key) =
        register_test_agent(client, base_url, user_api_key, "Consumer", "consumer").await;
    let (provider_id, provider_key) =
        register_test_agent(client, base_url, user_api_key, "Provider", "provider").await;

    // Create space
    let resp = client
        .post(format!("{base_url}/api/v1/spaces"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({
            "name": space_name,
            "invitee_ids": [provider_id],
            "context": {}
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "create space failed: {}", resp.status());
    let space: serde_json::Value = resp.json().await.unwrap();
    let space_id = space["id"].as_str().unwrap().to_string();

    // Provider joins — this assigns seller role and activates the space
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/join"))
        .bearer_auth(&provider_key)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "join failed: {}", resp.status());

    (space_id, consumer_key, provider_key)
}

#[tokio::test]
async fn test_health_check() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    let resp = client.get(format!("{base_url}/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn test_register_and_get_agent() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    // Register user first, then agent under that user
    let user_key = register_test_user(&client, &base_url, "test@example.com").await;
    let (agent_id, api_key) = register_test_agent(&client, &base_url, &user_key, "TestAgent", "consumer").await;
    assert!(!agent_id.is_empty());
    assert!(api_key.starts_with("gag_"));

    // Get agent
    let resp = client
        .get(format!("{base_url}/api/v1/agents/{agent_id}"))
        .bearer_auth(&api_key)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "TestAgent");
    assert_eq!(body["agent_type"], "consumer");
}

#[tokio::test]
async fn test_create_space_and_join() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    let user_key = register_test_user(&client, &base_url, "space@example.com").await;
    let (_consumer_id, consumer_key) =
        register_test_agent(&client, &base_url, &user_key, "Consumer", "consumer").await;
    let (provider_id, provider_key) =
        register_test_agent(&client, &base_url, &user_key, "Provider", "provider").await;

    // Create space
    let resp = client
        .post(format!("{base_url}/api/v1/spaces"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({
            "name": "Price Negotiation",
            "invitee_ids": [provider_id],
            "context": {"topic": "service pricing"}
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "create space failed: {}", resp.status());
    let space: serde_json::Value = resp.json().await.unwrap();
    let space_id = space["id"].as_str().unwrap();
    assert_eq!(space["name"], "Price Negotiation");
    assert!(space["agent_ids"].as_array().unwrap().len() >= 1);

    // Provider joins
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/join"))
        .bearer_auth(&provider_key)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Get space — verify both members
    let resp = client
        .get(format!("{base_url}/api/v1/spaces/{space_id}"))
        .bearer_auth(&consumer_key)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn test_send_and_receive_messages() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    let user_key = register_test_user(&client, &base_url, "chat@example.com").await;
    let (space_id, consumer_key, provider_key) =
        setup_space_with_members(&client, &base_url, &user_key, "Chat").await;

    // Send message from consumer
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/send"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({"content": "Hello, provider!"}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "send message failed: {}", resp.status());
    let msg: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(msg["content"], "Hello, provider!");

    // Send reply from provider
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/send"))
        .bearer_auth(&provider_key)
        .json(&serde_json::json!({"content": "Hi consumer!"}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "reply failed: {}", resp.status());

    // Get messages
    let resp = client
        .get(format!("{base_url}/api/v1/spaces/{space_id}/messages"))
        .bearer_auth(&consumer_key)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "get messages failed: {}", resp.status());
    let messages: serde_json::Value = resp.json().await.unwrap();
    let msgs = messages.as_array().expect("messages should be array");
    assert_eq!(msgs.len(), 2, "should have 2 messages");
}

#[tokio::test]
async fn test_proposal_submit_and_accept() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    let user_key = register_test_user(&client, &base_url, "deal@example.com").await;
    let (space_id, consumer_key, provider_key) =
        setup_space_with_members(&client, &base_url, &user_key, "DealAccept").await;

    // Provider submits proposal
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/proposals/submit"))
        .bearer_auth(&provider_key)
        .json(&serde_json::json!({
            "proposal_type": "initial",
            "dimensions": {"price": 500, "timeline_days": 7}
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "submit proposal failed: {} {}", resp.status(), resp.text().await.unwrap_or_default());
    let proposal: serde_json::Value = resp.json().await.unwrap();
    let proposal_id = proposal["id"].as_str().unwrap();
    assert_eq!(proposal["status"], "pending");

    // Consumer accepts (buyer role can respond)
    let resp = client
        .post(format!(
            "{base_url}/api/v1/spaces/{space_id}/proposals/{proposal_id}/respond"
        ))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({"action": "accept"}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "accept failed: {} {}", resp.status(), resp.text().await.unwrap_or_default());
    let accepted: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(accepted["status"], "accepted");
}

/// End-to-end negotiation lifecycle test.
///
/// Validates the full coordination flow:
/// 1. Register user → 2 agents (consumer + provider)
/// 2. Create space → both join → status transitions to active
/// 3. Exchange messages
/// 4. Submit proposal → respond
/// 5. Write shared state (terms) → verify versioning
/// 6. Concurrent state write → verify optimistic lock conflict
/// 7. Close space (concluded) → verify terminal state
/// 8. Attempt illegal state transition → verify rejection
/// 9. Verify trace audit log
#[tokio::test]
async fn test_full_negotiation_lifecycle() {
    let (base_url, _server) = spawn_test_server().await;
    let client = reqwest::Client::new();

    // ── Step 1: Register user + two agents ──────────────
    let user_key = register_test_user(&client, &base_url, "e2e@gaggle.io").await;
    let (_consumer_id, consumer_key) =
        register_test_agent(&client, &base_url, &user_key, "BuyerBot", "consumer").await;
    let (provider_id, provider_key) =
        register_test_agent(&client, &base_url, &user_key, "SellerBot", "provider").await;

    // ── Step 2: Create space + both join ────────────────
    let resp = client
        .post(format!("{base_url}/api/v1/spaces"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({
            "name": "Supply Chain Negotiation",
            "invitee_ids": [provider_id],
            "context": {"product": "Steel Coils", "quantity": 500, "unit": "tons"}
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "create space: {}", resp.status());
    let space: serde_json::Value = resp.json().await.unwrap();
    let space_id = space["id"].as_str().unwrap().to_string();
    assert_eq!(space["status"], "created");
    assert_eq!(space["version"], 1);

    // Provider joins → triggers created→active transition
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/join"))
        .bearer_auth(&provider_key)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "join: {}", resp.status());
    let joined: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(joined["status"], "active");
    assert_eq!(joined["version"], 2);

    // ── Step 3: Exchange messages ──────────────────────
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/send"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({"content": "We need 500 tons of steel coils, delivery within 30 days"}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/send"))
        .bearer_auth(&provider_key)
        .json(&serde_json::json!({"content": "We can supply at $850/ton, FOB Shanghai"}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // ── Step 4: Submit + respond to proposal ────────────
    // Provider (seller) submits a proposal
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/proposals/submit"))
        .bearer_auth(&provider_key)
        .json(&serde_json::json!({
            "proposal_type": "initial",
            "dimensions": {"price_per_ton": 850, "delivery_days": 30, "payment_terms": "net30"}
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "submit proposal: {} {}", resp.status(), resp.text().await.unwrap_or_default());
    let proposal: serde_json::Value = resp.json().await.unwrap();
    let proposal_id = proposal["id"].as_str().unwrap();

    // Consumer (creator/buyer) accepts the provider's proposal
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/proposals/{proposal_id}/respond"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({"action": "accept"}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "accept proposal: {} {}", resp.status(), resp.text().await.unwrap_or_default());

    // ── Step 5: Write shared state (negotiated terms) ──
    // First write: no expected_version (initial state for this space)
    let resp = client
        .put(format!("{base_url}/api/v1/spaces/{space_id}/state/agreed_terms"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({
            "value": {"price_per_ton": 820, "delivery_days": 25, "payment": "net30"}
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "set state: {} {}", resp.status(), resp.text().await.unwrap_or_default());
    let state_result: serde_json::Value = resp.json().await.unwrap();
    let state_version = state_result["new_version"].as_u64().expect(&format!("expected 'new_version' in response, got: {state_result}"));
    assert!(state_version > 0, "state version should be positive after first write");

    // Read back shared state → verify
    let resp = client
        .get(format!("{base_url}/api/v1/spaces/{space_id}/state"))
        .bearer_auth(&consumer_key)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let snapshot: serde_json::Value = resp.json().await.unwrap();
    assert!(snapshot["version"].as_u64().unwrap() > 0, "state version should be positive");
    let entries = snapshot["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "should have at least one state entry");
    let agreed = entries.iter().find(|e| e["key"] == "agreed_terms").expect("agreed_terms should exist");
    assert_eq!(agreed["value"]["price_per_ton"], 820);

    // ── Step 6: Concurrent state write → conflict ──────
    // Both agents try to write the SAME key at the same per-key version
    // First: read the per-key version of "agreed_terms"
    let agreed_entry = snapshot["entries"].as_array().unwrap()
        .iter().find(|e| e["key"] == "agreed_terms").expect("agreed_terms should exist");
    let agreed_key_version = agreed_entry["version"].as_u64().unwrap();

    // First write with correct per-key version succeeds
    let resp1 = client
        .put(format!("{base_url}/api/v1/spaces/{space_id}/state/agreed_terms"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({
            "value": {"price_per_ton": 830, "delivery_days": 25, "payment": "net30"},
            "expected_version": agreed_key_version
        }))
        .send()
        .await
        .unwrap();
    assert!(resp1.status().is_success(), "first concurrent write should succeed: {} {}", resp1.status(), resp1.text().await.unwrap_or_default());

    // Second write with same (now stale) per-key version → conflict
    let resp2 = client
        .put(format!("{base_url}/api/v1/spaces/{space_id}/state/agreed_terms"))
        .bearer_auth(&provider_key)
        .json(&serde_json::json!({
            "value": {"price_per_ton": 800, "delivery_days": 25, "payment": "net30"},
            "expected_version": agreed_key_version
        }))
        .send()
        .await
        .unwrap();
    // Should get a conflict error (version bumped by first write)
    assert!(!resp2.status().is_success(), "second concurrent write should fail (version conflict)");

    // ── Step 7: Close space (concluded) ─────────────────
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/close"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({
            "conclusion": "concluded",
            "final_terms": {"price_per_ton": 820}
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "close space: {} {}", resp.status(), resp.text().await.unwrap_or_default());
    let closed: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(closed["status"], "concluded");
    assert!(closed["closed_at"].is_number(), "closed_at should be set");

    // ── Step 8: Illegal state transition ────────────────
    // Try to close an already concluded space → should fail
    let resp = client
        .post(format!("{base_url}/api/v1/spaces/{space_id}/close"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({"conclusion": "cancelled"}))
        .send()
        .await
        .unwrap();
    assert!(!resp.status().is_success(), "should reject closing a concluded space");
    let err: serde_json::Value = resp.json().await.unwrap_or_default();
    assert!(err["error"]["message"].as_str().unwrap_or("").contains("terminal"));

    // Try to write state on a concluded space → should fail
    let resp = client
        .put(format!("{base_url}/api/v1/spaces/{space_id}/state/post_close"))
        .bearer_auth(&consumer_key)
        .json(&serde_json::json!({"value": "should_fail"}))
        .send()
        .await
        .unwrap();
    // State writes on concluded space should be rejected
    assert!(!resp.status().is_success(), "should reject state write on concluded space");

    // ── Step 9: Verify trace audit log ──────────────────
    let resp = client
        .get(format!("{base_url}/api/v1/spaces/{space_id}/trace"))
        .bearer_auth(&consumer_key)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let trace: serde_json::Value = resp.json().await.unwrap();
    let entries = trace["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "trace should have entries");

    // Verify key audit events exist across the full lifecycle
    let actions: Vec<&str> = entries.iter()
        .filter_map(|e| e["action"].as_str())
        .collect();
    assert!(actions.contains(&"space_created"), "trace should contain space_created");
    assert!(actions.contains(&"space_joined"), "trace should contain space_joined");
    assert!(actions.contains(&"message_sent"), "trace should contain message_sent");
    assert!(actions.contains(&"proposal_submitted"), "trace should contain proposal_submitted");
    assert!(actions.contains(&"proposal_responded"), "trace should contain proposal_responded");
    assert!(actions.contains(&"state_set"), "trace should contain state_set");
    assert!(actions.contains(&"space_closed"), "trace should contain space_closed");
}
