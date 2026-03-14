use crate::peer::auth_proof::AuthProof;
use crate::peer::extension_protocol::ExtendedHandshake;
use crate::peer::handshake::{generate_peer_id, Handshake};
use crate::peer::protocol::X402MessageId;
use crate::peer::tracker_client::{PeerInfo, TrackerClient};
use crate::peer::ut_metadata::{calculate_num_pieces, MetadataMessage};
use crate::read_message;
use dirs::home_dir;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::{EncodableKey, Signer};
use std::io::{self, Read};
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
    ExtendedHandshakeFailed(String),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    #[error("Authentication failed")]
    AuthError(),
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
    pub async fn download(&self, is_magnet: bool) -> Result<(), LeecherError> {
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
            match self.connect_to_peer(peer_info, is_magnet).await {
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

    async fn connect_to_peer(
        &self,
        peer_info: &PeerInfo,
        is_magnet: bool,
    ) -> Result<(), LeecherError> {
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

        // implement BEP 10
        if is_magnet {
            ExtendedHandshake::new().send_extended_handshake(&mut stream);
            const MAX_METADATA_SIZE: u32 = 4 * 1024 * 1024;

            let mut peer_ut_metadata_id: Option<u8> = None;
            let metadata_size: u32;

            // Wait for extended handshake response
            loop {
                let message = read_message(&mut stream).unwrap();

                if message.id == X402MessageId::Extended && message.extended_message_id == Some(0) {
                    let peer_handshake =
                        ExtendedHandshake::receive_extended_handshake(&message).unwrap();

                    peer_ut_metadata_id = peer_handshake.m.get("ut_metadata").copied();
                    metadata_size = peer_handshake.metadata_size.unwrap() as u32;

                    if let Some(id) = peer_ut_metadata_id {
                        println!("Peer supports ut_metadata with id {}", id);
                    } else {
                        panic!("Peer does not support ut_metadata");
                    }

                    if metadata_size > MAX_METADATA_SIZE {
                        panic!("metadata too large");
                    }
                    break;
                }
            }

            let ut_metadata_id = peer_ut_metadata_id.unwrap();
            let calculated_pieces = calculate_num_pieces(metadata_size);

            // Request pieces in a pipelined manner (e.g. 8 pieces at a time)
            let pipeline = 8.min(calculated_pieces);

            for piece in 0..pipeline {
                MetadataMessage::send_ut_metadata_request(&mut stream, ut_metadata_id, piece)
                    .unwrap();
                println!("Requested metadata piece {}", piece);
            }
            /* let mut next_piece = pipeline;
             loop {
                let msg = read_message(&mut stream)?;

                if let Some(piece_index) = parse_metadata_piece(&msg) {
                    save_piece(piece_index);

                    if next_piece < calculated_pieces {
                        send_request(next_piece);
                        next_piece += 1;
                    }
                }

                if all_pieces_downloaded() {
                    break;
                }
            } */
        }

        // path ~/.config/solana/id.json
        let keypair_path = home_dir()
            .ok_or_else(|| {
                LeecherError::IoError(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Home directory not found",
                ))
            })?
            .join(".config")
            .join("solana")
            .join("id.json");

        let keypair = Keypair::read_from_file(&keypair_path).unwrap();

        let auth_proof = AuthProof::create(&keypair, AuthProof::generate_nonce());
        auth_proof.send(&mut stream)?;
        println!("Sent authentication proof to peer.");

        let auth_response = read_message(&mut stream).unwrap().id;

        if auth_response == X402MessageId::AuthOk {
            println!("Peer authenticated successfully!");
        } else {
            println!("Peer authentication failed!");
            return Err(LeecherError::AuthError());
        }

        Ok(())
    }
}
