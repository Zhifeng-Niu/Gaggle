//! Gaggle - Agent-to-Agent 商业平台
//!
//! MVP实现：消费者Agent与服务商Agent的接入、Negotiation Space拉起、多轮谈判

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use gaggle::agents::AgentRegistry;
use gaggle::api::event_queue::EventQueue;
use gaggle::api::rest::AppState;
use gaggle::api::routes::create_router;
use gaggle::config::Config;
use gaggle::discovery::DiscoveryStore;
use gaggle::execution::ExecutionStore;
use gaggle::marketplace::MarketplaceStore;
use gaggle::negotiation::SpaceManager;
use gaggle::negotiation::SharedStateManager;
use gaggle::reputation::ReputationStore;
use gaggle::users::UserStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,gaggle=debug".to_string()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Gaggle Server...");

    // 记录服务器启动时间（供 /health 端点使用）
    let _ = gaggle::api::health::SERVER_START.set(chrono::Utc::now().timestamp());

    // 加载配置
    let config = Config::from_env();
    tracing::info!("Config loaded: {}:{}", config.host, config.port);

    // 初始化Agent注册表
    let registry = AgentRegistry::new(&config.database_path)?;
    let registry = Arc::new(registry);
    tracing::info!("Agent registry initialized at {}", config.database_path);

    // 初始化用户存储
    let user_store = UserStore::new(&config.database_path)?;
    let user_store = Arc::new(user_store);
    tracing::info!("User store initialized");

    // 初始化Space管理器
    let space_manager = SpaceManager::new(&config.database_path)?;
    let space_manager = Arc::new(space_manager);
    tracing::info!("Space manager initialized");

    // 初始化 Shared State Manager
    let shared_state_manager = SharedStateManager::new(&config.database_path)?;
    let shared_state_manager = Arc::new(shared_state_manager);
    tracing::info!("Shared state manager initialized");

    // 初始化 Discovery Store
    let discovery_store = DiscoveryStore::new(&config.database_path)?;
    let discovery_store = Arc::new(discovery_store);
    tracing::info!("Discovery store initialized");

    // 初始化 Reputation Store
    let reputation_store = ReputationStore::new(&config.database_path)?;
    let reputation_store = Arc::new(reputation_store);
    tracing::info!("Reputation store initialized");

    // 初始化 Execution Store
    let execution_store = ExecutionStore::new(&config.database_path)?;
    let execution_store = Arc::new(execution_store);
    tracing::info!("Execution store initialized");

    // 初始化 Marketplace Store
    let marketplace_store = MarketplaceStore::new(&config.database_path)?;
    let marketplace_store = Arc::new(marketplace_store);
    tracing::info!("Marketplace store initialized");

    // 初始化离线事件队列
    let event_queue = EventQueue::new(&config.database_path)?;
    let event_queue = Arc::new(event_queue);
    tracing::info!("Event queue initialized");

    // 初始化审计追踪
    let trace_store = gaggle::api::trace::TraceStore::new(&config.database_path)?;
    let trace_store = Arc::new(trace_store);
    tracing::info!("Trace store initialized");

    // 创建应用状态
    let state = AppState {
        registry,
        shared_state_manager,
        user_store,
        discovery_store,
        reputation_store,
        execution_store,
        marketplace_store,
        online_agents: Arc::new(RwLock::new(HashMap::new())),
        event_queue,
        trace_store,
        space_manager: space_manager.clone(),
    };

    // ── Space Lifecycle Governor + Event Retry Scheduler ──
    {
        let sm = space_manager;
        let eq = state.event_queue.clone();
        let online = state.online_agents.clone();
        tokio::spawn(async move {
            // Retry scheduler summary counters (logged every ~60s)
            static SUMMARY_CYCLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let mut summary_resets: u64 = 0;
            let mut summary_redeliveries: u64 = 0;
            let mut summary_dead_letters: u64 = 0;

            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;

                // 1. Event Retry Scheduler: find events past next_retry_at and redeliver
                match eq.get_retry_pending().await {
                    Ok(groups) if !groups.is_empty() => {
                        for (agent_id, events) in groups {
                            let is_online = {
                                let guard = online.read().await;
                                guard.contains_key(&agent_id)
                            };

                            if is_online {
                                for evt in &events {
                                    // Redeliver to all connections of this agent
                                    let replayed = serde_json::json!({
                                        "type": "replayed_event",
                                        "event_seq": evt.event_seq,
                                        "event_type": evt.event_type,
                                        "payload": serde_json::from_str::<serde_json::Value>(&evt.payload)
                                            .unwrap_or(serde_json::Value::Null),
                                    });
                                    let msg = replayed.to_string();

                                    let guard = online.read().await;
                                    if let Some(conns) = guard.get(&agent_id) {
                                        for conn in conns {
                                            let _ = conn.tx.send(msg.clone());
                                        }
                                    }
                                    drop(guard);

                                    // Mark retry attempt (may become dead letter)
                                    let still_alive = eq.mark_retry_attempt(evt.id).await.unwrap_or(false);
                                    if !still_alive {
                                        summary_dead_letters += 1;
                                        tracing::warn!(
                                            agent_id = %agent_id,
                                            event_seq = evt.event_seq,
                                            "RetryScheduler: event became dead letter after max retries"
                                        );
                                    } else {
                                        summary_redeliveries += 1;
                                    }
                                }
                            } else {
                                // Agent offline — reset retry timer without consuming retry_count.
                                summary_resets += events.len() as u64;
                                for evt in &events {
                                    let _ = eq.reset_retry_timer(evt.id).await;
                                }
                            }
                        }
                    }
                    Ok(_) => {} // no retries needed
                    Err(e) => {
                        tracing::warn!(error = %e, "RetryScheduler: failed to scan for retries");
                    }
                }

                // 2. Periodic summary + space expiry (every ~60s = 6 cycles)
                {
                    let cycle = SUMMARY_CYCLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if cycle % 6 == 0 {
                        // Log summary only if there was activity
                        if summary_resets > 0 || summary_redeliveries > 0 || summary_dead_letters > 0 {
                            tracing::info!(
                                resets = summary_resets,
                                redeliveries = summary_redeliveries,
                                dead_letters = summary_dead_letters,
                                "Governor: 60s summary"
                            );
                            summary_resets = 0;
                            summary_redeliveries = 0;
                            summary_dead_letters = 0;
                        }

                        match sm.find_expired_spaces().await {
                            Ok(expired) if !expired.is_empty() => {
                                tracing::info!(count = expired.len(), "Governor: processing expired spaces");
                                for (space_id, _agent_ids) in &expired {
                                    match sm.expire_space(space_id).await {
                                        Ok(_) => {
                                            if let Some(tx) = sm.get_broadcast_tx(space_id).await {
                                                let msg = serde_json::json!({
                                                    "type": "space_closed",
                                                    "space_id": space_id,
                                                    "payload": {
                                                        "conclusion": "expired",
                                                        "final_terms": null
                                                    }
                                                });
                                                let _ = tx.send(msg.to_string());
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(space_id = %space_id, error = %e, "Governor: failed to expire space");
                                        }
                                    }
                                }
                            }
                            Ok(_) => {}
                            Err(e) => {
                                tracing::warn!(error = %e, "Governor: failed to scan for expired spaces");
                            }
                        }

                        // Clean up delivered events older than 7 days
                        match eq.cleanup_delivered(7).await {
                            Ok(n) if n > 0 => tracing::info!(deleted = n, "Governor: cleaned up old events"),
                            _ => {}
                        }
                        // Clean up dead letter events older than 30 days
                        match eq.cleanup_dead_letters(30).await {
                            Ok(n) if n > 0 => tracing::info!(deleted = n, "Governor: cleaned up dead letters"),
                            _ => {}
                        }
                    }
                }
            }
        });
    }

    // 创建Router
    let app = create_router(state, config.rate_limit_rpm);

    // 启动服务器
    let addr = config.server_addr();
    tracing::info!("Gaggle server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;

    Ok(())
}
