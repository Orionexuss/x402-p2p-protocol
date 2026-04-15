use crate::peer::auth_proof::AuthProof;
use crate::peer::extension_protocol::ExtendedHandshake;
use crate::peer::handshake::{generate_peer_id, Handshake};
use crate::peer::locked_payment::LockedPayment;
use crate::peer::piece_exchange::PieceExchange;
use crate::peer::protocol::X402MessageId;
use crate::peer::tracker_client::{PeerInfo, TrackerClient};
use crate::peer::ut_metadata::{calculate_num_pieces, MetadataMessage, METADATA_PIECE_SIZE};
use crate::peer::utils::merkletree::{get_proof, hash, MerkleTree};
use crate::read_message;
use crate::torrent::types::Info;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::{Client, Cluster};
use anchor_lang::prelude::{declare_program, Pubkey};
use sha1::Digest;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use svix_ksuid::{KsuidLike, KsuidMs};
use thiserror::Error;

declare_program!(x402_contract);

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

    #[error("Payment protocol error: {0}")]
    PaymentError(String),

    #[error("Piece exchange error: {0}")]
    PieceExchangeError(String),
}

pub struct Leecher {
    peer_id: KsuidMs,
    info_hash: [u8; 20],
    pubkey: [u8; 32],
    tracker_url: String,
    output_path: PathBuf,
    total_size: u64,
}

const PROTOCOL_TOTAL_STEPS: usize = 7;

