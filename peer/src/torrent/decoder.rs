use hex;
use hex::encode;
use serde::{Deserialize, Serialize};
use serde_bencode;
use sha1::{Digest, Sha1};

use crate::torrent::infohash::derive_infohash;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Torrent {
    pub announce: String,
    pub info: Info,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Info {
    pub name: String,

    #[serde(rename = "piece length")]
    pub plength: usize,

    pub pieces: serde_bytes::ByteBuf,

    #[serde(default)]
    pub length: Option<usize>,
}

pub fn decode_torrent(data: &[u8]) -> Result<(), String> {
    let decoded: Torrent =
        serde_bencode::from_bytes(data).map_err(|e| format!("Failed to decode torrent: {}", e))?;

    let info_bytes = serde_bencode::to_bytes(&decoded.info)
        .map_err(|e| format!("Failed to encode info dict: {}", e))?;
    let info_hash = derive_infohash(&info_bytes);

    println!("Tracker URL: {}", decoded.announce);
    println!("Info:");
    println!("  Name: {}", decoded.info.name);
    println!("  Piece Length: {}", decoded.info.plength);
    println!("  Number of Pieces: {}", decoded.info.pieces.len() / 20);
    if let Some(length) = decoded.info.length {
        println!("  File Length: {} bytes", length);
    }
    println!("Info Hash: {}", encode(info_hash));

    Ok(())
}
