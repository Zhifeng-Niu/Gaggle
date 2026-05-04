//! Integration tests for Gaggle REST API.
//!
//! Each test spawns an in-memory server and exercises real HTTP flows.

mod common;

use common::spawn_test_server;

/// Helper: register an agent and return (agent_id, api_key).
async fn register_test_agent(
    client: &reqwest::Client,
    base_url: &str,
    name: &str,
    agent_type: &str,
) -> (String, String) {
    let resp = client
        .post(format!("{base_url}/api/v1/agents/register"))
        .bearer_auth("test_key")
        .json(&serde_json::json!({
            "agent_type": agent_type,
            "name": name,
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "register failed: {}", resp.status());
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
    space_name: &str,
) -> (String, String, String) {
    let (_consumer_id, consumer_key) =
        register_test_agent(client, base_url, "Consumer", "consumer").await;
    let (provider_id, provider_key) =
        register_test_agent(client, base_url, "Provider", "provider").await;

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

    // Register
    let (agent_id, api_key) = register_test_agent(&client, &base_url, "TestAgent", "consumer").await;
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

    let (_consumer_id, consumer_key) =
        register_test_agent(&client, &base_url, "Consumer", "consumer").await;
    let (provider_id, provider_key) =
        register_test_agent(&client, &base_url, "Provider", "provider").await;

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

    let (space_id, consumer_key, provider_key) =
        setup_space_with_members(&client, &base_url, "Chat").await;

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

    let (space_id, consumer_key, provider_key) =
        setup_space_with_members(&client, &base_url, "DealAccept").await;

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
