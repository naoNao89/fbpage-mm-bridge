use facebook_graph_service::{config::Config, create_app, db, run_migrations, AppState};
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    let config = Config::from_env()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(&config.log_level))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Facebook Graph Service...");
    info!("Configuration: {:?}", config);

    // Create database connection pool
    info!("Connecting to database...");
    let pool = db::create_pool(&config.database_url).await?;

    // Run migrations
    info!("Running database migrations...");
    run_migrations(&pool).await?;

    // Create service clients
    let customer_client =
        facebook_graph_service::services::CustomerServiceClient::new(&config.customer_service_url);
    let message_client =
        facebook_graph_service::services::MessageServiceClient::new(&config.message_service_url);
    let mattermost_client = facebook_graph_service::services::MattermostClient::new(
        &config.mattermost_url,
        &config.mattermost_username,
        config.mattermost_password.as_deref(),
    );

    // Create application state
    let state = AppState {
        pool,
        config: config.clone(),
        customer_client,
        message_client,
        mattermost_client,
    };

    // Create application router
    let app = create_app(state);

    // Parse bind address
    let addr: SocketAddr = config.bind_address.parse()?;
    info!("Facebook Graph Service listening on {}", addr);

    // Start server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
