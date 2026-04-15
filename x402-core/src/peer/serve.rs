use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::{fs, io};

use crate::decode_torrent;
use crate::peer::auth_proof::AuthProof;
use crate::peer::extension_protocol::{ExtendedHandshake, UT_METADATA_EXTENSION_ID};
use crate::peer::locked_payment::LockedPayment;
use crate::peer::piece_exchange::{PieceExchange, PieceExchangeMsgKind, RequestPiece};
use crate::peer::protocol::{X402MessageId, read_message};
use crate::peer::utils::merkletree::verify_proof;
use crate::peer::tracker_client::{AnnounceResponse, TrackerClient, TrackerClientError};
use crate::peer::ut_metadata::{MetadataMessage, calculate_num_pieces};
use crate::torrent::parser::calculate_info_hash;
use anchor_lang::prelude::Pubkey;
use svix_ksuid::{KsuidLike, KsuidMs};

use crate::peer::handshake::{Handshake, generate_peer_id};

#[derive(Debug)]
pub struct TorrentManager {
    torrents: HashMap<[u8; 20], TorrentWithMetadata>,
    payloads: HashMap<[u8; 20], PayloadStore>,
}

#[derive(Debug)]
pub struct TorrentWithMetadata {
    metadata: Vec<u8>,
}

#[derive(Debug)]
pub struct PayloadStore {
    pieces: Vec<Vec<u8>>,
}

impl PayloadStore {
    fn read_piece_bytes(&self, piece_index: u32) -> Result<Vec<u8>, String> {
        self.pieces
            .get(piece_index as usize)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "Piece index {} out of range (available pieces: {})",
                    piece_index,
                    self.pieces.len()
                )
            })
    }
}

impl TorrentManager {
    fn data_dir() -> PathBuf {
        // x402-core lives at <repo>/x402-core, payloads are stored at <repo>/data.
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../data")
    }

