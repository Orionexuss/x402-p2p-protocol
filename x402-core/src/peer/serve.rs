use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::{fs, io};

use crate::decode_torrent;
use crate::peer::auth_proof::AuthProof;
use crate::peer::extension_protocol::ExtendedHandshake;
use crate::peer::protocol::{X402MessageId, read_message};
use crate::peer::tracker_client::{AnnounceResponse, TrackerClient, TrackerClientError};
use crate::torrent::parser::calculate_info_hash;
use svix_ksuid::{KsuidLike, KsuidMs};

use crate::peer::handshake::{Handshake, generate_peer_id};

#[derive(Debug)]
struct TorrentManager {
    torrents: HashMap<[u8; 20], TorrentWithMetadata>,
}

#[derive(Debug)]
struct TorrentWithMetadata {
    metadata: Vec<u8>,
}

impl TorrentManager {
    pub fn load_torrents() -> Self {
        let mut torrents = HashMap::new();

        // Read all files in the ./torrents directory
        if let Ok(entries) = fs::read_dir("./torrents") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(data) = fs::read(&path)
                    && let Ok(torrent) = decode_torrent(&data)
                {
                    let info_hash = calculate_info_hash(&torrent);
                    let metadata = torrent.get_torrent_metadata();
                    let torrent_with_metadata = TorrentWithMetadata { metadata };
                    torrents.insert(info_hash, torrent_with_metadata);
                }
            }
        }

        TorrentManager { torrents }
    }

    pub fn get_torrent(&self, info_hash: &[u8; 20]) -> Option<&TorrentWithMetadata> {
        self.torrents.get(info_hash)
    }
}

pub struct Seeder {
    /// The address to bind to
    address: String,
    /// The port to listen on
    port: u16,
    /// Our peer ID
    peer_id: KsuidMs,
    /// Info hashes we're serving
    pub info_hashes: Vec<[u8; 20]>,
}

impl Seeder {
    pub fn new(address: String, port: u16) -> Self {
        Seeder {
            address,
            port,
            peer_id: generate_peer_id(),
            info_hashes: Vec::new(),
        }
    }

    /// Add an info hash that this seeder can serve
    pub fn add_torrent(&mut self, info_hash: [u8; 20]) {
        self.info_hashes.push(info_hash);
    }

    pub async fn announce_to_tracker(
        &self,
        tracker_url: String,
        info_hash: [u8; 20],
    ) -> Result<AnnounceResponse, TrackerClientError> {
        let tracker_client = TrackerClient::new(tracker_url);
        let peer_id = self.peer_id.bytes();
        let port = self.port;
        let pubkey = [0u8; 32]; // TODO: Use real wallet pubkey
        let left = 0u64; // Seeder has all pieces
        let event = Some("completed");

        tracker_client
            .announce(&info_hash, peer_id, port, &pubkey, left, event)
            .await
    }

