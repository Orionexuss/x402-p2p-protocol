use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Torrent {
    pub(crate) announce: String,
    pub(crate) info: Info,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Info {
    pub(crate) name: String,

    #[serde(rename = "piece length")]
    pub(crate) plength: usize,

    pub(crate) pieces: serde_bytes::ByteBuf,

    #[serde(default)]
    pub(crate) length: Option<usize>,
}
