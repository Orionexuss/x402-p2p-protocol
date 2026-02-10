#![allow(dead_code)]
use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::{Read, Result as IoResult};
use std::path::Path;

use crate::torrent::infohash::derive_infohash;
use crate::torrent::magnet::MagnetLink;
use crate::torrent::types::{Info, Torrent};

/// Builder for creating torrent files from actual files
pub struct TorrentBuilder {
    /// Path to the file to create a torrent for
    file_path: String,
    /// Tracker announce URL
    announce_url: String,
    /// Piece length in bytes (None = auto-calculate based on file size)
    piece_length: Option<usize>,
}

impl TorrentBuilder {
    /// Create a new TorrentBuilder with automatic piece length calculation
    pub fn new(file_path: &str, announce_url: &str) -> Self {
        Self {
            file_path: file_path.to_string(),
            announce_url: announce_url.to_string(),
            piece_length: None, // Auto-calculate by default
        }
    }

    /// Set custom piece length in bytes (for advanced users only)
    pub fn piece_length(mut self, length: usize) -> Self {
        self.piece_length = Some(length);
        self
    }

    /// Calculate optimal piece length based on file size
    /// This follows BitTorrent conventions:
    /// - Small files (< 16 MB): 16 KB
    /// - Small-medium (16-512 MB): 256 KB  
    /// - Medium-large (512 MB - 2 GB): 512 KB
    /// - Large files (2-4 GB): 1 MB
    /// - Very large (> 4 GB): 2 MB
    fn calculate_optimal_piece_length(file_size: u64) -> usize {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;
        const GB: u64 = 1024 * MB;

        if file_size < 16 * MB {
            16 * 1024 // 16 KB
        } else if file_size < 512 * MB {
            256 * 1024 // 256 KB
        } else if file_size < 2 * GB {
            512 * 1024 // 512 KB
        } else if file_size < 4 * GB {
            1024 * 1024 // 1 MB
        } else {
            2 * 1024 * 1024 // 2 MB
        }
    }

