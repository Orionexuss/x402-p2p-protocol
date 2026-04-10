use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::{TcpListener, TcpStream};
use std::{fs, io};

use crate::decode_torrent;
use crate::peer::auth_proof::AuthProof;
use crate::peer::extension_protocol::{ExtendedHandshake, UT_METADATA_EXTENSION_ID};
use crate::peer::leech::LeecherError;
use crate::peer::protocol::{X402MessageId, read_message};
use crate::peer::tracker_client::{AnnounceResponse, TrackerClient, TrackerClientError};
use crate::peer::ut_metadata::{MetadataMessage, calculate_num_pieces};
use crate::torrent::parser::calculate_info_hash;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::solana_sdk::signer::EncodableKey;
use anchor_client::solana_sdk::signer::Signer;
use anchor_lang::prelude::Pubkey;
use dirs::home_dir;
use svix_ksuid::{KsuidLike, KsuidMs};

use crate::peer::handshake::{Handshake, generate_peer_id};

#[derive(Debug)]
pub struct TorrentManager {
    torrents: HashMap<[u8; 20], TorrentWithMetadata>,
}

#[derive(Debug)]
pub struct TorrentWithMetadata {
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

    /// info_hashes of the torrents we are seeding
    pub info_hashes: Vec<[u8; 20]>,

    /// Announced price per info hash (same value sent to tracker).
    prices_by_info_hash: HashMap<[u8; 20], u64>,
}

impl Seeder {
    const MAX_METADATA_REQUEST_QUEUE: usize = 8;
    const MAX_METADATA_RESPONSES_PER_TICK: usize = 2;

    pub fn new(address: String, port: u16) -> Option<(Self, TorrentManager)> {
        // get info hashes of all torrents in the ./torrents directory
        let torrent_manager = TorrentManager::load_torrents();
        let info_hashes = torrent_manager
            .torrents
            .keys()
            .copied()
            .collect::<Vec<[u8; 20]>>();

        Some((
            Self {
                address,
                port,
                peer_id: generate_peer_id(),
                info_hashes,
                prices_by_info_hash: HashMap::new(),
            },
            torrent_manager,
        ))
    }

    pub async fn announce_to_tracker(
        &mut self,
        tracker_url: String,
        price: u64,
        info_hash: [u8; 20],
        seeder_pubkey: &Pubkey,
    ) -> Result<AnnounceResponse, TrackerClientError> {


        let tracker_client = TrackerClient::new(tracker_url);
        let peer_id = self.peer_id.bytes();
        let port = self.port;
        let pubkey = seeder_pubkey.to_bytes();
        let left = 0u64; // Seeder has all pieces
        let event = Some("completed");

        let response = tracker_client
            .announce(&info_hash, price, peer_id, port, &pubkey, left, event)
            .await?;

        self.prices_by_info_hash.insert(info_hash, price);

        Ok(response)
    }

