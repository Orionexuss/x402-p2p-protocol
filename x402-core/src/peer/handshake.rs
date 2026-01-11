use std::io::{Read, Write};
use std::net::TcpStream;

use svix_ksuid::{KsuidLike, KsuidMs};

const PROTOCOL_STRING: &[u8] = b"BitTorrent protocol";
const HANDSHAKE_LENGTH: usize = 68;

/// Represents a BitTorrent handshake message
#[derive(Debug, Clone, PartialEq)]
pub struct Handshake {
    /// Protocol string length (always 19 for BitTorrent)
    pstrlen: u8,
    /// Protocol string (always "BitTorrent protocol")
    pstr: [u8; 19],
    /// 8 reserved bytes for extensions
    pub reserved: [u8; 8],
    /// 20-byte SHA1 hash of the info dictionary
    pub info_hash: [u8; 20],
    /// 20-byte peer ID
    pub peer_id: KsuidMs,
}

impl Handshake {
    /// Create a new handshake message
    pub fn new(info_hash: [u8; 20], peer_id: KsuidMs) -> Self {
        let mut pstr = [0u8; 19];
        pstr.copy_from_slice(PROTOCOL_STRING);

        Handshake {
            pstrlen: 19,
            pstr,
            reserved: [0u8; 8],
            info_hash,
            peer_id,
        }
    }

    /// Create a handshake from an info hash hex string
    pub fn from_hex(info_hash_hex: &str, peer_id: KsuidMs) -> Result<Self, String> {
        if info_hash_hex.len() != 40 {
            return Err(format!(
                "Invalid info hash length: expected 40, got {}",
                info_hash_hex.len()
            ));
        }

        let mut info_hash = [0u8; 20];
        for i in 0..20 {
            info_hash[i] = u8::from_str_radix(&info_hash_hex[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("Invalid hex in info hash: {}", e))?;
        }

        Ok(Self::new(info_hash, peer_id))
    }

    /// Serialize the handshake to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(HANDSHAKE_LENGTH);
        buf.push(self.pstrlen);
        buf.extend_from_slice(&self.pstr);
        buf.extend_from_slice(&self.reserved);
        buf.extend_from_slice(&self.info_hash);
        buf.extend_from_slice(&self.peer_id.bytes().as_ref());
        buf
    }

    /// Deserialize a handshake from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        if data.len() < HANDSHAKE_LENGTH {
            return Err(format!(
                "Handshake too short: expected {}, got {}",
                HANDSHAKE_LENGTH,
                data.len()
            ));
        }

        let pstrlen = data[0];
        if pstrlen != 19 {
            return Err(format!("Invalid protocol string length: {}", pstrlen));
        }

        let mut pstr = [0u8; 19];
        pstr.copy_from_slice(&data[1..20]);

        if &pstr != PROTOCOL_STRING {
            return Err("Invalid protocol string".to_string());
        }

        let mut reserved = [0u8; 8];
        reserved.copy_from_slice(&data[20..28]);

        let mut info_hash = [0u8; 20];
        info_hash.copy_from_slice(&data[28..48]);

        let mut peer_id_bytes = [0u8; 20];
        peer_id_bytes.copy_from_slice(&data[48..68]);

        let peer_id = KsuidMs::from_bytes(peer_id_bytes);

        Ok(Handshake {
            pstrlen,
            pstr,
            reserved,
            info_hash,
            peer_id,
        })
    }

    pub fn peer_id_hex(&self) -> String {
        hex::encode(self.peer_id.bytes())
    }

    /// Send handshake over a TCP stream
    pub fn send(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        let data = self.serialize();
        stream.write_all(&data)?;
        stream.flush()?;
        Ok(())
    }

    /// Receive handshake from a TCP stream
    pub fn receive(stream: &mut TcpStream) -> Result<Self, String> {
        let mut buf = [0u8; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut buf)
            .map_err(|e| format!("Failed to read handshake: {}", e))?;
        Self::deserialize(&buf)
    }

    /// Perform a complete handshake exchange (send then receive)
    pub fn exchange(
        stream: &mut TcpStream,
        info_hash: [u8; 20],
        peer_id: KsuidMs,
    ) -> Result<Self, String> {
        let handshake = Self::new(info_hash, peer_id);
        handshake
            .send(stream)
            .map_err(|e| format!("Failed to send handshake: {}", e))?;

        let response = Self::receive(stream)?;

        // Verify the info hash matches
        if response.info_hash != info_hash {
            return Err("Info hash mismatch in handshake response".to_string());
        }

        Ok(response)
    }

    /// Get the info hash as a hex string
    pub fn info_hash_hex(&self) -> String {
        self.info_hash
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

/// Generate a random peer ID
pub fn generate_peer_id() -> KsuidMs {
    svix_ksuid::KsuidMs::new(None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_new() {
        let info_hash = [1u8; 20];
        let peer_id = KsuidMs::new(None, None);
        let handshake = Handshake::new(info_hash, peer_id);

        assert_eq!(handshake.pstrlen, 19);
        assert_eq!(&handshake.pstr, PROTOCOL_STRING);
        assert_eq!(handshake.info_hash, info_hash);
        assert_eq!(handshake.peer_id, peer_id);
    }

    #[test]
    fn test_handshake_serialize_deserialize() {
        let info_hash = [1u8; 20];
        let peer_id = KsuidMs::new(None, None);
        let handshake = Handshake::new(info_hash, peer_id);

        let serialized = handshake.serialize();
        assert_eq!(serialized.len(), HANDSHAKE_LENGTH);

        let deserialized = Handshake::deserialize(&serialized).unwrap();
        assert_eq!(handshake, deserialized);
    }

    #[test]
    fn test_handshake_from_hex() {
        let hex = "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36";
        let peer_id = KsuidMs::new(None, None);
        let handshake = Handshake::from_hex(hex, peer_id).unwrap();

        assert_eq!(handshake.info_hash_hex(), hex);
    }

    #[test]
    fn test_handshake_invalid_hex() {
        let peer_id = KsuidMs::new(None, None);
        let result = Handshake::from_hex("invalid", peer_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_handshake_deserialize_invalid_length() {
        let short_data = vec![0u8; 50];
        let result = Handshake::deserialize(&short_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_handshake_deserialize_invalid_protocol() {
        let mut data = vec![0u8; HANDSHAKE_LENGTH];
        data[0] = 19;
        data[1..20].copy_from_slice(b"Invalid Protocol!!!");
        let result = Handshake::deserialize(&data);
        assert!(result.is_err());
    }
}
