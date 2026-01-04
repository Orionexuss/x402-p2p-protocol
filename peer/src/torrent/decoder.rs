use serde::{Deserialize, Serialize};
use serde_bencode;
use sha1::{Digest, Sha1};

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Torrent {
    announce: String,
    info: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Info {
    pub name: String,

    #[serde(rename = "piece length")]
    pub plength: usize,

    pub pieces: Vec<u8>,

    #[serde(flatten)]
    pub keys: Vec<u8>,
}

pub fn decode_torrent(data: &[u8]) -> Result<(), String> {
    let s = std::str::from_utf8(data).map_err(|e| e.to_string())?;
    let decoded = serde_bencode::from_str(&s);

    Ok(())
}
