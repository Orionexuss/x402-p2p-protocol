use crate::peer::handshake::{generate_peer_id, Handshake};
use crate::peer::tracker_client::{PeerInfo, TrackerClient};
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;
use svix_ksuid::{KsuidLike, KsuidMs};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LeecherError {
    #[error("Tracker error: {0}")]
    TrackerError(String),

    #[error("No peers available")]
    NoPeersAvailable,

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
}

pub struct Leecher {
    peer_id: KsuidMs,
    info_hash: [u8; 20],
    pubkey: [u8; 32],
    tracker_url: String,
    output_path: PathBuf,
    total_size: u64,
}

impl Leecher {
    pub fn new(
        info_hash: [u8; 20],
        tracker_url: String,
        output_path: PathBuf,
        total_size: u64,
    ) -> Self {
        let peer_id = generate_peer_id();
        let pubkey = [0u8; 32]; // TODO: Use real wallet pubkey

        Self {
            peer_id,
            info_hash,
            pubkey,
            tracker_url,
            output_path,
            total_size,
        }
    }

    /// Start the download process
    pub async fn download(&self) -> Result<(), LeecherError> {
        println!(
            "Starting download for info_hash: {}",
            hex::encode(self.info_hash)
        );
        println!("Peer ID: {}", hex::encode(self.peer_id.bytes()));
        println!("Output: {}", self.output_path.display());
        println!();

        // 1. Contact tracker to get peer list
        println!("Contacting tracker at {}...", self.tracker_url);
        let tracker_client = TrackerClient::new(self.tracker_url.clone());

        let response = tracker_client
            .announce(
                &self.info_hash,
                self.peer_id.bytes(),
                6881, // Our listening port
                &self.pubkey,
                self.total_size,
                Some("started"),
            )
            .await
            .map_err(|e| LeecherError::TrackerError(e.to_string()))?;

        println!(
            "Tracker response: {} seeders, {} leechers",
            response.complete, response.incomplete
        );
        println!("Min stake: {} lamports", response.min_stake);
        println!("Piece price: {} lamports", response.piece_price);
        println!();

        // 2. Get all available peers (seeders first)
        let mut peers = response.seeders;
        peers.extend(response.leechers);

        if peers.is_empty() {
            return Err(LeecherError::NoPeersAvailable);
        }

        println!("Found {} peers:", peers.len());
        for peer in &peers {
            println!(
                "  - {}:{} (stake: {}, reputation: {})",
                peer.ip, peer.port, peer.stake, peer.reputation
            );
        }
        println!();

        // 3. Try to connect to peers
        for peer_info in &peers {
            match self.connect_to_peer(peer_info).await {
                Ok(_) => {
                    println!(
                        "Successfully connected to peer {}:{}",
                        peer_info.ip, peer_info.port
                    );
                    // TODO: Request pieces and download data
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "Failed to connect to peer {}:{}: {}",
                        peer_info.ip, peer_info.port, e
                    );
                    continue;
                }
            }
        }

        // 4. Announce completion to tracker
        tracker_client
            .announce(
                &self.info_hash,
                self.peer_id.bytes(),
                6881,
                &self.pubkey,
                0, // left = 0 means we're done
                Some("completed"),
            )
            .await
            .map_err(|e| LeecherError::TrackerError(e.to_string()))?;

        println!("\nDownload completed! (Protocol implementation in progress)");

        Ok(())
    }

    async fn connect_to_peer(&self, peer_info: &PeerInfo) -> Result<(), LeecherError> {
        println!(
            "\nConnecting to peer {}:{}...",
            peer_info.ip, peer_info.port
        );

        // Parse peer address
        let addr: SocketAddr = format!("{}:{}", peer_info.ip, peer_info.port)
            .parse()
            .map_err(|e| LeecherError::ConnectionFailed(format!("Invalid address: {}", e)))?;

        // Connect with timeout
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
            .map_err(|e| LeecherError::ConnectionFailed(e.to_string()))?;

        println!("Connected! Performing handshake...");

        // Perform BitTorrent handshake
        let handshake = Handshake::exchange(&mut stream, self.info_hash, self.peer_id)
            .map_err(LeecherError::HandshakeFailed)?;

        println!("Handshake successful!");
        println!("  Peer ID: {}", hex::encode(handshake.peer_id.bytes()));
        println!("  Info Hash: {}", handshake.info_hash_hex());

        // TODO: Perform X402 handshake
        // TODO: Request price for pieces
        // TODO: Lock payment
        // TODO: Request and download pieces
        // TODO: Verify pieces
        // TODO: Reveal payment

        Ok(())
    }
}
