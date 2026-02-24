use std::io::{Read, Write};
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

    pub fn send(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        let mut buf = Vec::with_capacity(96);
        buf.extend_from_slice(&self.pubkey.to_bytes());
        buf.extend_from_slice(self.signature.as_ref());
        stream.write_all(&buf)?;
        stream.write_all(&self.nonce)?;
        stream.flush()?;
        Ok(())
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

    pub fn send_auth_ok(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        let response = b"AUTH_OK";
        stream.write_all(response)?;
        stream.flush()?;
        Ok(())
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
