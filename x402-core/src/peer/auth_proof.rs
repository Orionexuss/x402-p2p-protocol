use std::io::Read;
use std::net::TcpStream;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};

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

    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 128 {
            return None;
        }

        let pubkey = Pubkey::try_from(&data[0..32]).unwrap();
        let signature = Signature::try_from(&data[32..96]).ok()?;
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(&data[96..128]);

        Some(AuthProof {
            pubkey,
            signature,
            nonce,
        })
    }

    pub fn receive(stream: &mut TcpStream) -> std::io::Result<Self> {
        let mut buf = [0u8; 96]; // 32 bytes pubkey + 64 bytes signature
        stream.read_exact(&mut buf)?;

        let pubkey = Pubkey::try_from(&buf[0..32]).unwrap();
        let signature = Signature::try_from(&buf[32..96]).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid signature")
        })?;

        // For simplicity, we assume the nonce is sent separately after the proof
        let mut nonce_buf = [0u8; 32];
        stream.read_exact(&mut nonce_buf)?;

        Ok(AuthProof {
            pubkey,
            signature,
            nonce: nonce_buf,
        })
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