    /// Add an info hash from hex string
    pub fn add_torrent_hex(&mut self, info_hash_hex: &str) -> Result<(), String> {
        if info_hash_hex.len() != 40 {
            return Err(format!(
                "Invalid info hash length: expected 40, got {}",
                info_hash_hex.len()
            ));
        }

        let mut info_hash = [0u8; 20];
        for i in 0..20 {
            info_hash[i] = u8::from_str_radix(&info_hash_hex[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("Invalid hex: {}", e))?;
        }

        self.add_torrent(info_hash);
        Ok(())
    }

    /// Start listening for incoming connections
    pub fn listen(&self) -> io::Result<()> {
        let torrent_manager = TorrentManager::load_torrents();

        let addr = format!("{}:{}", self.address, self.port);
        let listener = TcpListener::bind(&addr)?;
        println!("Seeder listening on {}", addr);
        println!("Peer ID: {}", hex::encode(self.peer_id.bytes()));

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    println!();
                    println!("New connection from: {}", stream.peer_addr()?);
                    if let Err(e) = self.handle_connection(stream, &torrent_manager) {
                        eprintln!("Error handling connection: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Connection failed: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Handle an incoming peer connection
    fn handle_connection(
        &self,
        mut stream: TcpStream,
        torrent_manager: &TorrentManager,
    ) -> Result<(), String> {
        println!("Waiting for handshake...");

        // Receive the handshake from the leecher
        let handshake = Handshake::receive(&mut stream)?;

        println!("Received handshake:");
        println!("  Info Hash: {}", handshake.info_hash_hex());
        println!("  Peer ID: {}", hex::encode(handshake.peer_id.bytes()));

        // Check if we have this torrent
        if !self.info_hashes.contains(&handshake.info_hash) {
            return Err(format!(
                "We don't have torrent with info hash: {}",
                handshake.info_hash_hex()
            ));
        }

        println!("Info hash matches! Sending handshake response...");

        // Send our handshake response
        let response = Handshake::new(handshake.info_hash, self.peer_id);
        response
            .send(&mut stream)
            .map_err(|e| format!("Failed to send handshake: {}", e))?;

        println!("Handshake successful!");
        // ID that the peer expects us to use when sending ut_metadata
        let mut peer_ut_metadata: Option<u8> = None;

        // To avoid sending our handshake more than once
        let mut sent_extended_handshake = false;
        // Route incoming protocol messages by ID so order is not assumed.
        loop {
            let message = read_message(&mut stream)
                .map_err(|e| format!("Failed to read protocol message: {}", e))?;

            match message.id {
                // Extension channel (BEP 10 style metadata and custom messages)
                X402MessageId::Extended => {
                    match message.extended_message_id {
                        Some(0) => {
                            println!("Received extended handshake");

                            let peer_handshake = ExtendedHandshake::receive_extended_handshake(
                                &message,
                            )
                            .map_err(|e| format!("Failed to process extended handshake: {}", e))?;

                            // Save if the peer supports ut_metadata
                            peer_ut_metadata = peer_handshake.m.get("ut_metadata").copied();

                            if let Some(id) = peer_ut_metadata {
                                println!("Peer supports ut_metadata with id {}", id);
                            } else {
                                println!("Peer does not support ut_metadata");
                            }

                            // Send our handshake only once
                            if !sent_extended_handshake
                                && let Some(torrent) =
                                    torrent_manager.get_torrent(&handshake.info_hash)
                            {
                                let mut my_handshake = ExtendedHandshake::new();

                                let metadata = torrent.metadata.clone();
                                my_handshake.metadata_size = Some(metadata.len() as u64);

                                my_handshake.send_extended_handshake(&mut stream);

                                sent_extended_handshake = true;
                            }
                        }

                        // ----- OTHER EXTENDED MESSAGES -----
                        Some(ext_id) => {
                            if Some(ext_id) == peer_ut_metadata {
                                println!("Received ut_metadata message");

                                // TODO: parse ut_metadata message and handle accordingly
                                // - request
                                // - data
                                // - reject
                            } else {
                                println!("Received unknown extended message id {}", ext_id);
                            }
                        }

                        // ----- MALFORMED MESSAGE -----
                        None => {
                            println!("Invalid extended message (missing extended id)");
                        }
                    }
                }
                X402MessageId::AuthProof => {
                    let auth_proof = AuthProof::receive(&message)?;

                    if auth_proof.verify() {
                        println!("Peer authenticated successfully!");
                        auth_proof
                            .send_auth_ok(&mut stream)
                            .map_err(|e| format!("Failed to send AuthOk: {}", e))?;
                        // Keep connection alive for the rest of the protocol flow.
                        return Ok(());
                    }

                    return Err("Peer authentication failed".to_string());
                }

                X402MessageId::InquirePrice => {
                    println!("Received InquirePrice (TODO: build and send PriceOffer)");
                }

                X402MessageId::PriceOffer => {
                    println!("Received PriceOffer (unexpected for seeder; TODO: validate/ignore)");
                }

                X402MessageId::LockedPayment => {
                    println!("Received LockedPayment (TODO: verify lock/amount)");
                }

                X402MessageId::RequestBlock => {
                    println!("Received RequestBlock (TODO: send PieceChunk)");
                }

                X402MessageId::PieceChunk => {
                    println!("Received PieceChunk (unexpected for seeder; TODO: validate/ignore)");
                }

                X402MessageId::PaymentReveal => {
                    println!("Received PaymentReveal (TODO: verify and send PaymentAck)");
                }

                X402MessageId::PaymentAck => {
                    println!("Received PaymentAck (unexpected for seeder; TODO: validate/ignore)");
                }

                X402MessageId::Have => {
                    println!("Received Have (TODO: process peer piece availability)");
                }

                // AuthOk is typically sent by seeder after AuthProof.
                X402MessageId::AuthOk => {
                    println!("Received AuthOk (unexpected for seeder; TODO: validate/ignore)");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seeder_new() {
        let seeder = Seeder::new("127.0.0.1".to_string(), 6881);
        assert_eq!(seeder.address, "127.0.0.1");
        assert_eq!(seeder.port, 6881);
        assert_eq!(seeder.info_hashes.len(), 0);
    }

    #[test]
    fn test_add_torrent() {
        let mut seeder = Seeder::new("127.0.0.1".to_string(), 6881);
        let info_hash = [1u8; 20];
        seeder.add_torrent(info_hash);
        assert_eq!(seeder.info_hashes.len(), 1);
        assert_eq!(seeder.info_hashes[0], info_hash);
    }

    #[test]
    fn test_add_torrent_hex() {
        let mut seeder = Seeder::new("127.0.0.1".to_string(), 6881);
        let hex = "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36";

        let result = seeder.add_torrent_hex(hex);
        assert!(result.is_ok());
        assert_eq!(seeder.info_hashes.len(), 1);
    }

    #[test]
    fn test_add_torrent_hex_invalid() {
        let mut seeder = Seeder::new("127.0.0.1".to_string(), 6881);
        let result = seeder.add_torrent_hex("invalid");
        assert!(result.is_err());
    }
}
