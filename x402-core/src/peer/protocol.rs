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
    pub payload: Vec<u8>,
}

impl X402Message {
    /// Create a new message
    pub fn new(id: X402MessageId, payload: Vec<u8>) -> Self {
        Self { id, payload }
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

    // Read payload (remaining bytes)
    let payload_length = (length - 1) as usize; // Subtract 1 for the message ID byte
    let mut payload = vec![0u8; payload_length];
    stream.read_exact(&mut payload)?;

    Ok(X402Message { id, payload })
}

/// Write a message to a TCP stream
///
/// Message format:
/// - 4 bytes: big-endian length (includes message ID + payload)
/// - 1 byte: message ID
/// - N bytes: payload
pub fn write_message(stream: &mut TcpStream, message: &X402Message) -> Result<(), ProtocolError> {
    let length = message.message_length();

    // Validate message size
    if length > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge(length));
    }

    // Write 4-byte length prefix
    stream.write_all(&length.to_be_bytes())?;

    // Write 1-byte message ID
    stream.write_all(&[message.id.to_u8()])?;

    // Write payload
    stream.write_all(&message.payload)?;

    // Ensure data is sent
    stream.flush()?;

    Ok(())
}

/// Example blocking message loop that processes incoming messages
///
/// This is a simple demonstration of how to use the message framing protocol.
/// In production, you would handle each message ID with specific business logic.
pub fn message_loop(mut stream: TcpStream) -> Result<(), ProtocolError> {
    println!("Starting message loop...");

    loop {
        // Read the next message
        let message = match read_message(&mut stream) {
            Ok(msg) => msg,
            Err(ProtocolError::ConnectionClosed) => {
                println!("Connection closed by peer");
                break;
            }
            Err(e) => {
                eprintln!("Error reading message: {}", e);
                return Err(e);
            }
        };

        // Log the received message
        println!(
            "Received message: {:?} ({} bytes)",
            message.id,
            message.payload.len()
        );

        // Handle different message types
        match message.id {
            X402MessageId::AuthProof => {
                println!("  -> AuthProof received");
                // TODO: Verify authentication proof
            }
            X402MessageId::AuthOk => {
                // TODO: Process authentication success
            }
            X402MessageId::InquirePrice => {
                println!("  -> InquirePrice received");
                // TODO: Process price inquiry and send PriceOffer
            }
            X402MessageId::PriceOffer => {
                println!("  -> PriceOffer received");
                // TODO: Evaluate price offer
            }
            X402MessageId::LockedPayment => {
                println!("  -> LockedPayment received");
                // TODO: Verify locked payment
            }
            X402MessageId::RequestBlock => {
                println!("  -> RequestBlock received");
                // TODO: Process block request
            }
            X402MessageId::PieceChunk => {
                println!("  -> PieceChunk received");
                // TODO: Validate and store piece chunk
            }
            X402MessageId::PaymentReveal => {
                println!("  -> PaymentReveal received");
                // TODO: Process payment reveal
            }
            X402MessageId::PaymentAck => {
                println!("  -> PaymentAck received");
                // TODO: Process payment acknowledgment
            }
            X402MessageId::Have => {
                println!("  -> Have received");
                // TODO: Update peer's piece availability
            }
        }
    }

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
