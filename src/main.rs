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

    // 创建应用状态
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

    // 创建Router
    let app = create_router(state, config.rate_limit_rpm);

    // 启动服务器
    let addr = config.server_addr();
    tracing::info!("Gaggle server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;

    Ok(())
}
