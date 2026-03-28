use crate::peer::auth_proof::AuthProof;
use crate::peer::extension_protocol::ExtendedHandshake;
use crate::peer::handshake::{generate_peer_id, Handshake};
use crate::peer::protocol::X402MessageId;
use crate::peer::tracker_client::{PeerInfo, TrackerClient};
use crate::peer::ut_metadata::{calculate_num_pieces, MetadataMessage, METADATA_PIECE_SIZE};
use crate::read_message;
use sha1::Digest;
use solana_sdk::signature::Keypair;
use std::collections::HashSet;
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
    ExtendedHandshakeFailed(String),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    #[error("Authentication failed")]
    AuthError(),

    #[error("Metadata protocol error: {0}")]
    MetadataError(String),
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
        pubkey: [u8; 32],
        output_path: PathBuf,
        total_size: u64,
    ) -> Self {
        let peer_id = generate_peer_id();

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
    pub async fn download(&self, is_magnet: bool, keypair: &Keypair) -> Result<(), LeecherError> {
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
                0,
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
            match self.connect_to_peer(peer_info, is_magnet, keypair).await {
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
                0,
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
        keypair: &Keypair,
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
        let handshake = Handshake::exchange(&mut stream, self.info_hash, self.peer_id, 0)
            .map_err(LeecherError::HandshakeFailed)?;

        println!("Handshake successful!");
        println!("  Peer ID: {}", hex::encode(handshake.peer_id.bytes()));
        println!("  Info Hash: {}", handshake.info_hash_hex());
        println!("  Seeder Price: {}", handshake.price);

        // implement BEP 10
        if is_magnet {
            ExtendedHandshake::new().send_extended_handshake(&mut stream);
            const MAX_METADATA_SIZE: u32 = 4 * 1024 * 1024; // 4 MB

            // Wait for extended handshake response
            let (peer_ut_metadata_id, metadata_size) = loop {
                let message = read_message(&mut stream).unwrap();

                if message.id == X402MessageId::Extended && message.extended_message_id == Some(0) {
                    let peer_handshake =
                        ExtendedHandshake::receive_extended_handshake(&message).unwrap();

                    let peer_ut_metadata_id = peer_handshake.m.get("ut_metadata").copied();
                    let metadata_size = peer_handshake.metadata_size.unwrap() as u32;

                    if peer_ut_metadata_id.is_none() {
                        panic!("Peer does not support ut_metadata");
                    }

                    if metadata_size > MAX_METADATA_SIZE {
                        panic!("metadata too large");
                    }
                    break (peer_ut_metadata_id, metadata_size);
                }
            };

            let peer_ut_metadata_id = peer_ut_metadata_id.unwrap();
            let calculated_pieces = calculate_num_pieces(metadata_size);

            // Request pieces in a pipelined manner (e.g. 8 pieces at a time)
            let pipeline = 8.min(calculated_pieces);

            let mut in_flight: HashSet<u32> = HashSet::new();
            let mut received_count = 0u32;
            let mut pieces = vec![None::<Vec<u8>>; calculated_pieces as usize];

            for piece in 0..pipeline {
                MetadataMessage::send_ut_metadata_request(&mut stream, peer_ut_metadata_id, piece)
                    .map_err(|e| LeecherError::MetadataError(e.to_string()))?;
                in_flight.insert(piece);
            }

            let mut next_piece = pipeline;

            while received_count < calculated_pieces {
                let msg = read_message(&mut stream)
                    .map_err(|e| LeecherError::MetadataError(e.to_string()))?;

                if MetadataMessage::is_ut_metadata_reject(&msg, peer_ut_metadata_id) {
                    return Err(LeecherError::MetadataError(
                        "Seeder rejected a metadata request".to_string(),
                    ));
                }

                let Some((piece, data_block)) =
                    MetadataMessage::receive_ut_metadata_data(&msg, peer_ut_metadata_id)
                else {
                    // Ignore unrelated protocol messages while metadata is in progress.
                    continue;
                };

                if piece >= calculated_pieces {
                    return Err(LeecherError::MetadataError(format!(
                        "Received out-of-range metadata piece {} (expected < {})",
                        piece, calculated_pieces
                    )));
                }

                // The last piece may be smaller than METADATA_PIECE_SIZE, so calculate the expected size for this piece.
                let expected_len = if piece == calculated_pieces - 1 {
                    let remainder = (metadata_size as usize) % (METADATA_PIECE_SIZE as usize);
                    if remainder == 0 {
                        // If remainder is 0, it means the last piece is exactly METADATA_PIECE_SIZE.
                        METADATA_PIECE_SIZE as usize
                    } else {
                        remainder
                    }
                } else {
                    METADATA_PIECE_SIZE as usize
                };

                if data_block.len() != expected_len {
                    return Err(LeecherError::MetadataError(format!(
                        "Piece {} has invalid size {} (expected {})",
                        piece,
                        data_block.len(),
                        expected_len
                    )));
                }

                in_flight.remove(&piece);

                if pieces[piece as usize].is_none() {
                    pieces[piece as usize] = Some(data_block);
                    received_count += 1;
                }

                while in_flight.len() < pipeline as usize && next_piece < calculated_pieces {
                    MetadataMessage::send_ut_metadata_request(
                        &mut stream,
                        peer_ut_metadata_id,
                        next_piece,
                    )
                    .map_err(|e| LeecherError::MetadataError(e.to_string()))?;
                    in_flight.insert(next_piece);
                    next_piece += 1;
                }
            }

            let mut metadata = Vec::with_capacity(metadata_size as usize);
            for (idx, piece) in pieces.into_iter().enumerate() {
                let piece = piece.ok_or_else(|| {
                    LeecherError::MetadataError(format!("Missing metadata piece {}", idx))
                })?;
                metadata.extend_from_slice(&piece);
            }

            if metadata.len() != metadata_size as usize {
                return Err(LeecherError::MetadataError(format!(
                    "Reconstructed metadata size mismatch: got {}, expected {}",
                    metadata.len(),
                    metadata_size
                )));
            }

            println!("Metadata download complete ({} bytes)", metadata.len());

            let calculated_info_hash = sha1::Sha1::digest(&metadata);

            if calculated_info_hash.as_slice() != self.info_hash {
                return Err(LeecherError::MetadataError(format!(
                    "Info hash mismatch: calculated {}, expected {}",
                    hex::encode(calculated_info_hash),
                    hex::encode(self.info_hash)
                )));
            }
        }

        let auth_proof = AuthProof::create(keypair, AuthProof::generate_nonce());
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
