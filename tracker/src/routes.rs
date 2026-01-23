use crate::error::TrackerError;
use crate::tracker::X402Tracker;
use crate::types::*;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

pub async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "x402 tracker is running")
}

pub async fn announce_handler(
    State(tracker): State<Arc<X402Tracker>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<AnnounceRequest>,
) -> Result<Json<AnnounceResponse>, TrackerError> {
    info!("Announce from {}", addr.ip());
    let resp = tracker.handle_announce(req, addr.ip()).await?;
    Ok(Json(resp))
}

#[derive(Deserialize)]
pub struct DiscoverQuery {
    info_hash: String,
}

pub async fn discover_handler(
    State(tracker): State<Arc<X402Tracker>>,
    Query(params): Query<DiscoverQuery>,
) -> Result<Json<AnnounceResponse>, TrackerError> {
    info!("Discover for info_hash: {}", params.info_hash);
    let resp = tracker.handle_discover(&params.info_hash).await?;
    Ok(Json(resp))
}

pub async fn report_handler(
    State(tracker): State<Arc<X402Tracker>>,
    Json(req): Json<ReportRequest>,
) -> Result<StatusCode, TrackerError> {
    tracker.handle_report(req).await?;
    Ok(StatusCode::OK)
}

pub async fn stats_handler(
    State(tracker): State<Arc<X402Tracker>>,
) -> Json<StatsResponse> {
    Json(tracker.get_stats().await)
}
