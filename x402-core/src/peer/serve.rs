use std::io;
use std::net::{TcpListener, TcpStream};

use svix_ksuid::{KsuidLike, KsuidMs};

use crate::peer::handshake::{generate_peer_id, Handshake};

pub struct Seeder {
    /// The address to bind to
    address: String,
    /// The port to listen on
    port: u16,
    /// Our peer ID
    peer_id: KsuidMs,
    /// Info hashes we're serving
    info_hashes: Vec<[u8; 20]>,
}

impl Seeder {
    pub fn new(address: String, port: u16) -> Self {
        Seeder {
            address,
            port,
            peer_id: generate_peer_id(),
            info_hashes: Vec::new(),
        }
    }

    /// Add an info hash that this seeder can serve
    pub fn add_torrent(&mut self, info_hash: [u8; 20]) {
        self.info_hashes.push(info_hash);
    }

    /// Add an info hash from hex string
    pub fn add_torrent_hex(&mut self, info_hash_hex: &str) -> Result<(), String> {
        if info_hash_hex.len() != 40 {
            return Err(format!(
                "Invalid info hash length: expected 40, got {}",
                info_hash_hex.len()
            ));
        }

        let mut info_hash = [0u8; 20];
        for i in 0..20 {
            info_hash[i] = u8::from_str_radix(&info_hash_hex[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("Invalid hex: {}", e))?;
        }

        self.add_torrent(info_hash);
        Ok(())
    }

    /// Start listening for incoming connections
    pub fn listen(&self) -> io::Result<()> {
        let addr = format!("{}:{}", self.address, self.port);
        let listener = TcpListener::bind(&addr)?;
        println!("Seeder listening on {}", addr);
        println!("Peer ID: {}", hex::encode(self.peer_id.bytes()));

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    println!("New connection from: {}", stream.peer_addr()?);
                    if let Err(e) = self.handle_connection(stream) {
                        eprintln!("Error handling connection: {}", e);
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
    fn handle_connection(&self, mut stream: TcpStream) -> Result<(), String> {
        println!("Waiting for handshake...");

        // Receive the handshake from the leecher
        let handshake = Handshake::receive(&mut stream)?;

        println!("Received handshake:");
        println!("  Info Hash: {}", handshake.info_hash_hex());
        println!("  Peer ID: {}", hex::encode(handshake.peer_id.bytes()));

        // Check if we have this torrent
        if !self.info_hashes.contains(&handshake.info_hash) {
            return Err(format!(
                "We don't have torrent with info hash: {}",
                handshake.info_hash_hex()
            ));
        }

        println!("Info hash matches! Sending handshake response...");

        // Send our handshake response
        let response = Handshake::new(handshake.info_hash, self.peer_id);
        response
            .send(&mut stream)
            .map_err(|e| format!("Failed to send handshake: {}", e))?;

        println!("Handshake successful!");

        // TODO: Continue with piece exchange protocol

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seeder_new() {
        let seeder = Seeder::new("127.0.0.1".to_string(), 6881);
        assert_eq!(seeder.address, "127.0.0.1");
        assert_eq!(seeder.port, 6881);
        assert_eq!(seeder.info_hashes.len(), 0);
    }

    #[test]
    fn test_add_torrent() {
        let mut seeder = Seeder::new("127.0.0.1".to_string(), 6881);
        let info_hash = [1u8; 20];
        seeder.add_torrent(info_hash);
        assert_eq!(seeder.info_hashes.len(), 1);
        assert_eq!(seeder.info_hashes[0], info_hash);
    }

    #[test]
    fn test_add_torrent_hex() {
        let mut seeder = Seeder::new("127.0.0.1".to_string(), 6881);
        let hex = "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36";

        let result = seeder.add_torrent_hex(hex);
        assert!(result.is_ok());
        assert_eq!(seeder.info_hashes.len(), 1);
    }

    #[test]
    fn test_add_torrent_hex_invalid() {
        let mut seeder = Seeder::new("127.0.0.1".to_string(), 6881);
        let result = seeder.add_torrent_hex("invalid");
        assert!(result.is_err());
    }
}