    /// Start listening for incoming connections
    pub fn listen(&self, torrent_manager: &TorrentManager, seeder_pubkey: &Pubkey) -> io::Result<()> {
        let addr = format!("{}:{}", self.address, self.port);
        let listener = TcpListener::bind(&addr)?;
        println!("Seeder listening on {}", addr);
        println!("Peer ID: {}", hex::encode(self.peer_id.bytes()));

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    println!();
                    println!("New connection from: {}", stream.peer_addr()?);
                    if let Err(e) = self.handle_connection(stream, torrent_manager, seeder_pubkey) {
                        eprintln!("Connection handler internal error: {}", e);
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
        seeder_pubkey: &Pubkey,
    ) -> Result<(), String> {
        println!("Waiting for handshake...");

        // Receive the handshake from the leecher
        let handshake = match Handshake::receive(&mut stream) {
            Ok(handshake) => handshake,
            Err(_) => {
                println!("Closing peer connection after invalid handshake");
                return Ok(());
            }
        };

        println!("Received handshake:");
        println!("  Info Hash: {}", handshake.info_hash_hex());
        println!("  Peer ID: {}", hex::encode(handshake.peer_id.bytes()));

        // Check if we have this torrent
        if torrent_manager.get_torrent(&handshake.info_hash).is_none() {
            println!("Closing peer connection: torrent not available on this seeder");
            return Ok(());
        }

        println!("Info hash matches! Sending handshake response...");

        let seeder_price = self
            .prices_by_info_hash
            .get(&handshake.info_hash)
            .copied()
            .unwrap_or(0);

        // Send our handshake response
        let response = Handshake::new(handshake.info_hash, self.peer_id, seeder_price);
        if response.send(&mut stream).is_err() {
            println!("Closing peer connection after handshake send failure");
            return Ok(());
        }

        println!("Handshake successful!");

        let metadata = torrent_manager
            .get_torrent(&handshake.info_hash)
            .ok_or_else(|| {
                format!(
                    "Missing torrent metadata for info hash: {}",
                    handshake.info_hash_hex()
                )
            })?
            .metadata
            .clone();
        let metadata_size = metadata.len() as u32;
        let metadata_piece_count = calculate_num_pieces(metadata_size);

        // ID that the peer expects us to use when sending ut_metadata
        let mut peer_ut_metadata_id: Option<u8> = None;
        let mut metadata_request_queue: VecDeque<u32> = VecDeque::new();
        let mut pop_from_back = false;

        // To avoid sending our handshake more than once
        let mut sent_extended_handshake = false;
        // Route incoming protocol messages by ID so order is not assumed.
        loop {
            let message = match read_message(&mut stream) {
                Ok(message) => message,
                Err(_) => {
                    println!("Closing peer connection after protocol read error");
                    break;
                }
            };

            match message.id {
                // Extension channel (BEP 10 style metadata and custom messages)
                X402MessageId::Extended => {
                    match message.extended_message_id {
                        Some(0) => {
                            println!("Received extended handshake");

                            let peer_handshake = match ExtendedHandshake::receive_extended_handshake(
                                &message,
                            ) {
                                Ok(peer_handshake) => peer_handshake,
                                Err(_) => {
                                    println!(
                                        "Closing peer connection after malformed extended handshake"
                                    );
                                    break;
                                }
                            };

                            // Save if the peer supports ut_metadata
                            peer_ut_metadata_id = peer_handshake.m.get("ut_metadata").copied();

                            if let Some(id) = peer_ut_metadata_id {
                                println!("Peer supports ut_metadata with id {}", id);
                            } else {
                                println!("Peer does not support ut_metadata");
                            }

                            // Send our handshake only once
                            if !sent_extended_handshake {
                                let mut my_handshake = ExtendedHandshake::new();
                                my_handshake.metadata_size = Some(metadata_size as u64);

                                if my_handshake.send_extended_handshake(&mut stream).is_err() {
                                    println!(
                                        "Closing peer connection after extended handshake send failure"
                                    );
                                    break;
                                }
                                println!(
                                    "Sent extended handshake with metadata size {:?}",
                                    my_handshake
                                );

                                sent_extended_handshake = true;
                            }
                        }

                        Some(ext_id) => {
                            if ext_id == UT_METADATA_EXTENSION_ID {
                                println!("Received ut_metadata message with id {}", ext_id);
                                let requested_piece =
                                    match MetadataMessage::receive_ut_metadata_request(&message) {
                                        Some(requested_piece) => requested_piece,
                                        None => {
                                            println!(
                                                "Closing peer connection after invalid ut_metadata request"
                                            );
                                            break;
                                        }
                                    };

                                let response_ext_id = match peer_ut_metadata_id {
                                    Some(id) => id,
                                    None => {
                                        println!(
                                            "Closing peer connection: ut_metadata was not negotiated"
                                        );
                                        break;
                                    }
                                };

                                if requested_piece >= metadata_piece_count {
                                    println!(
                                        "Closing peer connection after out-of-range metadata request"
                                    );
                                    break;
                                }

                                if metadata_request_queue.len() >= Self::MAX_METADATA_REQUEST_QUEUE
                                {
                                    println!(
                                        "Closing peer connection after metadata queue overflow"
                                    );
                                    break;
                                } else {
                                    metadata_request_queue.push_back(requested_piece);
                                    println!(
                                        "Queued metadata piece {} (queue: {})",
                                        requested_piece,
                                        metadata_request_queue.len()
                                    );
                                }

                                if MetadataMessage::flush_metadata_queue(
                                    &mut stream,
                                    response_ext_id,
                                    &metadata,
                                    &mut metadata_request_queue,
                                    &mut pop_from_back,
                                    Self::MAX_METADATA_RESPONSES_PER_TICK,
                                )
                                .is_err()
                                {
                                    println!(
                                        "Closing peer connection after metadata response error"
                                    );
                                    break;
                                }
                            } else {
                                println!("Closing peer connection after unknown extended message");
                                break;
                            }
                        }

                        // ----- MALFORMED MESSAGE -----
                        None => {
                            println!("Closing peer connection after malformed extended message");
                            break;
                        }
                    }
                }
                X402MessageId::AuthProof => {
                    let auth_proof = match AuthProof::receive(&message) {
                        Ok(auth_proof) => auth_proof,
                        Err(_) => {
                            println!("Closing peer connection after invalid AuthProof");
                            break;
                        }
                    };

                    if auth_proof.verify() {
                        println!("Peer authenticated successfully!");
                        if auth_proof.send_auth_ok(&mut stream).is_err() {
                            println!("Closing peer connection after AuthOk send failure");
                            break;
                        }
                        // Keep connection alive for the rest of the protocol flow.
                        continue;
                    }

                    println!("Closing peer connection after failed authentication");
                    break;
                }

                X402MessageId::LockedPayment => {
                    println!("Received LockedPayment (TODO: verify on-chain and conditionally ack)")
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seeder_new() {
        let (seeder, _) = Seeder::new("127.0.0.1".to_string(), 6881).unwrap();
        assert_eq!(seeder.address, "127.0.0.1");
        assert_eq!(seeder.port, 6881);
    }
}
