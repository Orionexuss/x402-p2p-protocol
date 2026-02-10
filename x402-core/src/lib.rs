pub mod peer;
pub mod torrent;

// Re-export only public API
pub use peer::handshake::{generate_peer_id, Handshake};
pub use peer::leech::Leecher;
pub use peer::serve::Seeder;
pub use peer::tracker_client::TrackerClient;
pub use torrent::builder::TorrentBuilder;
pub use torrent::magnet::MagnetLink;
pub use torrent::parser::decode_torrent;
pub use torrent::types::Torrent;
