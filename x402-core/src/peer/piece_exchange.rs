use std::net::TcpStream;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::peer::protocol::{write_message, ProtocolError, X402Message, X402MessageId};
use crate::peer::utils::merkletree::hash;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceExchangeMsgKind {
    RequestPiece = 0,
    EncryptedPiece = 1,
    SecretReveal = 2,
    KeyReveal = 3,
    PlainPiece = 4,
}

impl PieceExchangeMsgKind {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::RequestPiece),
            1 => Some(Self::EncryptedPiece),
            2 => Some(Self::SecretReveal),
            3 => Some(Self::KeyReveal),
            4 => Some(Self::PlainPiece),
            _ => None,
        }
    }
}

/// Request for a piece, optionally revealing the previous piece secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestPiece {
    pub piece_index: u32,
    pub hash: [u8; 32],
    pub secret: Option<[u8; 32]>,
    pub proof: Vec<[u8; 32]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPiece {
    pub piece_index: u32,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalSecretReveal {
    pub secret: [u8; 32],
}

/// Reveals the previous decryption key and carries the next encrypted piece.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyReveal {
    pub key: [u8; 32],
    pub next_piece: EncryptedPiece,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlainPiece {
    pub piece_index: u32,
    pub data: Vec<u8>,
}

fn encode_u32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

fn decode_u32(buf: &[u8], offset: usize) -> Option<(u32, usize)> {
    if buf.len() < offset + 4 {
        return None;
    }
    let v = u32::from_be_bytes(buf[offset..offset + 4].try_into().ok()?);
    Some((v, offset + 4))
}

fn encode_bytes32(b: &[u8; 32]) -> &[u8; 32] {
    b
}

fn decode_bytes32(buf: &[u8], offset: usize) -> Option<([u8; 32], usize)> {
    if buf.len() < offset + 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&buf[offset..offset + 32]);
    Some((out, offset + 32))
}

fn encode_varlen(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + data.len());
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    out
}

fn decode_varlen(buf: &[u8], offset: usize) -> Option<(Vec<u8>, usize)> {
    let (len, after_len) = decode_u32(buf, offset)?;
    let end = after_len + len as usize;
    if buf.len() < end {
        return None;
    }
    Some((buf[after_len..end].to_vec(), end))
}

/// Symmetric XOR stream built from SHA-256(key || counter).
pub fn xor_keystream(key: &[u8; 32], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut counter: u64 = 0;
    while out.len() < data.len() {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.update(counter.to_be_bytes());
        let block: [u8; 32] = hasher.finalize().into();
        let remaining = data.len() - out.len();
        let take = remaining.min(32);
        out.extend_from_slice(&block[..take]);
        counter += 1;
    }
    for (o, d) in out.iter_mut().zip(data.iter()) {
        *o ^= d;
    }
    out
}

pub struct PieceExchange;

impl PieceExchange {
    /// Request a piece, commit its secret hash, and optionally reveal the previous secret.
    pub fn send_request(
        stream: &mut TcpStream,
        piece_index: u32,
        hash: &[u8; 32],
        secret: Option<&[u8; 32]>,
        proof: &[[u8; 32]],
    ) -> Result<(), ProtocolError> {
        let mut payload = Vec::new();
        payload.push(PieceExchangeMsgKind::RequestPiece as u8);
        payload.extend_from_slice(&encode_u32(piece_index));
        payload.extend_from_slice(encode_bytes32(hash));
        if let Some(s) = secret {
            payload.push(1u8);
            payload.extend_from_slice(encode_bytes32(s));
        } else {
            payload.push(0u8);
        }
        payload.extend_from_slice(&(proof.len() as u32).to_be_bytes());
        for node in proof {
            payload.extend_from_slice(node);
        }
        write_message(
            stream,
            &X402Message::new(X402MessageId::PieceExchange, payload),
        )
    }

    pub fn send_final_secret(
        stream: &mut TcpStream,
        secret: &[u8; 32],
    ) -> Result<(), ProtocolError> {
        let mut payload = Vec::new();
        payload.push(PieceExchangeMsgKind::SecretReveal as u8);
        payload.extend_from_slice(encode_bytes32(secret));
        write_message(
            stream,
            &X402Message::new(X402MessageId::PieceExchange, payload),
        )
    }

    pub fn recv_encrypted_piece(msg: &X402Message) -> Result<EncryptedPiece, PieceExchangeError> {
        let p = &msg.payload;
        if p.is_empty() || p[0] != PieceExchangeMsgKind::EncryptedPiece as u8 {
            return Err(PieceExchangeError::UnexpectedMessage);
        }
        let (index, after_idx) = decode_u32(p, 1).ok_or(PieceExchangeError::MalformedMessage)?;
        let (ciphertext, _) =
            decode_varlen(p, after_idx).ok_or(PieceExchangeError::MalformedMessage)?;
        Ok(EncryptedPiece {
            piece_index: index,
            ciphertext,
        })
    }