    fn resolve_payload_path(data_dir: &Path, info_hash_hex: &str) -> Result<PathBuf, String> {
        let exact = data_dir.join(info_hash_hex);
        if exact.is_file() {
            return Ok(exact);
        }

        let mut matches: Vec<PathBuf> = Vec::new();
        let entries = fs::read_dir(data_dir)
            .map_err(|e| format!("Failed to read data dir {}: {}", data_dir.display(), e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            if name == info_hash_hex {
                return Ok(path);
            }

            if let Some(suffix) = name.strip_prefix(info_hash_hex)
                && suffix.starts_with('.')
                && suffix.len() > 1
            {
                matches.push(path);
            }
        }

        if matches.len() == 1 {
            return Ok(matches.remove(0));
        }

        if matches.is_empty() {
            return Err(format!(
                "No payload file found for info hash {} in {}",
                info_hash_hex,
                data_dir.display()
            ));
        }

        Err(format!(
            "Multiple payload files found for info hash {} in {}",
            info_hash_hex,
            data_dir.display()
        ))
    }

    pub fn load_torrents() -> Self {
        let mut torrents = HashMap::new();
        let mut payloads = HashMap::new();
        let data_dir = Self::data_dir();

        // Read all files in the ./torrents directory
        if let Ok(entries) = fs::read_dir("./torrents") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(data) = fs::read(&path)
                    && let Ok(torrent) = decode_torrent(&data)
                {
                    let info_hash = calculate_info_hash(&torrent);
                    let info_hash_hex = hex::encode(info_hash);
                    let payload_path = match Self::resolve_payload_path(&data_dir, &info_hash_hex) {
                        Ok(payload_path) => payload_path,
                        Err(e) => {
                            eprintln!("Skipping torrent {}: {}", path.display(), e);
                            continue;
                        }
                    };

                    let file_bytes = match fs::read(&payload_path) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            eprintln!(
                                "Skipping torrent {}: failed to read payload {} ({})",
                                path.display(),
                                payload_path.display(),
                                e
                            );
                            continue;
                        }
                    };

                    let piece_length = torrent.info.plength;
                    if piece_length == 0 {
                        eprintln!(
                            "Skipping torrent {}: invalid piece length 0",
                            path.display()
                        );
                        continue;
                    }

                    let declared_total_length = torrent.info.length.unwrap_or(file_bytes.len());
                    if file_bytes.len() < declared_total_length {
                        eprintln!(
                            "Skipping torrent {}: payload {} is smaller ({}) than torrent length ({})",
                            path.display(),
                            payload_path.display(),
                            file_bytes.len(),
                            declared_total_length
                        );
                        continue;
                    }

                    // Pre-split into piece-sized chunks once at startup.
                    let mut pieces = Vec::new();
                    let mut offset = 0usize;
                    while offset < declared_total_length {
                        let end = (offset + piece_length).min(declared_total_length);
                        pieces.push(file_bytes[offset..end].to_vec());
                        offset = end;
                    }

                    let metadata = torrent.get_torrent_metadata();
                    let torrent_with_metadata = TorrentWithMetadata { metadata };
                    let payload_store = PayloadStore { pieces };
                    torrents.insert(info_hash, torrent_with_metadata);
                    payloads.insert(info_hash, payload_store);
                }
            }
        }

        TorrentManager { torrents, payloads }
    }

    pub fn get_torrent(&self, info_hash: &[u8; 20]) -> Option<&TorrentWithMetadata> {
        self.torrents.get(info_hash)
    }

    pub fn read_piece_bytes(&self, info_hash: &[u8; 20], piece_index: u32) -> Result<Vec<u8>, String> {
        let payload = self
            .payloads
            .get(info_hash)
            .ok_or_else(|| format!("Payload store not found for info hash {}", hex::encode(info_hash)))?;

        payload.read_piece_bytes(piece_index)
    }

    pub fn piece_count(&self, info_hash: &[u8; 20]) -> Option<u32> {
        self.payloads
            .get(info_hash)
            .map(|payload| payload.pieces.len() as u32)
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

const PROTOCOL_TOTAL_STEPS: usize = 7;

fn protocol_stage(step: usize, label: &str, detail: &str) {
    println!("  [{}/{}] {:<14} {}", step, PROTOCOL_TOTAL_STEPS, label, detail);
}

fn protocol_step(step: usize, id: X402MessageId, direction: &str, detail: &str) {
    println!(
        "  [{}/{}] {:<14} ({:>2}) {} {}",
        step,
        PROTOCOL_TOTAL_STEPS,
        match id {
            X402MessageId::AuthProof => "AuthProof",
            X402MessageId::AuthOk => "AuthOk",
            X402MessageId::LockedPayment => "LockedPayment",
            X402MessageId::PaymentAck => "PaymentAck",
            X402MessageId::PieceExchange => "PieceExchange",
            X402MessageId::Extended => "Extended",
        },
        id.to_u8(),
        direction,
        detail
    );
}

fn print_piece_progress(prefix: &str, done: u32, total: u32) {
    let total = total.max(1);
    let width = 28usize;
    let filled = ((done as usize) * width) / (total as usize);
    let bar = format!("{}{}", "#".repeat(filled), "-".repeat(width - filled));
    let pct = (done as f64 / total as f64) * 100.0;
    print!("\r{} [{}] {}/{} ({:>5.1}%)", prefix, bar, done, total, pct);
    let _ = io::stdout().flush();
}

impl Seeder {
    const MAX_METADATA_REQUEST_QUEUE: usize = 8;
    const MAX_METADATA_RESPONSES_PER_TICK: usize = 2;
    const MAX_PIECE_REQUEST_QUEUE: usize = 32;
    const MAX_PIECE_RESPONSES_PER_TICK: usize = 4;

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
    pub fn listen(
        &self,
        torrent_manager: &TorrentManager,
        seeder_pubkey: &Pubkey,
    ) -> io::Result<()> {
        let addr = format!("{}:{}", self.address, self.port);
        let listener = TcpListener::bind(&addr)?;
        println!("Seeder listening on {}", addr);
        println!("Peer ID: {}", hex::encode(self.peer_id.bytes()));

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    println!("\n============================================================");
                    println!("Incoming connection from {}", stream.peer_addr()?);
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
        println!("Protocol timeline:");
        println!("  waiting for initial handshake...");

        // Receive the handshake from the leecher
        let handshake = match Handshake::receive(&mut stream) {
            Ok(handshake) => handshake,
            Err(_) => {
                println!("Closing peer connection after invalid handshake");
                return Ok(());
            }
        };

        protocol_stage(1, "Handshake", "received and validated");
        println!("    Info Hash: {}", handshake.info_hash_hex());
        println!("    Peer ID  : {}", hex::encode(handshake.peer_id.bytes()));

        // Check if we have this torrent
        if torrent_manager.get_torrent(&handshake.info_hash).is_none() {
            println!("Closing peer connection: torrent not available on this seeder");
            return Ok(());
        }

        println!("  torrent is available, sending handshake response...");

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

        println!("  handshake complete");

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
        let mut merkle_root: Option<[u8; 32]> = None;
        let mut leecher_pubkey: Option<Pubkey> = None;
        let mut piece_request_queue: VecDeque<RequestPiece> = VecDeque::new();
        let mut pop_piece_from_back = false;
        let mut committed_piece_hashes: HashMap<u32, [u8; 32]> = HashMap::new();
        let mut piece_keys: HashMap<u32, [u8; 32]> = HashMap::new();
        let mut highest_requested_piece: Option<u32> = None;
        let mut announced_piece_exchange = false;
        let mut announced_extended_step = false;

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
                            if !announced_extended_step {
                                protocol_step(2, X402MessageId::Extended, "<-", "received extended handshake");
                                announced_extended_step = true;
                            }

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
                                protocol_step(2, X402MessageId::Extended, "->", "sent extended handshake");

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
                    if !announced_extended_step {
                        protocol_step(2, X402MessageId::Extended, "--", "skipped (torrent metadata not requested)");
                        announced_extended_step = true;
                    }
                    protocol_step(3, X402MessageId::AuthProof, "<-", "received authentication proof");
                    let auth_proof = match AuthProof::receive(&message) {
                        Ok(auth_proof) => auth_proof,
                        Err(_) => {
                            println!("Closing peer connection after invalid AuthProof");
                            break;
                        }
                    };
                    leecher_pubkey = Some(auth_proof.pubkey);

                    if auth_proof.verify() {
                        if auth_proof.send_auth_ok(&mut stream).is_err() {
                            println!("Closing peer connection after AuthOk send failure");
                            break;
                        }
                        protocol_step(4, X402MessageId::AuthOk, "->", "sent auth ok");
                        // Keep connection alive for the rest of the protocol flow.
                        continue;
                    }

                    println!("Closing peer connection after failed authentication");
                    break;
                }

                X402MessageId::LockedPayment => {
                    if merkle_root.is_none() {
                        protocol_step(5, X402MessageId::LockedPayment, "<-", "received locked-payment root");
                        merkle_root = Some(
                            match LockedPayment::receive_locked_payment(
                                &message,
                                leecher_pubkey.as_ref().ok_or_else(|| {
                                    "Missing leecher pubkey for LockedPayment verification"
                                        .to_string()
                                })?,
                                seeder_pubkey,
                                &handshake.info_hash,
                                seeder_price,
                            ) {
                                Ok(root) => root,
                                Err(_) => {
                                    println!(
                                        "Closing peer connection after invalid LockedPayment message"
                                    );
                                    break;
                                }
                            },
                        );

                        LockedPayment::send_payment_ack(&mut stream);
                        protocol_step(6, X402MessageId::PaymentAck, "->", "sent payment acknowledgment");
                    }
                }

                X402MessageId::PieceExchange => {
                    if !announced_piece_exchange {
                        protocol_step(7, X402MessageId::PieceExchange, "..", "piece exchange started");
                        announced_piece_exchange = true;
                    }

                    if merkle_root.is_none() {
                        println!("Closing peer connection: PieceExchange before LockedPayment");
                        break;
                    }

                    let Some(kind) = PieceExchange::msg_kind(&message) else {
                        println!("Closing peer connection after malformed PieceExchange message");
                        break;
                    };

                    match kind {
                        PieceExchangeMsgKind::RequestPiece => {
                            let request = match PieceExchange::recv_request(&message) {
                                Ok(request) => request,
                                Err(_) => {
                                    println!("Closing peer connection after invalid RequestPiece");
                                    break;
                                }
                            };

                            let piece_count = match torrent_manager.piece_count(&handshake.info_hash)
                            {
                                Some(count) => count,
                                None => {
                                    println!("Closing peer connection: missing payload store");
                                    break;
                                }
                            };

                            if request.piece_index >= piece_count {
                                println!(
                                    "Closing peer connection after out-of-range piece request {}",
                                    request.piece_index
                                );
                                break;
                            }

                            if piece_request_queue.len() >= Self::MAX_PIECE_REQUEST_QUEUE {
                                println!("Closing peer connection after piece queue overflow");
                                break;
                            }

                            piece_request_queue.push_back(request);

                            let mut keep_connection = true;

                            for _ in 0..Self::MAX_PIECE_RESPONSES_PER_TICK {
                                let Some(request) = (if pop_piece_from_back {
                                    piece_request_queue.pop_back()
                                } else {
                                    piece_request_queue.pop_front()
                                }) else {
                                    break;
                                };

                                pop_piece_from_back = !pop_piece_from_back;

                                let piece_index = request.piece_index;
                                let maybe_total_pieces = torrent_manager.piece_count(&handshake.info_hash);
                                let piece_bytes = match torrent_manager
                                    .read_piece_bytes(&handshake.info_hash, piece_index)
                                {
                                    Ok(bytes) => bytes,
                                    Err(_) => {
                                        println!(
                                            "Closing peer connection after piece read failure"
                                        );
                                        keep_connection = false;
                                        break;
                                    }
                                };

                                // Verify that this hash commitment is a valid leaf in the
                                // on-chain Merkle root before accepting it.  Without this
                                // check a malicious leecher could commit to fake hashes that
                                // don't correspond to the paid secrets.
                                let root = merkle_root.unwrap();
                                if !verify_proof(
                                    request.hash,
                                    request.proof.clone(),
                                    piece_index as usize,
                                    root,
                                ) {
                                    println!(
                                        "Closing peer connection after invalid Merkle proof for piece {}",
                                        piece_index
                                    );
                                    keep_connection = false;
                                    break;
                                }

                                let is_new_commitment =
                                    committed_piece_hashes.insert(piece_index, request.hash).is_none();
                                highest_requested_piece = match highest_requested_piece {
                                    Some(current_max) => Some(current_max.max(piece_index)),
                                    None => Some(piece_index),
                                };
                                if is_new_commitment
                                    && let Some(total_pieces) = maybe_total_pieces
                                {
                                    let done = committed_piece_hashes.len() as u32;
                                    print_piece_progress("  transfer send", done.min(total_pieces), total_pieces);
                                }

                                if piece_index == 0 {
                                    let key: [u8; 32] = rand::random();
                                    piece_keys.insert(piece_index, key);

                                    if PieceExchange::send_encrypted_piece(
                                        &mut stream,
                                        piece_index,
                                        &key,
                                        &piece_bytes,
                                    )
                                    .is_err()
                                    {
                                        println!(
                                            "Closing peer connection after encrypted piece send failure"
                                        );
                                        keep_connection = false;
                                        break;
                                    }

                                    continue;
                                }

                                let prev_index = piece_index - 1;
                                let Some(revealed_secret) = request.secret else {
                                    println!(
                                        "Closing peer connection: missing secret for piece {}",
                                        piece_index
                                    );
                                    keep_connection = false;
                                    break;
                                };

                                let Some(expected_hash) = committed_piece_hashes.get(&prev_index) else {
                                    println!(
                                        "Closing peer connection: missing hash commitment for piece {}",
                                        prev_index
                                    );
                                    keep_connection = false;
                                    break;
                                };

                                if !PieceExchange::verify_secret(&revealed_secret, expected_hash) {
                                    println!(
                                        "Closing peer connection after invalid secret for piece {}",
                                        prev_index
                                    );
                                    keep_connection = false;
                                    break;
                                }

                                let Some(prev_key) = piece_keys.remove(&prev_index) else {
                                    println!(
                                        "Closing peer connection: missing key for piece {}",
                                        prev_index
                                    );
                                    keep_connection = false;
                                    break;
                                };

                                let next_key: [u8; 32] = rand::random();
                                piece_keys.insert(piece_index, next_key);

                                if PieceExchange::send_key_reveal(
                                    &mut stream,
                                    &prev_key,
                                    piece_index,
                                    &next_key,
                                    &piece_bytes,
                                )
                                .is_err()
                                {
                                    println!(
                                        "Closing peer connection after key reveal send failure"
                                    );
                                    keep_connection = false;
                                    break;
                                }
                            }

                            if !keep_connection {
                                break;
                            }
                        }

                        PieceExchangeMsgKind::SecretReveal => {
                            let final_secret = match PieceExchange::recv_final_secret(&message) {
                                Ok(final_secret) => final_secret,
                                Err(_) => {
                                    println!("Closing peer connection after invalid final secret");
                                    break;
                                }
                            };

                            let Some(last_piece_index) = highest_requested_piece else {
                                println!("Closing peer connection: final secret received too early");
                                break;
                            };

                            let Some(expected_hash) = committed_piece_hashes.get(&last_piece_index) else {
                                println!(
                                    "Closing peer connection: missing commitment for final piece {}",
                                    last_piece_index
                                );
                                break;
                            };

                            if !PieceExchange::verify_secret(&final_secret.secret, expected_hash) {
                                println!("Closing peer connection after invalid final secret");
                                break;
                            }

                            let piece_bytes = match torrent_manager
                                .read_piece_bytes(&handshake.info_hash, last_piece_index)
                            {
                                Ok(bytes) => bytes,
                                Err(_) => {
                                    println!(
                                        "Closing peer connection after final piece read failure"
                                    );
                                    break;
                                }
                            };

                            if PieceExchange::send_plain_piece(
                                &mut stream,
                                last_piece_index,
                                &piece_bytes,
                            )
                            .is_err()
                            {
                                println!("Closing peer connection after final piece send failure");
                                break;
                            }
                            println!();
                            println!("Piece exchange completed for {}", handshake.info_hash_hex());
                            break;
                        }

                        _ => {
                            println!("Closing peer connection after unexpected piece-exchange kind");
                            break;
                        }
                    }
                }

                _ => {
                    println!("Closing peer connection after unexpected message ID: {:?}", message.id);
                    break;
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
