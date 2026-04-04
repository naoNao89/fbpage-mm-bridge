use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use webhook_gateway::{config::Config, create_app, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(&config.log_level))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Webhook Gateway...");
    info!("Configuration: {:?}", config);

    let state = AppState {
        config: config.clone(),
    };

    let app = create_app(state);

    let addr: SocketAddr = config.bind_address.parse()?;
    info!("Webhook Gateway listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
