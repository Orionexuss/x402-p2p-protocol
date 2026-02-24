use bitvec::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::SystemTime;

pub type InfoHash = [u8; 20];
pub type PeerId = [u8; 20];
pub type Pubkey = [u8; 32];

/// Peer registry entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEntry {
    pub peer_id: PeerId,
    pub ip: IpAddr,
    pub port: u16,
    pub pubkey: Pubkey,
    pub stake: u64,
    pub reputation: i32,
    pub last_seen: SystemTime,
    #[serde(skip)]
    pub pieces: BitVec<u8, Msb0>,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
}

/// Torrent swarm state
#[derive(Debug, Clone)]
pub struct Swarm {
    pub info_hash: InfoHash,
    pub seeders: HashMap<(IpAddr, u16), PeerEntry>,
    pub leechers: HashMap<(IpAddr, u16), PeerEntry>,
    pub min_stake: u64,
    pub piece_price: u64,
    pub created_at: SystemTime,
}

impl Swarm {
    pub fn new(info_hash: InfoHash) -> Self {
        Self {
            info_hash,
            seeders: HashMap::new(),
            leechers: HashMap::new(),
            min_stake: 0, // Default: no minimum stake (Phase 1)
            piece_price: 1000, // Default: 1000 lamports per piece
            created_at: SystemTime::now(),
        }
    }

    pub fn total_peers(&self) -> usize {
        self.seeders.len() + self.leechers.len()
    }
}

/// Economic policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicPolicy {
    pub min_stake: u64,
    pub min_reputation: i32,
    pub penalty_threshold: i32,
}

impl Default for EconomicPolicy {
    fn default() -> Self {
        Self {
            min_stake: 0, // Phase 1: no stake required
            min_reputation: -100,
            penalty_threshold: -50,
        }
    }
}

/// Announce event type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AnnounceEvent {
    Started,
    Completed,
    Stopped,
    Update,
}

/// Announce request from peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnounceRequest {
    pub info_hash: String, // hex encoded
    pub peer_id: String,   // hex encoded
    pub port: u16,
    pub pubkey: String,    // hex encoded
    #[serde(default)]
    pub signature: String, // hex encoded (optional for Phase 1)
    #[serde(default)]
    pub uploaded: u64,
    #[serde(default)]
    pub downloaded: u64,
    pub left: u64,
    #[serde(default)]
    pub pieces: Vec<u8>, // bitfield
    #[serde(default)]
    pub event: Option<AnnounceEvent>,
}

/// Peer info in response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub ip: String,
    pub port: u16,
    pub pubkey: String,
    pub stake: u64,
    pub reputation: i32,
    pub pieces: Vec<u8>,
}

/// Announce response to peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnounceResponse {
    pub interval: u32,
    pub min_stake: u64,
    pub piece_price: u64,
    pub seeders: Vec<PeerInfo>,
    pub leechers: Vec<PeerInfo>,
    pub complete: usize,   // number of seeders
    pub incomplete: usize, // number of leechers
}

/// Report request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRequest {
    pub reporter: String,
    pub reported: String,
    pub info_hash: String,
    pub reason: ReportReason,
    #[serde(default)]
    pub proof: String,
    #[serde(default)]
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportReason {
    InvalidData,
    NoResponse,
    PaymentFraud,
}

/// Stats response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    pub total_swarms: usize,
    pub total_peers: usize,
    pub total_seeders: usize,
    pub total_leechers: usize,
}