    pub fn recv_key_reveal(msg: &X402Message) -> Result<KeyReveal, PieceExchangeError> {
        let p = &msg.payload;
        if p.is_empty() || p[0] != PieceExchangeMsgKind::KeyReveal as u8 {
            return Err(PieceExchangeError::UnexpectedMessage);
        }
        let (key, after_key) = decode_bytes32(p, 1).ok_or(PieceExchangeError::MalformedMessage)?;
        let (index, after_idx) =
            decode_u32(p, after_key).ok_or(PieceExchangeError::MalformedMessage)?;
        let (ciphertext, _) =
            decode_varlen(p, after_idx).ok_or(PieceExchangeError::MalformedMessage)?;
        Ok(KeyReveal {
            key,
            next_piece: EncryptedPiece {
                piece_index: index,
                ciphertext,
            },
        })
    }

    pub fn recv_plain_piece(msg: &X402Message) -> Result<PlainPiece, PieceExchangeError> {
        let p = &msg.payload;
        if p.is_empty() || p[0] != PieceExchangeMsgKind::PlainPiece as u8 {
            return Err(PieceExchangeError::UnexpectedMessage);
        }
        let (index, after_idx) = decode_u32(p, 1).ok_or(PieceExchangeError::MalformedMessage)?;
        let (data, _) = decode_varlen(p, after_idx).ok_or(PieceExchangeError::MalformedMessage)?;
        Ok(PlainPiece {
            piece_index: index,
            data,
        })
    }

    pub fn send_encrypted_piece(
        stream: &mut TcpStream,
        piece_index: u32,
        key: &[u8; 32],
        piece_data: &[u8],
    ) -> Result<(), ProtocolError> {
        let ciphertext = xor_keystream(key, piece_data);
        let mut payload = Vec::new();
        payload.push(PieceExchangeMsgKind::EncryptedPiece as u8);
        payload.extend_from_slice(&encode_u32(piece_index));
        payload.extend_from_slice(&encode_varlen(&ciphertext));
        write_message(
            stream,
            &X402Message::new(X402MessageId::PieceExchange, payload),
        )
    }

    /// Reveal the previous decryption key together with the next encrypted piece.
    pub fn send_key_reveal(
        stream: &mut TcpStream,
        key_for_previous: &[u8; 32],
        next_piece_index: u32,
        next_key: &[u8; 32],
        next_piece_data: &[u8],
    ) -> Result<(), ProtocolError> {
        let ciphertext = xor_keystream(next_key, next_piece_data);
        let mut payload = Vec::new();
        payload.push(PieceExchangeMsgKind::KeyReveal as u8);
        payload.extend_from_slice(encode_bytes32(key_for_previous));
        payload.extend_from_slice(&encode_u32(next_piece_index));
        payload.extend_from_slice(&encode_varlen(&ciphertext));
        write_message(
            stream,
            &X402Message::new(X402MessageId::PieceExchange, payload),
        )
    }

    pub fn send_plain_piece(
        stream: &mut TcpStream,
        piece_index: u32,
        piece_data: &[u8],
    ) -> Result<(), ProtocolError> {
        let mut payload = Vec::new();
        payload.push(PieceExchangeMsgKind::PlainPiece as u8);
        payload.extend_from_slice(&encode_u32(piece_index));
        payload.extend_from_slice(&encode_varlen(piece_data));
        write_message(
            stream,
            &X402Message::new(X402MessageId::PieceExchange, payload),
        )
    }

    pub fn recv_request(msg: &X402Message) -> Result<RequestPiece, PieceExchangeError> {
        let p = &msg.payload;
        if p.is_empty() || p[0] != PieceExchangeMsgKind::RequestPiece as u8 {
            return Err(PieceExchangeError::UnexpectedMessage);
        }
        let (index, after_idx) = decode_u32(p, 1).ok_or(PieceExchangeError::MalformedMessage)?;
        let (h, after_hash) =
            decode_bytes32(p, after_idx).ok_or(PieceExchangeError::MalformedMessage)?;
        if after_hash >= p.len() {
            return Err(PieceExchangeError::MalformedMessage);
        }
        let (secret, after_secret) = match p[after_hash] {
            0 => (None, after_hash + 1),
            1 => {
                let (s, after_s) = decode_bytes32(p, after_hash + 1)
                    .ok_or(PieceExchangeError::MalformedMessage)?;
                (Some(s), after_s)
            }
            _ => return Err(PieceExchangeError::MalformedMessage),
        };
        let (proof_len, mut offset) =
            decode_u32(p, after_secret).ok_or(PieceExchangeError::MalformedMessage)?;
        let mut proof = Vec::with_capacity(proof_len as usize);
        for _ in 0..proof_len {
            let (node, next) =
                decode_bytes32(p, offset).ok_or(PieceExchangeError::MalformedMessage)?;
            proof.push(node);
            offset = next;
        }
        Ok(RequestPiece {
            piece_index: index,
            hash: h,
            secret,
            proof,
        })
    }

