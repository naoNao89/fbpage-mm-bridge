use facebook_graph_service::config::BypassMode;
use facebook_graph_service::services::{MattermostDbClient, MattermostOps};
use facebook_graph_service::{config::Config, create_app, db, run_migrations, AppState};
use std::net::SocketAddr;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut config = Config::from_env()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(&config.log_level))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Facebook Graph Service...");
    info!("Configuration: {:?}", config);

    let pool = db::create_pool(&config.database_url, config.database_max_connections).await?;
    run_migrations(&pool).await?;

    let customer_client =
        facebook_graph_service::services::CustomerServiceClient::new(&config.customer_service_url);
    let message_client =
        facebook_graph_service::services::MessageServiceClient::new(&config.message_service_url);
    let mattermost_client = facebook_graph_service::services::MattermostClient::new(
        &config.mattermost_url,
        &config.mattermost_username,
        config.mattermost_password.as_deref(),
    )
    .with_db_pool(pool.clone())
    .await;

    let mattermost_db = match &config.mattermost_database_url {
        Some(db_url) => {
            match MattermostDbClient::new(db_url, config.mattermost_database_max_connections).await
            {
                Ok(client) => {
                    info!("Mattermost DB client initialized (direct DB access enabled)");
                    Some(client)
                }
                Err(e) => {
                    warn!("Failed to initialize Mattermost DB client: {}. Direct DB operations will be disabled.", e);
                    None
                }
            }
        }
        None => {
            info!("MATTERMOST_DATABASE_URL not set - direct DB access disabled");
            None
        }
    };

    if let Some(db_client) = &mattermost_db {
        match db_client.schema_version().await {
            Ok(Some(version)) => {
                info!("Mattermost server-version probe: {version}");
                if !version.starts_with("10.")
                    && config.mattermost_bypass_mode == BypassMode::Enabled
                {
                    warn!(
                        "Mattermost DB bypass requested as enabled, but schema version {version} is not release-10 compatible; downgrading to shadow"
                    );
                    config.mattermost_bypass_mode = BypassMode::Shadow;
                }
            }
            Ok(None) => {
                warn!("Mattermost server-version probe returned no version");
                if config.mattermost_bypass_mode == BypassMode::Enabled {
                    warn!("Mattermost DB bypass requested as enabled with unknown schema version; downgrading to shadow");
                    config.mattermost_bypass_mode = BypassMode::Shadow;
                }
            }
            Err(e) => {
                warn!("Mattermost server-version probe failed: {e}");
                if config.mattermost_bypass_mode == BypassMode::Enabled {
                    warn!("Mattermost DB bypass requested as enabled but schema probe failed; downgrading to shadow");
                    config.mattermost_bypass_mode = BypassMode::Shadow;
                }
            }
        }
    }

    let minio = match facebook_graph_service::storage::MinioStorage::new(
        &config.minio_endpoint,
        &config.minio_access_key,
        &config.minio_secret_key,
        &config.minio_bucket,
        std::time::Duration::from_secs(config.minio_presigned_ttl_secs),
    )
    .await
    {
        Ok(storage) => {
            info!("MinIO storage initialized successfully");
            Some(storage)
        }
        Err(e) => {
            warn!("MinIO storage initialization failed (media storage disabled): {e}");
            None
        }
    };

    let conversation_id_cache = {
        let mut cache = std::collections::HashMap::new();
        match db::load_mm_cache(&pool, "conversation").await {
            Ok(entries) => {
                cache.extend(entries);
                info!(
                    "Loaded {} conversation_id cache entries from database",
                    cache.len()
                );
            }
            Err(e) => warn!("Failed to load conversation_id cache from database: {e}"),
        }
        std::sync::Arc::new(tokio::sync::RwLock::new(cache))
    };

    let mattermost_ops = MattermostOps::new(
        pool.clone(),
        mattermost_client.clone(),
        mattermost_db.clone(),
        config.mattermost_bypass_mode,
    );

    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        customer_client,
        message_client,
        mattermost_client,
        mattermost_db,
        mattermost_ops,
        minio,
        conversation_id_cache,
    };

    let app = create_app(state.clone());

    let addr: SocketAddr = config.bind_address.parse()?;
    info!("Facebook Graph Service listening on {}", addr);

    if config.poll_interval_secs > 0 {
        let poll_state = state.clone();
        let poll_interval = config.poll_interval_secs;
        tokio::spawn(async move {
            facebook_graph_service::poll::run_poller(poll_state, poll_interval).await;
        });
        info!(
            "Real-time poller started (interval: {}s)",
            config.poll_interval_secs
        );
    }

    {
        let worker_state = state.clone();
        tokio::spawn(async move {
            facebook_graph_service::media_worker::run_media_worker(worker_state, 60).await;
        });
        info!("Media download worker started (interval: 60s)");
    }

    {
        let sync_state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                info!("Starting hourly sync of all conversations...");
                match facebook_graph_service::handlers::sync_all_conversations_sync(&sync_state)
                    .await
                {
                    Ok(result) => {
                        info!(
                            "Hourly sync completed: {} fetched, {} posted, {} skipped",
                            result.messages_fetched,
                            result.messages_posted,
                            result.messages_skipped
                        );
                    }
                    Err(e) => {
                        tracing::error!("Hourly sync failed: {}", e);
                    }
                }
            }
        });
        info!("Hourly sync scheduler started");
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
