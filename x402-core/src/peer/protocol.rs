use std::io::{Read, Write};
use std::net::TcpStream;
use thiserror::Error;

/// X402 protocol message IDs
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X402MessageId {
    AuthProof = 0,
    AuthOk = 1,
    InquirePrice = 2,
    PriceOffer = 3,
    LockedPayment = 4,
    RequestBlock = 5,
    PieceChunk = 6,
    PaymentReveal = 7,
    PaymentAck = 8,
    Have = 9,
    Extended = 20,
}

impl X402MessageId {
    /// Convert a u8 to a message ID
    pub fn from_u8(value: u8) -> Result<Self, ProtocolError> {
        match value {
            0 => Ok(X402MessageId::AuthProof),
            1 => Ok(X402MessageId::AuthOk),
            2 => Ok(X402MessageId::InquirePrice),
            3 => Ok(X402MessageId::PriceOffer),
            4 => Ok(X402MessageId::LockedPayment),
            5 => Ok(X402MessageId::RequestBlock),
            6 => Ok(X402MessageId::PieceChunk),
            7 => Ok(X402MessageId::PaymentReveal),
            8 => Ok(X402MessageId::PaymentAck),
            9 => Ok(X402MessageId::Have),
            20 => Ok(X402MessageId::Extended),
            _ => Err(ProtocolError::InvalidMessageId(value)),
        }
    }

    /// Convert message ID to u8
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// X402 protocol message
#[derive(Debug, Clone)]
pub struct X402Message {
    pub id: X402MessageId,
    pub extended_message_id: Option<u8>, // Only used if id == Extended
    pub payload: Vec<u8>,
}

impl X402Message {
    /// Create a new message
    pub fn new(id: X402MessageId, payload: Vec<u8>) -> Self {
        Self {
            id,
            extended_message_id: None,
            payload,
        }
    }

    pub fn new_extended(extended_message_id: u8, payload: Vec<u8>) -> Self {
        Self {
            id: X402MessageId::Extended,
            extended_message_id: Some(extended_message_id),
            payload,
        }
    }

    /// Get the total message length (ID byte + payload)
    pub fn message_length(&self) -> u32 {
        1 + self.payload.len() as u32
    }
}

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid message ID: {0}")]
    InvalidMessageId(u8),

    #[error("Message too large: {0} bytes (max 16MB)")]
    MessageTooLarge(u32),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Invalid extended message format")]
    InvalidExtendedMessage,
}

const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024; // 16 MB

/// Read a message from a TCP stream
///
/// Message format:
/// - 4 bytes: big-endian length (includes message ID + payload)
/// - 1 byte: message ID
/// - N bytes: payload
pub fn read_message(stream: &mut TcpStream) -> Result<X402Message, ProtocolError> {
    // Read 4-byte length prefix
    let mut length_buf = [0u8; 4];
    stream.read_exact(&mut length_buf)?;
    let length = u32::from_be_bytes(length_buf);

    // Validate message size
    if length == 0 {
        return Err(ProtocolError::ConnectionClosed);
    }
    if length > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge(length));
    }

    // Read message ID (1 byte)
    let mut id_buf = [0u8; 1];
    stream.read_exact(&mut id_buf)?;
    let id = X402MessageId::from_u8(id_buf[0])?;

    let mut extended_message_id = None;

    if id == X402MessageId::Extended {
        // Read extended message ID (1 byte)
        let mut ext_id_buf = [0u8; 1];
        stream.read_exact(&mut ext_id_buf)?;
        extended_message_id = Some(ext_id_buf[0]);
    }

    // Read payload (remaining bytes)
    let payload_length = if id == X402MessageId::Extended {
        (length - 2) as usize
    } else {
        (length - 1) as usize
    };
    let mut payload = vec![0u8; payload_length];
    stream.read_exact(&mut payload)?;

    Ok(X402Message {
        id,
        extended_message_id,
        payload,
    })
}

/// Write a message to a TCP stream
///
/// Message format:
/// - 4 bytes: big-endian length (includes message ID + payload)
/// - 1 byte: message ID
/// - N bytes: payload
pub fn write_message(stream: &mut TcpStream, message: &X402Message) -> Result<(), ProtocolError> {
    let mut length = 1 + message.payload.len(); // message_id + payload

    if message.id == X402MessageId::Extended {
        length += 1; // extended message id
    }

    if length > MAX_MESSAGE_SIZE as usize {
        return Err(ProtocolError::MessageTooLarge(length as u32));
    }

    // Write 4-byte length prefix
    stream.write_all(&(length as u32).to_be_bytes())?;

    // Write 1-byte message ID
    stream.write_all(&[message.id.to_u8()])?;

    // If this is an extended message, write the extended message ID byte
    if message.id == X402MessageId::Extended {
        let ext_id = message
            .extended_message_id
            .ok_or(ProtocolError::InvalidExtendedMessage)?;

        stream.write_all(&[ext_id])?;
    }

    // Write payload
    stream.write_all(&message.payload)?;

    // Ensure data is sent
    stream.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_id_conversion() {
        assert_eq!(X402MessageId::from_u8(0).unwrap(), X402MessageId::AuthProof);
        assert_eq!(X402MessageId::from_u8(1).unwrap(), X402MessageId::AuthOk);
        assert_eq!(
            X402MessageId::from_u8(8).unwrap(),
            X402MessageId::PaymentAck
        );
        assert!(X402MessageId::from_u8(10).is_err());
    }

    #[test]
    fn test_message_length() {
        let msg = X402Message::new(X402MessageId::Have, vec![1, 2, 3, 4]);
        assert_eq!(msg.message_length(), 5); // 1 byte ID + 4 bytes payload
    }

    #[test]
    fn test_message_id_round_trip() {
        for i in 0..=8 {
            let id = X402MessageId::from_u8(i).unwrap();
            assert_eq!(id.to_u8(), i);
        }
    }
}
