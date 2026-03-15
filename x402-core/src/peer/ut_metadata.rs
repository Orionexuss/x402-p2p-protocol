use std::net::TcpStream;

#[derive(Debug, Clone, Copy)]
enum MetadataMessageType {
    Request = 0,
    Data = 1,
    Reject = 2,
}

const MAX_METADATA_SIZE: u32 = 16 * 1024; // 16 KiB
pub const METADATA_PIECE_SIZE: u32 = MAX_METADATA_SIZE;

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

        let payload = serde_bencode::to_bytes(&request).unwrap();

        let message = X402Message::new_extended(extended_message_id, payload);

        write_message(stream, &message).unwrap();

        Ok(())
    }

    pub fn send_ut_metadata_data(
        stream: &mut TcpStream,
        extended_message_id: u8,
        piece: u32,
        total_size: u32,
        data_block: &[u8],
    ) -> std::io::Result<()> {
        let mut payload = serde_bencode::to_bytes(&MetadataMessage {
            msg_type: MetadataMessageType::Data as u8,
            piece,
            total_size: Some(total_size),
        })
        .unwrap();

        payload.extend_from_slice(data_block);

        let message = X402Message::new_extended(extended_message_id, payload);
        write_message(stream, &message).unwrap();

        Ok(())
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
        .unwrap();

        let message = X402Message::new_extended(extended_message_id, payload);
        write_message(stream, &message).unwrap();

        Ok(())
    }

    pub fn receive_ut_metadata_request(message: &X402Message) -> Option<u32> {
        if message.id != X402MessageId::Extended {
            println!("Received non-request message or wrong extended message ID");
            return None;
        }

        let request: MetadataMessage = serde_bencode::from_bytes(&message.payload).ok()?;

        if request.msg_type != MetadataMessageType::Request as u8 {
            return None;
        }

        Some(request.piece)
    }

    pub fn metadata_piece_bounds(piece: u32, total_size: usize) -> Option<(usize, usize)> {
        let start = piece as usize * METADATA_PIECE_SIZE as usize;
        if start >= total_size {
            return None;
        }

        let end = (start + METADATA_PIECE_SIZE as usize).min(total_size);
        Some((start, end))
    }
}