fn protocol_stage(step: usize, label: &str, detail: &str) {
    println!(
        "  [{}/{}] {:<14} {}",
        step, PROTOCOL_TOTAL_STEPS, label, detail
    );
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
    pub async fn download(
        &self,
        is_magnet: bool,
        keypair: &Keypair,
        pieces_length: Option<u32>,
    ) -> Result<(), LeecherError> {
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
        println!("Piece price: {} USDC", response.piece_price);
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
            match self
                .connect_to_peer(peer_info, is_magnet, keypair, pieces_length)
                .await
            {
                Ok(_) => {
                    println!(
                        "Successfully connected to peer {}:{}",
                        peer_info.ip, peer_info.port
                    );
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
        mut pieces_length: Option<u32>,
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

        println!("Connected. Starting protocol session...");

        // Perform BitTorrent handshake
        let handshake = Handshake::exchange(&mut stream, self.info_hash, self.peer_id, 0)
            .map_err(LeecherError::HandshakeFailed)?;

        println!("Protocol timeline:");
        protocol_stage(1, "Handshake", "session established");
        println!(
            "    Peer ID     : {}",
            hex::encode(handshake.peer_id.bytes())
        );
        println!("    Info Hash   : {}", handshake.info_hash_hex());
        println!("    Seeder Price: {}", handshake.price);

        // implement BEP 10
        if is_magnet {
            protocol_step(2, X402MessageId::Extended, "->", "sent extended handshake");
            ExtendedHandshake::new()
                .send_extended_handshake(&mut stream)
                .map_err(|e| LeecherError::MetadataError(e.to_string()))?;
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
                    protocol_step(
                        2,
                        X402MessageId::Extended,
                        "<-",
                        "received extended handshake",
                    );
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

            let metadata_deserialized: Info = serde_bencode::from_bytes(&metadata).unwrap();
            pieces_length = Some(metadata_deserialized.pieces.len() as u32 / 20);

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
        } else {
            protocol_step(
                2,
                X402MessageId::Extended,
                "--",
                "skipped (torrent metadata is local)",
            );
        }

        let auth_proof = AuthProof::create(keypair, AuthProof::generate_nonce());
        auth_proof.send(&mut stream)?;
        protocol_step(
            3,
            X402MessageId::AuthProof,
            "->",
            "sent authentication proof",
        );

        let auth_response = read_message(&mut stream).unwrap().id;

        if auth_response == X402MessageId::AuthOk {
            protocol_step(
                4,
                X402MessageId::AuthOk,
                "<-",
                "authentication accepted by seeder",
            );
        } else {
            println!("Peer authentication failed!");
            return Err(LeecherError::AuthError());
        }

        // Implement merkle tree
        let mut secrets = vec![];
        for _ in 0..pieces_length.unwrap() {
            let s: [u8; 32] = rand::random();
            secrets.push(s);
        }
        let leaves: Vec<[u8; 32]> = secrets.iter().map(|s| hash(s)).collect();
        let tree = MerkleTree::new(leaves);
        let merkle_root = tree.root();

        // Pre-compute a Merkle proof for every secret so the seeder can
        // verify each hash commitment against the on-chain root.
        let proofs: Vec<Vec<[u8; 32]>> = (0..secrets.len()).map(|i| get_proof(&tree, i)).collect();

        let amount = handshake.price;

        if amount == 0 {
            return Err(LeecherError::PaymentError(
                "Seeder reported zero price; refusing to lock zero-value payment".to_string(),
            ));
        }

        let seeder_pubkey = Pubkey::from_str(&peer_info.pubkey)
            .map_err(|e| LeecherError::PaymentError(format!("Invalid seeder pubkey: {}", e)))?;

        let leecher_pubkey = Pubkey::new_from_array(self.pubkey);
        let info_hash = self.info_hash;
        let keypair_bytes = keypair.to_bytes();

        let leecher_keypair = Keypair::try_from(&keypair_bytes[..])
            .map_err(|e| format!("Invalid keypair bytes: {}", e))
            .unwrap();

        let payment = LockedPayment::new(leecher_pubkey, seeder_pubkey, &info_hash, merkle_root);
        let onchain_info_hash = info_hash;
        let total_secrets = pieces_length.ok_or_else(|| {
            LeecherError::PaymentError("Missing piece count before locking payment".to_string())
        })?;

        let onchain_session_result = tokio::task::spawn_blocking(move || {
            // Keep client/program alive for the full on-chain session in this worker.
            let anchor_client = Client::new(Cluster::Devnet, &leecher_keypair);
            let program = anchor_client
                .program(x402_contract::ID)
                .map_err(|e| format!("Failed to create Anchor program client: {}", e))?;

            LockedPayment::new(
                leecher_pubkey,
                seeder_pubkey,
                &onchain_info_hash,
                merkle_root,
            )
            .submit_onchain(amount, total_secrets, &program)
        })
        .await
        .map_err(|e| LeecherError::PaymentError(format!("Payment worker join error: {}", e)))?;

        onchain_session_result
            .map_err(|e| LeecherError::PaymentError(format!("Failed to lock payment: {}", e)))?;

        payment
            .send_locked_payment_message(&mut stream)
            .map_err(|e| {
                LeecherError::PaymentError(format!(
                    "Failed to notify peer of locked payment: {}",
                    e
                ))
            })?;
        protocol_step(
            5,
            X402MessageId::LockedPayment,
            "->",
            "sent merkle-root commitment",
        );

        if LockedPayment::receive_payment_ack(&mut stream).is_err() {
            return Err(LeecherError::PaymentError(
                "Failed to receive payment acknowledgment from peer".to_string(),
            ));
        }
        protocol_step(
            6,
            X402MessageId::PaymentAck,
            "<-",
            "payment acknowledged by seeder",
        );
        println!("Payment locked and verified on-chain successfully!");

        protocol_step(
            7,
            X402MessageId::PieceExchange,
            "..",
            "starting encrypted piece exchange",
        );

        let piece_count = pieces_length.ok_or_else(|| {
            LeecherError::PieceExchangeError(
                "Missing piece count before starting piece exchange".to_string(),
            )
        })?;

        if piece_count == 0 {
            return Err(LeecherError::PieceExchangeError(
                "Torrent has zero pieces".to_string(),
            ));
        }

        const MAX_PIECE_PIPELINE: u32 = 8;
        let pipeline = MAX_PIECE_PIPELINE.min(piece_count).max(1);

        let mut in_flight: HashSet<u32> = HashSet::new();
        let mut encrypted_pieces = vec![None::<Vec<u8>>; piece_count as usize];
        let mut decrypt_keys = vec![None::<[u8; 32]>; piece_count as usize];
        let mut next_request = 0u32;

        while in_flight.len() < pipeline as usize && next_request < piece_count {
            let hash_for_piece = hash(&secrets[next_request as usize]);
            let secret_for_previous = if next_request == 0 {
                None
            } else {
                Some(&secrets[(next_request - 1) as usize])
            };

            PieceExchange::send_request(
                &mut stream,
                next_request,
                &hash_for_piece,
                secret_for_previous,
                &proofs[next_request as usize],
            )
            .map_err(|e| LeecherError::PieceExchangeError(e.to_string()))?;

            in_flight.insert(next_request);
            next_request += 1;
        }

        let mut received_encrypted = 0u32;
        let mut received_keys = 0u32;
        let expected_keys = piece_count.saturating_sub(1);
        print_piece_progress("  transfer recv", 0, piece_count);

        while received_encrypted < piece_count || received_keys < expected_keys {
            let msg = read_message(&mut stream)
                .map_err(|e| LeecherError::PieceExchangeError(e.to_string()))?;

            if msg.id != X402MessageId::PieceExchange {
                continue;
            }

            match PieceExchange::msg_kind(&msg) {
                Some(crate::peer::piece_exchange::PieceExchangeMsgKind::EncryptedPiece) => {
                    let encrypted_piece = PieceExchange::recv_encrypted_piece(&msg)
                        .map_err(|e| LeecherError::PieceExchangeError(e.to_string()))?;

                    let idx = encrypted_piece.piece_index as usize;
                    if idx >= encrypted_pieces.len() {
                        return Err(LeecherError::PieceExchangeError(format!(
                            "Received out-of-range encrypted piece {}",
                            encrypted_piece.piece_index
                        )));
                    }

                    if encrypted_pieces[idx].is_none() {
                        encrypted_pieces[idx] = Some(encrypted_piece.ciphertext);
                        received_encrypted += 1;
                        print_piece_progress("  transfer recv", received_encrypted, piece_count);
                    }

                    in_flight.remove(&(idx as u32));
                }
                Some(crate::peer::piece_exchange::PieceExchangeMsgKind::KeyReveal) => {
                    let key_reveal = PieceExchange::recv_key_reveal(&msg)
                        .map_err(|e| LeecherError::PieceExchangeError(e.to_string()))?;

                    let next_idx_u32 = key_reveal.next_piece.piece_index;
                    let next_idx = next_idx_u32 as usize;
                    if next_idx >= encrypted_pieces.len() {
                        return Err(LeecherError::PieceExchangeError(format!(
                            "Received out-of-range key reveal for next piece {}",
                            next_idx_u32
                        )));
                    }

                    let prev_idx_u32 = next_idx_u32.saturating_sub(1);
                    let prev_idx = prev_idx_u32 as usize;
                    if prev_idx >= decrypt_keys.len() {
                        return Err(LeecherError::PieceExchangeError(format!(
                            "Received out-of-range key for piece {}",
                            prev_idx_u32
                        )));
                    }

                    if decrypt_keys[prev_idx].is_none() {
                        decrypt_keys[prev_idx] = Some(key_reveal.key);
                        received_keys += 1;
                    }

                    if encrypted_pieces[next_idx].is_none() {
                        encrypted_pieces[next_idx] = Some(key_reveal.next_piece.ciphertext);
                        received_encrypted += 1;
                        print_piece_progress("  transfer recv", received_encrypted, piece_count);
                    }

                    in_flight.remove(&next_idx_u32);
                }
                _ => continue,
            }

            while in_flight.len() < pipeline as usize && next_request < piece_count {
                let hash_for_piece = hash(&secrets[next_request as usize]);
                let secret_for_previous = if next_request == 0 {
                    None
                } else {
                    Some(&secrets[(next_request - 1) as usize])
                };

                PieceExchange::send_request(
                    &mut stream,
                    next_request,
                    &hash_for_piece,
                    secret_for_previous,
                    &proofs[next_request as usize],
                )
                .map_err(|e| LeecherError::PieceExchangeError(e.to_string()))?;

                in_flight.insert(next_request);
                next_request += 1;
            }
        }

        println!();

        PieceExchange::send_final_secret(&mut stream, &secrets[(piece_count - 1) as usize])
            .map_err(|e| LeecherError::PieceExchangeError(e.to_string()))?;

        let final_plain = loop {
            let msg = read_message(&mut stream)
                .map_err(|e| LeecherError::PieceExchangeError(e.to_string()))?;

            if msg.id != X402MessageId::PieceExchange {
                continue;
            }

            if !matches!(
                PieceExchange::msg_kind(&msg),
                Some(crate::peer::piece_exchange::PieceExchangeMsgKind::PlainPiece)
            ) {
                continue;
            }

            let plain_piece = PieceExchange::recv_plain_piece(&msg)
                .map_err(|e| LeecherError::PieceExchangeError(e.to_string()))?;
            break plain_piece;
        };

        if final_plain.piece_index != piece_count - 1 {
            return Err(LeecherError::PieceExchangeError(format!(
                "Final plain piece index mismatch: got {}, expected {}",
                final_plain.piece_index,
                piece_count - 1
            )));
        }

        let mut output = Vec::new();
        for idx in 0..(piece_count - 1) as usize {
            let key = decrypt_keys[idx].ok_or_else(|| {
                LeecherError::PieceExchangeError(format!(
                    "Missing decryption key for piece {}",
                    idx
                ))
            })?;
            let ciphertext = encrypted_pieces[idx].as_ref().ok_or_else(|| {
                LeecherError::PieceExchangeError(format!(
                    "Missing encrypted data for piece {}",
                    idx
                ))
            })?;

            let plain = PieceExchange::decrypt(&key, ciphertext);
            output.extend_from_slice(&plain);
        }

        output.extend_from_slice(&final_plain.data);

        fs::write(&self.output_path, &output).map_err(|e| {
            LeecherError::PieceExchangeError(format!("Failed to write output: {}", e))
        })?;

        println!(
            "Piece exchange complete: wrote {} bytes to {}",
            output.len(),
            self.output_path.display()
        );

        Ok(())
    }
}
