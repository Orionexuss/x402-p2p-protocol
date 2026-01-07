pub mod torrent;

// Re-export only public API
pub use torrent::magnet::MagnetLink;
pub use torrent::parser::decode_torrent;
