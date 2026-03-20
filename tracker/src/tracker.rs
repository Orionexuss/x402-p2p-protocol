use crate::error::TrackerError;
use crate::types::*;
use bitvec::prelude::*;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

pub struct X402Tracker {
    swarms: Arc<RwLock<HashMap<InfoHash, Swarm>>>,
    reputation: Arc<RwLock<HashMap<PeerId, i32>>>,
    policy: EconomicPolicy,
}

impl X402Tracker {
    pub fn new(policy: EconomicPolicy) -> Self {
        Self {
            swarms: Arc::new(RwLock::new(HashMap::new())),
            reputation: Arc::new(RwLock::new(HashMap::new())),
            policy,
        }
    }

    pub async fn handle_announce(
        &self,
        req: AnnounceRequest,
        ip: IpAddr,
    ) -> Result<AnnounceResponse, TrackerError> {
        // Parse info hash
        let info_hash =
            hex::decode(&req.info_hash).map_err(|e| TrackerError::HexDecode(e.to_string()))?;
        if info_hash.len() != 20 {
            return Err(TrackerError::InvalidInfoHash(
                "Info hash must be 20 bytes".to_string(),
            ));
        }
        let info_hash: InfoHash = info_hash.try_into().unwrap();

        // Parse peer ID
        let peer_id =
            hex::decode(&req.peer_id).map_err(|e| TrackerError::HexDecode(e.to_string()))?;
        if peer_id.len() != 20 {
            return Err(TrackerError::InvalidPeerId(
                "Peer ID must be 20 bytes".to_string(),
            ));
        }
        let peer_id: PeerId = peer_id.try_into().unwrap();

        // Parse pubkey
        let pubkey =
            hex::decode(&req.pubkey).map_err(|e| TrackerError::HexDecode(e.to_string()))?;
        if pubkey.len() != 32 {
            return Err(TrackerError::InvalidPubkey(
                "Public key must be 32 bytes".to_string(),
            ));
        }
        let pubkey: Pubkey = pubkey.try_into().unwrap();

        // Convert pieces bitfield to BitVec
        let pieces = BitVec::<u8, Msb0>::from_vec(req.pieces);

        info!(
            "Announce from peer {} for info_hash {} (event: {:?})",
            hex::encode(&peer_id),
            hex::encode(&info_hash),
            req.event
        );

        // Handle stopped event
        if req.event == Some(AnnounceEvent::Stopped) {
            self.remove_peer(&info_hash, &peer_id).await;
            return Ok(AnnounceResponse {
                interval: 1800,
                min_stake: self.policy.min_stake,
                piece_price: 1000,
                seeders: vec![],
                leechers: vec![],
                complete: 0,
                incomplete: 0,
            });
        }

        // Phase 1: Skip stake verification (will implement in Phase 2)
        let stake = 0; // TODO: Query Solana in Phase 2

        // Check economic requirements (lenient in Phase 1)
        let reputation = self.get_reputation(&peer_id).await;
        if reputation < self.policy.min_reputation {
            return Err(TrackerError::LowReputation(reputation));
        }

        // Get or create swarm
        let mut swarms = self.swarms.write().await;
        let swarm = swarms
            .entry(info_hash)
            .or_insert_with(|| Swarm::new(info_hash));

        // Create peer entry
        let peer = PeerEntry {
            peer_id,
            ip,
            port: req.port,
            pubkey,
            price: req.price,
            stake,
            reputation,
            last_seen: SystemTime::now(),
            pieces,
            uploaded: req.uploaded,
            downloaded: req.downloaded,
            left: req.left,
        };

        // Add to appropriate list
        let peer_key = (ip, req.port);
        if req.left == 0 {
            debug!("Adding peer as seeder");
            swarm.leechers.remove(&peer_key);
            swarm.seeders.insert(peer_key, peer);
        } else {
            debug!("Adding peer as leecher");
            swarm.seeders.remove(&peer_key);
            swarm.leechers.insert(peer_key, peer);
        }

        // Get peer lists
        let seeders = self.format_peers(&swarm.seeders);
        let leechers = self.format_peers(&swarm.leechers);

        let complete = swarm.seeders.len();
        let incomplete = swarm.leechers.len();

        info!(
            "Swarm {} now has {} seeders and {} leechers",
            hex::encode(&info_hash),
            complete,
            incomplete
        );

        Ok(AnnounceResponse {
            interval: 1800, // 30 minutes
            min_stake: swarm.min_stake,
            piece_price: swarm.piece_price,
            seeders,
            leechers,
            complete,
            incomplete,
        })
    }

