pub fn derive_infohash(info_bytes: &[u8]) -> [u8; 20] {
    use sha1::{Digest, Sha1};

    let mut hasher = Sha1::new();

    hasher.update(info_bytes);
    let result = hasher.finalize();
    let mut infohash = [0u8; 20];
    infohash.copy_from_slice(&result);
    infohash
}
