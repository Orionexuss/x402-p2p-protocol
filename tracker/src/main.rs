mod error;
mod routes;
mod tracker;
mod types;

use crate::tracker::X402Tracker;
use crate::types::EconomicPolicy;
use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Create tracker with default policy
    let policy = EconomicPolicy::default();
    let tracker = Arc::new(X402Tracker::new(policy));

    // Spawn cleanup task
    let tracker_clone = Arc::clone(&tracker);
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(300)); // Every 5 minutes
        loop {
            interval.tick().await;
            tracing::debug!("Running peer cleanup...");
            tracker_clone.cleanup_stale_peers().await;
        }
    });

    // Build router
    let app = Router::new()
        .route("/", get(routes::health_check))
        .route("/health", get(routes::health_check))
        .route("/announce", post(routes::announce_handler))
        .route("/discover", get(routes::discover_handler))
        .route("/report", post(routes::report_handler))
        .route("/stats", get(routes::stats_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(tracker);

    // Get listen address from environment or use default
    let listen_addr = std::env::var("TRACKER_LISTEN").unwrap_or_else(|_| "0.0.0.0:6969".to_string());

    // Start server
    let listener = tokio::net::TcpListener::bind(&listen_addr).await.unwrap();

    tracing::info!("x402 tracker listening on {}", listen_addr);
    tracing::info!("Endpoints:");
    tracing::info!("  POST /announce  - Register/update peer");
    tracing::info!("  GET  /discover  - Query peers without registering");
    tracing::info!("  POST /report    - Report misbehaving peer");
    tracing::info!("  GET  /stats     - Get tracker statistics");
    tracing::info!("  GET  /health    - Health check");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