    pub async fn handle_discover(
        &self,
        info_hash_hex: &str,
    ) -> Result<AnnounceResponse, TrackerError> {
        let info_hash =
            hex::decode(info_hash_hex).map_err(|e| TrackerError::HexDecode(e.to_string()))?;
        if info_hash.len() != 20 {
            return Err(TrackerError::InvalidInfoHash(
                "Info hash must be 20 bytes".to_string(),
            ));
        }
        let info_hash: InfoHash = info_hash.try_into().unwrap();

        let swarms = self.swarms.read().await;
        let swarm = swarms
            .get(&info_hash)
            .ok_or_else(|| TrackerError::InvalidInfoHash("Swarm not found".to_string()))?;

        let seeders = self.format_peers(&swarm.seeders);
        let leechers = self.format_peers(&swarm.leechers);

        Ok(AnnounceResponse {
            interval: 1800,
            min_stake: swarm.min_stake,
            piece_price: swarm.piece_price,
            seeders,
            leechers,
            complete: swarm.seeders.len(),
            incomplete: swarm.leechers.len(),
        })
    }

    pub async fn handle_report(&self, req: ReportRequest) -> Result<(), TrackerError> {
        let reported =
            hex::decode(&req.reported).map_err(|e| TrackerError::HexDecode(e.to_string()))?;
        if reported.len() != 20 {
            return Err(TrackerError::InvalidPeerId(
                "Peer ID must be 20 bytes".to_string(),
            ));
        }
        let reported: PeerId = reported.try_into().unwrap();

        warn!(
            "Report received for peer {} (reason: {:?})",
            hex::encode(&reported),
            req.reason
        );

        // Apply reputation penalty
        let mut reputation = self.reputation.write().await;
        let current = reputation.entry(reported).or_insert(0);
        *current -= 10; // Penalty

        info!(
            "Peer {} reputation now: {}",
            hex::encode(&reported),
            *current
        );

        Ok(())
    }

    pub async fn get_stats(&self) -> StatsResponse {
        let swarms = self.swarms.read().await;

        let total_swarms = swarms.len();
        let mut total_peers = 0;
        let mut total_seeders = 0;
        let mut total_leechers = 0;

        for swarm in swarms.values() {
            total_seeders += swarm.seeders.len();
            total_leechers += swarm.leechers.len();
            total_peers += swarm.total_peers();
        }

        StatsResponse {
            total_swarms,
            total_peers,
            total_seeders,
            total_leechers,
        }
    }

    pub async fn cleanup_stale_peers(&self) {
        let mut swarms = self.swarms.write().await;
        let stale_threshold = Duration::from_secs(3600); // 1 hour

        for swarm in swarms.values_mut() {
            let now = SystemTime::now();

            swarm.seeders.retain(|_, peer| {
                now.duration_since(peer.last_seen).unwrap_or_default() < stale_threshold
            });

            swarm.leechers.retain(|_, peer| {
                now.duration_since(peer.last_seen).unwrap_or_default() < stale_threshold
            });
        }

        // Remove empty swarms
        swarms.retain(|_, swarm| swarm.total_peers() > 0);
    }

    async fn remove_peer(&self, info_hash: &InfoHash, peer_id: &PeerId) {
        let mut swarms = self.swarms.write().await;
        if let Some(swarm) = swarms.get_mut(info_hash) {
            // Find and remove peer by peer_id (since we receive peer_id in the request)
            swarm.seeders.retain(|_, peer| &peer.peer_id != peer_id);
            swarm.leechers.retain(|_, peer| &peer.peer_id != peer_id);

            if swarm.total_peers() == 0 {
                swarms.remove(info_hash);
            }
        }
    }

    async fn get_reputation(&self, peer_id: &PeerId) -> i32 {
        let reputation = self.reputation.read().await;
        *reputation.get(peer_id).unwrap_or(&0)
    }

    fn format_peers(&self, peers: &HashMap<(IpAddr, u16), PeerEntry>) -> Vec<PeerInfo> {
        peers
            .values()
            .filter(|p| p.reputation >= self.policy.min_reputation)
            .map(|p| PeerInfo {
                peer_id: hex::encode(&p.peer_id),
                ip: p.ip.to_string(),
                port: p.port,
                pubkey: hex::encode(&p.pubkey),
                price: p.price,
                stake: p.stake,
                reputation: p.reputation,
                pieces: p.pieces.clone().into_vec(),
            })
            .collect()
    }
}