    pub fn recv_final_secret(msg: &X402Message) -> Result<FinalSecretReveal, PieceExchangeError> {
        let p = &msg.payload;
        if p.is_empty() || p[0] != PieceExchangeMsgKind::SecretReveal as u8 {
            return Err(PieceExchangeError::UnexpectedMessage);
        }
        let (secret, _) = decode_bytes32(p, 1).ok_or(PieceExchangeError::MalformedMessage)?;
        Ok(FinalSecretReveal { secret })
    }

    pub fn decrypt(key: &[u8; 32], ciphertext: &[u8]) -> Vec<u8> {
        xor_keystream(key, ciphertext)
    }

    pub fn verify_secret(secret: &[u8; 32], expected_hash: &[u8; 32]) -> bool {
        &hash(secret) == expected_hash
    }

    pub fn msg_kind(msg: &X402Message) -> Option<PieceExchangeMsgKind> {
        if msg.id != X402MessageId::PieceExchange {
            return None;
        }
        msg.payload
            .first()
            .and_then(|&b| PieceExchangeMsgKind::from_u8(b))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PieceExchangeError {
    #[error("Malformed piece-exchange message")]
    MalformedMessage,
    #[error("Unexpected message kind")]
    UnexpectedMessage,
    #[error("Secret verification failed")]
    BadSecret,
    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xor_keystream_symmetric() {
        let key = [0xABu8; 32];
        let data = b"hello, world! this is a test piece of data.";
        let encrypted = xor_keystream(&key, data);
        assert_ne!(encrypted, data.to_vec());
        let decrypted = xor_keystream(&key, &encrypted);
        assert_eq!(decrypted, data.to_vec());
    }

    #[test]
    fn test_verify_secret() {
        let secret: [u8; 32] = [0x01u8; 32];
        let h = hash(&secret);
        assert!(PieceExchange::verify_secret(&secret, &h));
        let wrong: [u8; 32] = [0x02u8; 32];
        assert!(!PieceExchange::verify_secret(&wrong, &h));
    }

    #[test]
    fn test_request_piece_roundtrip_no_secret() {
        let h = [0x11u8; 32];
        let mut payload = Vec::new();
        payload.push(PieceExchangeMsgKind::RequestPiece as u8);
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&h);
        payload.push(0u8);
        let msg = X402Message::new(X402MessageId::PieceExchange, payload);
        let req = PieceExchange::recv_request(&msg).unwrap();
        assert_eq!(req.piece_index, 0);
        assert_eq!(req.hash, h);
        assert!(req.secret.is_none());
    }

    #[test]
    fn test_request_piece_roundtrip_with_secret() {
        let h = [0x22u8; 32];
        let s = [0x33u8; 32];
        let mut payload = Vec::new();
        payload.push(PieceExchangeMsgKind::RequestPiece as u8);
        payload.extend_from_slice(&1u32.to_be_bytes());
        payload.extend_from_slice(&h);
        payload.push(1u8);
        payload.extend_from_slice(&s);
        let msg = X402Message::new(X402MessageId::PieceExchange, payload);
        let req = PieceExchange::recv_request(&msg).unwrap();
        assert_eq!(req.piece_index, 1);
        assert_eq!(req.hash, h);
        assert_eq!(req.secret, Some(s));
    }

    #[test]
    fn test_final_secret_roundtrip() {
        let s = [0xFFu8; 32];
        let mut payload = vec![PieceExchangeMsgKind::SecretReveal as u8];
        payload.extend_from_slice(&s);
        let msg = X402Message::new(X402MessageId::PieceExchange, payload);
        let reveal = PieceExchange::recv_final_secret(&msg).unwrap();
        assert_eq!(reveal.secret, s);
    }

    #[test]
    fn test_plain_piece_roundtrip() {
        let data = b"piece data here".to_vec();
        let mut payload = vec![PieceExchangeMsgKind::PlainPiece as u8];
        payload.extend_from_slice(&2u32.to_be_bytes());
        let mut varlen = (data.len() as u32).to_be_bytes().to_vec();
        varlen.extend_from_slice(&data);
        payload.extend_from_slice(&varlen);
        let msg = X402Message::new(X402MessageId::PieceExchange, payload);
        let plain = PieceExchange::recv_plain_piece(&msg).unwrap();
        assert_eq!(plain.piece_index, 2);
        assert_eq!(plain.data, data);
    }

    #[test]
    fn test_key_reveal_roundtrip() {
        let key = [0xAAu8; 32];
        let inner_ciphertext = b"encrypted next piece".to_vec();
        let mut payload = vec![PieceExchangeMsgKind::KeyReveal as u8];
        payload.extend_from_slice(&key);
        payload.extend_from_slice(&1u32.to_be_bytes());
        let mut varlen = (inner_ciphertext.len() as u32).to_be_bytes().to_vec();
        varlen.extend_from_slice(&inner_ciphertext);
        payload.extend_from_slice(&varlen);
        let msg = X402Message::new(X402MessageId::PieceExchange, payload);
        let kr = PieceExchange::recv_key_reveal(&msg).unwrap();
        assert_eq!(kr.key, key);
        assert_eq!(kr.next_piece.piece_index, 1);
        assert_eq!(kr.next_piece.ciphertext, inner_ciphertext);
    }
}