    /// Build the torrent structure
    pub fn build(&self) -> IoResult<Torrent> {
        let path = Path::new(&self.file_path);

        // Get file name
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid file path")
            })?
            .to_string();

        // Open and read file
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        let file_length = metadata.len() as usize;

        // Determine piece length (auto-calculate if not set)
        let piece_length = self
            .piece_length
            .unwrap_or_else(|| Self::calculate_optimal_piece_length(metadata.len()));

        // Calculate pieces
        let pieces = self.calculate_pieces(&mut file, piece_length)?;

        // Create torrent structure
        let torrent = Torrent {
            announce: self.announce_url.clone(),
            info: Info {
                name,
                plength: piece_length,
                pieces: serde_bytes::ByteBuf::from(pieces),
                length: Some(file_length),
            },
        };

        Ok(torrent)
    }

    /// Calculate SHA1 hashes for all pieces
    fn calculate_pieces(&self, file: &mut File, piece_length: usize) -> IoResult<Vec<u8>> {
        let mut pieces = Vec::new();
        let mut buffer = vec![0u8; piece_length];

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break; // End of file
            }

            // Hash this piece
            let mut hasher = Sha1::new();
            hasher.update(&buffer[..bytes_read]);
            let hash = hasher.finalize();

            // Append to pieces (each hash is 20 bytes)
            pieces.extend_from_slice(&hash);
        }

        Ok(pieces)
    }

    /// Build and serialize the torrent to bytes (bencoded)
    pub fn build_to_bytes(&self) -> Result<Vec<u8>, String> {
        let torrent = self
            .build()
            .map_err(|e| format!("Failed to build torrent: {}", e))?;

        serde_bencode::to_bytes(&torrent).map_err(|e| format!("Failed to encode torrent: {}", e))
    }

    /// Build and save torrent to a .torrent file
    pub fn build_to_file(&self, output_path: &str) -> Result<(), String> {
        let bytes = self.build_to_bytes()?;

        std::fs::write(output_path, bytes)
            .map_err(|e| format!("Failed to write torrent file: {}", e))
    }

    /// Build torrent and return info hash
    pub fn build_info_hash(&self) -> Result<[u8; 20], String> {
        let torrent = self
            .build()
            .map_err(|e| format!("Failed to build torrent: {}", e))?;

        let info_bytes = serde_bencode::to_bytes(&torrent.info)
            .map_err(|e| format!("Failed to encode info dict: {}", e))?;

        Ok(derive_infohash(&info_bytes))
    }

    /// Build torrent and generate magnet URI
    pub fn build_magnet(&self) -> Result<MagnetLink, String> {
        let torrent = self
            .build()
            .map_err(|e| format!("Failed to build torrent: {}", e))?;

        let info_hash = self.build_info_hash()?;

        Ok(MagnetLink {
            info_hash: hex::encode(info_hash),
            display_name: Some(torrent.info.name.clone()),
            trackers: vec![self.announce_url.clone()],
            exact_length: Some(torrent.total_length()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_build_torrent_from_file() {
        // Create a temporary test file
        let mut temp_file = NamedTempFile::new().unwrap();
        let test_data = b"Hello, World! This is test data for torrent creation.";
        temp_file.write_all(test_data).unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        // Build torrent
        let builder = TorrentBuilder::new(temp_path, "http://tracker.test.com:6969/announce");
        let torrent = builder.build().unwrap();

        // Verify torrent structure
        assert_eq!(torrent.announce, "http://tracker.test.com:6969/announce");
        assert_eq!(torrent.info.length, Some(test_data.len()));
        assert_eq!(torrent.info.plength, 16 * 1024);
        assert!(!torrent.info.pieces.is_empty());
        assert_eq!(torrent.info.pieces.len() % 20, 0); // Must be multiple of 20
    }

    #[test]
    fn test_build_with_custom_piece_length() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"Small file").unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        let builder = TorrentBuilder::new(temp_path, "http://tracker.test.com").piece_length(16); // Very small pieces for testing

        let torrent = builder.build().unwrap();
        assert_eq!(torrent.info.plength, 16);
    }

    #[test]
    fn test_build_to_bytes() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"Test data").unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        let builder = TorrentBuilder::new(temp_path, "http://tracker.test.com");
        let bytes = builder.build_to_bytes().unwrap();

        // Verify it's valid bencode
        assert!(!bytes.is_empty());
        assert!(bytes[0] == b'd'); // Bencode dictionary starts with 'd'
    }

    #[test]
    fn test_build_info_hash() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"Test data").unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        let builder = TorrentBuilder::new(temp_path, "http://tracker.test.com");
        let info_hash = builder.build_info_hash().unwrap();

        // Info hash should be 20 bytes
        assert_eq!(info_hash.len(), 20);
    }

    #[test]
    fn test_build_magnet() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"Test magnet data").unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        let builder = TorrentBuilder::new(temp_path, "http://tracker.test.com");
        let magnet = builder.build_magnet().unwrap();

        // Verify magnet structure
        assert_eq!(magnet.info_hash.len(), 40); // 20 bytes as hex = 40 chars
        assert!(magnet.display_name.is_some());
        assert_eq!(magnet.trackers.len(), 1);
        assert!(magnet.exact_length.is_some());

        // Verify it can be converted to URL
        let url = magnet.to_url();
        assert!(url.starts_with("magnet:?"));
        assert!(url.contains("xt=urn:btih:"));
    }

    #[test]
    fn test_calculate_pieces_multiple() {
        let mut temp_file = NamedTempFile::new().unwrap();
        // Write enough data to create multiple pieces
        let data = vec![0u8; 1024 * 1024]; // 1 MB
        temp_file.write_all(&data).unwrap();
        let temp_path = temp_file.path().to_str().unwrap();

        let builder =
            TorrentBuilder::new(temp_path, "http://tracker.test.com").piece_length(256 * 1024); // 256 KB pieces

        let torrent = builder.build().unwrap();

        // Should have 4 pieces (1 MB / 256 KB = 4)
        let num_pieces = torrent.info.pieces.len() / 20;
        assert_eq!(num_pieces, 4);
    }
}
