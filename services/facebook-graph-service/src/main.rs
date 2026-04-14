use facebook_graph_service::{config::Config, create_app, db, run_migrations, AppState};
use std::net::SocketAddr;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(&config.log_level))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Facebook Graph Service...");
    info!("Configuration: {:?}", config);

    let pool = db::create_pool(&config.database_url).await?;
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

    let state = AppState {
        pool: pool.clone(),
        config: config.clone(),
        customer_client,
        message_client,
        mattermost_client,
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

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
