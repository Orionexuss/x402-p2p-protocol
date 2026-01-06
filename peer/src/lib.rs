pub mod torrent;

// Re-export commonly used items
pub use torrent::decoder::decode_torrent;
pub use torrent::magnet::MagnetLink;
