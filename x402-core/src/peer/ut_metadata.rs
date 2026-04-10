use std::collections::VecDeque;
use std::io;
use std::net::TcpStream;

#[derive(Debug, Clone, Copy)]
enum MetadataMessageType {
    Request = 0,
    Data = 1,
    Reject = 2,
}

pub const METADATA_PIECE_SIZE: u32 = 16 * 1024; // 16 KiB;

pub fn calculate_num_pieces(metadata_size: u32) -> u32 {
    metadata_size.div_ceil(METADATA_PIECE_SIZE)
}

use serde::{Deserialize, Serialize};

use crate::{write_message, X402Message, X402MessageId};

#[derive(Debug, Serialize, Deserialize)]
pub struct MetadataMessage {
    pub msg_type: u8,
    pub piece: u32,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_size: Option<u32>,
}

impl MetadataMessage {
    pub fn create_request(piece: u32) -> Self {
        MetadataMessage {
            msg_type: MetadataMessageType::Request as u8,
            piece,
            total_size: None,
        }
    }

    pub fn send_ut_metadata_request(
        stream: &mut TcpStream,
        extended_message_id: u8,
        piece: u32,
    ) -> std::io::Result<()> {
        let request = MetadataMessage::create_request(piece);

        let payload = serde_bencode::to_bytes(&request)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        let message = X402Message::new_extended(extended_message_id, payload);

        write_message(stream, &message).map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn send_ut_metadata_data(
        stream: &mut TcpStream,
        extended_message_id: u8,
        piece: u32,
        data_block: &[u8],
    ) -> std::io::Result<()> {
        if data_block.len() > METADATA_PIECE_SIZE as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Data block size exceeds METADATA_PIECE_SIZE",
            ));
        }

        let mut payload = serde_bencode::to_bytes(&MetadataMessage {
            msg_type: MetadataMessageType::Data as u8,
            piece,
            total_size: Some(data_block.len() as u32),
        })
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        println!("Sending metadata piece {}", piece);

        payload.extend_from_slice(data_block);

        let message = X402Message::new_extended(extended_message_id, payload);
        write_message(stream, &message).map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn send_ut_metadata_reject(
        stream: &mut TcpStream,
        extended_message_id: u8,
        piece: u32,
    ) -> std::io::Result<()> {
        let payload = serde_bencode::to_bytes(&MetadataMessage {
            msg_type: MetadataMessageType::Reject as u8,
            piece,
            total_size: None,
        })
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        let message = X402Message::new_extended(extended_message_id, payload);
        write_message(stream, &message).map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn receive_ut_metadata_request(message: &X402Message) -> Option<u32> {
        if message.id != X402MessageId::Extended {
            return None;
        }

        let request: MetadataMessage = serde_bencode::from_bytes(&message.payload).ok()?;

        if request.msg_type != MetadataMessageType::Request as u8 {
            return None;
        }

        Some(request.piece)
    }

    pub fn receive_ut_metadata_data(
        message: &X402Message,
        expected_extended_message_id: u8,
    ) -> Option<(u32, Vec<u8>)> {
        if message.id != X402MessageId::Extended {
            return None;
        }

        if message.extended_message_id != Some(expected_extended_message_id) {
            return None;
        }

        let data_msg: MetadataMessage = serde_bencode::from_bytes(&message.payload).ok()?;

        if data_msg.msg_type != MetadataMessageType::Data as u8 {
            return None;
        }
        if data_msg.total_size > Some(METADATA_PIECE_SIZE) {
            return None;
        }

        let data_block_start = serde_bencode::to_bytes(&data_msg).ok()?.len();
        if data_block_start > message.payload.len() {
            return None;
        }
        let data_block = message.payload[data_block_start..].to_vec();

        Some((data_msg.piece, data_block))
    }

    pub fn is_ut_metadata_reject(message: &X402Message, expected_extended_message_id: u8) -> bool {
        if message.id != X402MessageId::Extended
            || message.extended_message_id != Some(expected_extended_message_id)
        {
            return false;
        }

        let Ok(meta) = serde_bencode::from_bytes::<MetadataMessage>(&message.payload) else {
            return false;
        };

        meta.msg_type == MetadataMessageType::Reject as u8
    }

    pub fn metadata_piece_bounds(piece: u32, total_size: usize) -> Option<(usize, usize)> {
        let start = piece as usize * METADATA_PIECE_SIZE as usize;
        if start >= total_size {
            return None;
        }

        let end = (start + METADATA_PIECE_SIZE as usize).min(total_size);
        Some((start, end))
    }

    pub fn flush_metadata_queue(
        stream: &mut TcpStream,
        ext_id: u8,
        metadata: &[u8],
        queue: &mut VecDeque<u32>,
        pop_from_back: &mut bool,
        max_responses_per_tick: usize,
    ) -> std::io::Result<()> {
        let mut sent = 0usize;

        while sent < max_responses_per_tick {
            let next_piece = if *pop_from_back {
                queue.pop_back()
            } else {
                queue.pop_front()
            };

            let Some(piece) = next_piece else {
                break;
            };

            let Some((start, end)) = Self::metadata_piece_bounds(piece, metadata.len()) else {
                Self::send_ut_metadata_reject(stream, ext_id, piece)?;
                continue;
            };

            Self::send_ut_metadata_data(stream, ext_id, piece, &metadata[start..end])?;

            *pop_from_back = !*pop_from_back;
            sent += 1;
        }

        Ok(())
    }
}
