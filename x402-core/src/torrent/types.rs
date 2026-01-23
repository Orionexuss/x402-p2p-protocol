use serde::{Deserialize, Serialize};

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

impl Torrent {
    /// Get the total length of the torrent in bytes
    pub fn total_length(&self) -> u64 {
        self.info.length.unwrap_or(0) as u64
    }

    /// Get the number of pieces
    pub fn num_pieces(&self) -> usize {
        self.info.pieces.len() / 20
    }
}
