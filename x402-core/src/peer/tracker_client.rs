use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TrackerClientError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Tracker error: {0}")]
    TrackerError(String),
}

/// Announce request to tracker
#[derive(Debug, Clone, Serialize)]
pub struct AnnounceRequest {
    pub info_hash: String,
    pub price: u64,
    pub peer_id: String,
    pub port: u16,
    pub pubkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub pieces: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
}

/// Peer info from tracker
#[derive(Debug, Clone, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub ip: String,
    pub port: u16,
    pub pubkey: String,
    pub stake: u64,
    pub reputation: i32,
    pub pieces: Vec<u8>,
}

/// Announce response from tracker
#[derive(Debug, Clone, Deserialize)]
pub struct AnnounceResponse {
    pub interval: u32,
    pub min_stake: u64,
    pub piece_price: u64,
    pub seeders: Vec<PeerInfo>,
    pub leechers: Vec<PeerInfo>,
    pub complete: usize,
    pub incomplete: usize,
}

pub struct TrackerClient {
    tracker_url: String,
    client: reqwest::Client,
}

impl TrackerClient {
    pub fn new(tracker_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();

        Self {
            tracker_url,
            client,
        }
    }

    pub async fn announce(
        &self,
        info_hash: &[u8; 20],
        price: u64,
        peer_id: &[u8; 20],
        port: u16,
        pubkey: &[u8; 32],
        left: u64,
        event: Option<&str>,
    ) -> Result<AnnounceResponse, TrackerClientError> {
        let req = AnnounceRequest {
            info_hash: hex::encode(info_hash),
            price,
            peer_id: hex::encode(peer_id),
            port,
            pubkey: hex::encode(pubkey),
            signature: None, // TODO: Add signature in Phase 2
            uploaded: 0,
            downloaded: 0,
            left,
            pieces: vec![],
            event: event.map(|s| s.to_string()),
        };

        let url = format!("{}/announce", self.tracker_url);
        let resp = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| TrackerClientError::RequestFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let error = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(TrackerClientError::TrackerError(error));
        }

        let announce_resp = resp
            .json::<AnnounceResponse>()
            .await
            .map_err(|e| TrackerClientError::InvalidResponse(e.to_string()))?;

        Ok(announce_resp)
    }

    pub async fn discover(
        &self,
        info_hash: &[u8; 20],
    ) -> Result<AnnounceResponse, TrackerClientError> {
        let url = format!(
            "{}/discover?info_hash={}",
            self.tracker_url,
            hex::encode(info_hash)
        );

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| TrackerClientError::RequestFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let error = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(TrackerClientError::TrackerError(error));
        }

        let announce_resp = resp
            .json::<AnnounceResponse>()
            .await
            .map_err(|e| TrackerClientError::InvalidResponse(e.to_string()))?;

        Ok(announce_resp)
    }

    pub async fn report_peer(
        &self,
        reporter: &[u8; 20],
        reported: &[u8; 20],
        info_hash: &[u8; 20],
        reason: &str,
    ) -> Result<(), TrackerClientError> {
        let req = serde_json::json!({
            "reporter": hex::encode(reporter),
            "reported": hex::encode(reported),
            "info_hash": hex::encode(info_hash),
            "reason": reason,
        });

        let url = format!("{}/report", self.tracker_url);
        let resp = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| TrackerClientError::RequestFailed(e.to_string()))?;

        if !resp.status().is_success() {
            let error = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(TrackerClientError::TrackerError(error));
        }

        Ok(())
    }
}
