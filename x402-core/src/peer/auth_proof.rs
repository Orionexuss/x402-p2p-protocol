use std::io::{Read, Write};
use std::net::TcpStream;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};

use crate::peer::protocol::{read_message, write_message, X402Message, X402MessageId};

pub struct AuthProof {
    pub signature: Signature,
    pub pubkey: Pubkey,
    pub nonce: [u8; 32],
}

impl AuthProof {
    pub fn create(keypair: &Keypair, nonce: [u8; 32]) -> Self {
        let signature = keypair.sign_message(&nonce);
        AuthProof {
            pubkey: keypair.pubkey(),
            signature,
            nonce,
        }
    }

    pub fn verify(&self) -> bool {
        self.signature.verify(&self.pubkey.to_bytes(), &self.nonce)
    }

    pub fn generate_nonce() -> [u8; 32] {
        let mut nonce = [0u8; 32];
        rand::fill(&mut nonce[..]);
        nonce
    }

    pub fn send(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        let mut payload = Vec::with_capacity(128);
        payload.extend_from_slice(&self.pubkey.to_bytes());
        payload.extend_from_slice(self.signature.as_ref());
        payload.extend_from_slice(&self.nonce);

        let message = X402Message::new(X402MessageId::AuthProof, payload);
        write_message(stream, &message, None)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }

    pub fn receive(stream: &mut TcpStream) -> std::io::Result<Self> {
        let message = read_message(stream, false)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

        if message.id != X402MessageId::AuthProof {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Expected AuthProof message",
            ));
        }

        if message.payload.len() < 128 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "AuthProof payload too short",
            ));
        }

        let pubkey = Pubkey::try_from(&message.payload[0..32]).unwrap();
        let signature = Signature::try_from(&message.payload[32..96]).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid signature")
        })?;

        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(&message.payload[96..128]);

        Ok(AuthProof {
            pubkey,
            signature,
            nonce,
        })
    }

    pub fn send_auth_ok(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        // Send an empty PaymentAck message as authentication acknowledgment
        let message = X402Message::new(X402MessageId::AuthOk, vec![]);
        write_message(stream, &message, None)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_nonce() {
        let nonce1 = AuthProof::generate_nonce();
        let nonce2 = AuthProof::generate_nonce();
        assert_ne!(nonce1, nonce2, "Nonces should be unique");
    }

    #[test]
    fn test_auth_proof() {
        let keypair = Keypair::new();
        let nonce = AuthProof::generate_nonce();
        let proof = AuthProof::create(&keypair, nonce);

        assert!(proof.verify(), "Auth proof should be valid");
    }

    #[test]
    fn test_invalid_auth_proof() {
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let nonce = AuthProof::generate_nonce();
        let proof = AuthProof::create(&keypair1, nonce);

        // Tamper with the proof by using a different public key
        let tampered_proof = AuthProof {
            pubkey: keypair2.pubkey(),
            signature: proof.signature,
            nonce: proof.nonce,
        };

        assert!(
            !tampered_proof.verify(),
            "Tampered auth proof should be invalid"
        );
    }
}
