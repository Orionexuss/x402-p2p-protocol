use hex::encode;
use serde_bencode;

use crate::torrent::infohash::derive_infohash;
use crate::torrent::types::{Info, Torrent};

/// Parse a torrent file and return the Torrent struct
fn parse_torrent(data: &[u8]) -> Result<Torrent, String> {
    serde_bencode::from_bytes(data).map_err(|e| format!("Failed to decode torrent: {}", e))
}

/// Calculate the info hash for a torrent
fn calculate_info_hash(torrent: &Torrent) -> Result<String, String> {
    let info_bytes = serde_bencode::to_bytes(&torrent.info)
        .map_err(|e| format!("Failed to encode info dict: {}", e))?;
    let info_hash = derive_infohash(&info_bytes);
    Ok(encode(info_hash))
}

/// Decode and print torrent information
pub fn decode_torrent(data: &[u8]) -> Result<(), String> {
    let decoded = parse_torrent(data)?;
    let info_hash = calculate_info_hash(&decoded)?;

    println!("Tracker URL: {}", decoded.announce);
    println!("Info:");
    println!("  Name: {}", decoded.info.name);
    println!("  Piece Length: {}", decoded.info.plength);
    println!("  Number of Pieces: {}", decoded.info.pieces.len() / 20);
    if let Some(length) = decoded.info.length {
        println!("  File Length: {} bytes", length);
    }
    println!("Info Hash: {}", info_hash);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a minimal valid torrent file in bencode format
    fn create_test_torrent() -> Vec<u8> {
        let torrent = Torrent {
            announce: "http://tracker.example.com".to_string(),
            info: Info {
                name: "test.txt".to_string(),
                plength: 16384,
                pieces: serde_bytes::ByteBuf::from(vec![
                    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
                ]),
                length: Some(1024),
            },
        };
        serde_bencode::to_bytes(&torrent).unwrap()
    }

    #[test]
    fn test_parse_torrent() {
        let data = create_test_torrent();
        let result = parse_torrent(&data);

        assert!(result.is_ok());
        let torrent = result.unwrap();
        assert_eq!(torrent.announce, "http://tracker.example.com");
        assert_eq!(torrent.info.name, "test.txt");
        assert_eq!(torrent.info.plength, 16384);
        assert_eq!(torrent.info.length, Some(1024));
        assert_eq!(torrent.info.pieces.len(), 20);
    }

    #[test]
    fn test_parse_invalid_torrent() {
        let invalid_data = b"this is not bencode";
        let result = parse_torrent(invalid_data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to decode torrent"));
    }

    #[test]
    fn test_calculate_info_hash() {
        let data = create_test_torrent();
        let torrent = parse_torrent(&data).unwrap();
        let info_hash = calculate_info_hash(&torrent);

        assert!(info_hash.is_ok());
        let hash = info_hash.unwrap();
        // SHA-1 hash is 40 hex characters
        assert_eq!(hash.len(), 40);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_info_hash_consistency() {
        // Same torrent should produce same info hash
        let data = create_test_torrent();
        let torrent = parse_torrent(&data).unwrap();

        let hash1 = calculate_info_hash(&torrent).unwrap();
        let hash2 = calculate_info_hash(&torrent).unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_torrent_with_multiple_pieces() {
        let torrent = Torrent {
            announce: "http://tracker.test.com".to_string(),
            info: Info {
                name: "large_file.bin".to_string(),
                plength: 32768,
                // 3 pieces (60 bytes total)
                pieces: serde_bytes::ByteBuf::from(vec![0u8; 60]),
                length: Some(100000),
            },
        };

        let data = serde_bencode::to_bytes(&torrent).unwrap();
        let parsed = parse_torrent(&data).unwrap();

        assert_eq!(parsed.info.name, "large_file.bin");
        assert_eq!(parsed.info.pieces.len(), 60);
        assert_eq!(parsed.info.pieces.len() / 20, 3); // 3 pieces
    }

    #[test]
    fn test_decode_torrent_output() {
        let data = create_test_torrent();
        let result = decode_torrent(&data);

        // Should not error
        assert!(result.is_ok());
    }
}
