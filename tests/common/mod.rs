//! Test helpers for integration tests.
//!
//! Provides `spawn_test_server()` which starts an in-memory Gaggle server
//! on a random port and returns the base URL.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::RwLock;

use gaggle::agents::AgentRegistry;
use gaggle::api::event_queue::EventQueue;
use gaggle::api::health::SERVER_START;
use gaggle::api::rest::AppState;
use gaggle::api::routes::create_router;
use gaggle::discovery::DiscoveryStore;
use gaggle::execution::ExecutionStore;
use gaggle::marketplace::MarketplaceStore;
use gaggle::negotiation::SpaceManager;
use gaggle::reputation::ReputationStore;
use gaggle::users::UserStore;

/// Spawn a test server with in-memory SQLite databases.
///
/// Returns the base URL (e.g. "http://127.0.0.1:12345") and a shutdown handle.
/// The server runs in a background tokio task and will be cleaned up when
/// the shutdown handle is dropped.
pub async fn spawn_test_server() -> (String, tokio::task::JoinHandle<()>) {
    // Initialize server start time for /health endpoint
    let _ = SERVER_START.set(chrono::Utc::now().timestamp());

    // Initialize all stores with in-memory SQLite
    let registry = Arc::new(AgentRegistry::new(":memory:").unwrap());
    let user_store = Arc::new(UserStore::new(":memory:").unwrap());
    let space_manager = Arc::new(SpaceManager::new(":memory:").unwrap());
    let discovery_store = Arc::new(DiscoveryStore::new(":memory:").unwrap());
    let reputation_store = Arc::new(ReputationStore::new(":memory:").unwrap());
    let execution_store = Arc::new(ExecutionStore::new(":memory:").unwrap());
    let marketplace_store = Arc::new(MarketplaceStore::new(":memory:").unwrap());
    let event_queue = Arc::new(EventQueue::new(":memory:").unwrap());

    let state = AppState {
        registry,
        space_manager,
        user_store,
        discovery_store,
        reputation_store,
        execution_store,
        marketplace_store,
        online_agents: Arc::new(RwLock::new(HashMap::new())),
        event_queue,
    };

    // Create router with high rate limit for tests
    let app = create_router(state, 10_000);

    // Bind to random port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    // Spawn server in background
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    // Give server a moment to start accepting connections
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    (base_url, handle)
}
