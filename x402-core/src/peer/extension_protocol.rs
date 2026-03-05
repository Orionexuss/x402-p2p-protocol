use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct ExtendedHandshake {
    pub m: HashMap<String, u8>, // Mapping of extension name to message ID
}

impl Default for ExtendedHandshake {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtendedHandshake {
    pub fn new() -> Self {
        let mut extensions = HashMap::new();

        extensions.insert("ut_metadata".to_string(), 1);

        Self { m: extensions }
    }

    pub fn to_bencode(&self) -> Vec<u8> {
        serde_bencode::ser::to_bytes(&self).expect("Failed to serialize extended handshake")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_to_bencode() {
        let handshake = ExtendedHandshake::new();
        let bytes = handshake.to_bencode();

        // The expected bencoded form is: d1:md11:ut_metadatai1eee
        let expected = b"d1:md11:ut_metadatai1eee".to_vec();

        assert_eq!(bytes, expected);
    }
}
