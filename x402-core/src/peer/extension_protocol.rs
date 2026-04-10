use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::TcpStream};

use crate::{
    peer::protocol::ProtocolError, write_message, X402Message, X402MessageId,
};

pub const UT_METADATA_EXTENSION_ID: u8 = 1;

#[derive(Serialize, Deserialize, Debug)]
pub struct ExtendedHandshake {
    pub m: HashMap<String, u8>, // Mapping of extension name to message ID

    #[serde(default)]
    pub metadata_size: Option<u64>,
}

impl Default for ExtendedHandshake {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtendedHandshake {
    pub fn new() -> Self {
        let mut extensions = HashMap::new();

        extensions.insert("ut_metadata".to_string(), UT_METADATA_EXTENSION_ID);

        Self {
            m: extensions,
            metadata_size: None,
        }
    }

    pub fn to_bencode(&self) -> Result<Vec<u8>, ProtocolError> {
        serde_bencode::ser::to_bytes(&self).map_err(|_| ProtocolError::InvalidExtendedMessage)
    }
    pub fn from_bencode(data: &[u8]) -> Result<Self, ProtocolError> {
        serde_bencode::de::from_bytes(data).map_err(|_| ProtocolError::InvalidExtendedMessage)
    }

    pub fn send_extended_handshake(&self, stream: &mut TcpStream) -> Result<(), ProtocolError> {
        let payload = self.to_bencode()?;

        let message = X402Message::new_extended(0, payload);

        write_message(stream, &message)
    }

    pub fn receive_extended_handshake(
        message: &X402Message,
    ) -> Result<ExtendedHandshake, ProtocolError> {
        if message.id != X402MessageId::Extended {
            return Err(ProtocolError::InvalidMessageId(message.id.to_u8()));
        }

        if message.extended_message_id != Some(0) {
            return Err(ProtocolError::InvalidExtendedMessage);
        }

        let handshake = ExtendedHandshake::from_bencode(&message.payload)?;

        Ok(handshake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_to_bencode() {
        let handshake = ExtendedHandshake::new();
        let bytes = handshake.to_bencode().unwrap();

        // The expected bencoded form is: d1:md11:ut_metadatai1eee
        let expected = b"d1:md11:ut_metadatai1eee".to_vec();
        println!("{}", bytes.len());

        assert_eq!(bytes, expected);
    }
}
