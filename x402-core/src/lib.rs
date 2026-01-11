pub mod peer;
pub mod torrent;

// Re-export only public API
pub use peer::handshake::{generate_peer_id, Handshake};
pub use peer::serve::Seeder;
pub use torrent::magnet::MagnetLink;
pub use torrent::parser::decode_torrent;
