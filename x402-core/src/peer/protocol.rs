use svix_ksuid::KsuidMs;

/// Type alias for peer ID (20 bytes encoded as KSUID)
pub type PeerId = KsuidMs;

/// Type alias for public key (32 bytes for Ed25519)
pub type PubKey = [u8; 32];

/// Type alias for signature (64 bytes for Ed25519)
pub type Signature = [u8; 64];

/// Type alias for hash (32 bytes for SHA256)
pub type Hash = [u8; 32];

/// Proof structure for piece chunk verification
#[derive(Debug, Clone, PartialEq)]
pub struct Proof {
    /// Hash of the piece chunk
    pub hash: Hash,
    /// Additional verification data (Merkle path)
    pub merkle_path: Vec<Hash>,
}

impl Proof {
    /// Create a new proof
    pub fn new(hash: Hash, merkle_path: Vec<Hash>) -> Self {
        Self { hash, merkle_path }
    }

    /// Verify the proof against expected data
    pub fn verify(&self, data: &[u8]) -> bool {
        use sha2::{Sha256, Digest};
        let computed_hash = Sha256::digest(data);
        computed_hash.as_slice() == self.hash.as_slice()
    }
}

/// X402 protocol messages exchanged between peers
#[derive(Debug, Clone, PartialEq)]
pub enum X402Message {
    /// Initial handshake with peer ID and public key
    Handshake {
        peer_id: PeerId,
        pubkey: PubKey,
    },
    
    /// Authentication proof via signature
    AuthProof {
        signature: Signature,
    },
    
    /// Request price for a specific piece
    PriceInquiry {
        piece_id: u32,
    },
    
    /// Offer price for a specific piece
    PriceOffer {
        piece_id: u32,
        fee: u64,
    },
    
    /// Payment locked with hashlock (HTLC-style)
    LockedPayment {
        piece_id: u32,
        amount: u64,
        hashlock: Hash,
    },
    
    /// Request a specific block of a piece
    RequestBlock {
        piece_id: u32,
        block: u32,
    },
    
    /// Piece chunk data with proof
    PieceChunk {
        block: u32,
        data: Vec<u8>,
        proof: Proof,
    },
    
    /// Reveal preimage to unlock payment
    PaymentReveal {
        preimage: Hash,
    },
    
    /// Acknowledge payment received
    PaymentAck,
    
    /// Announce having a piece
    Have {
        piece_id: u32,
    },
}

impl X402Message {
    /// Get the message type as a byte identifier
    pub fn message_type(&self) -> u8 {
        match self {
            X402Message::Handshake { .. } => 0,
            X402Message::AuthProof { .. } => 1,
            X402Message::PriceInquiry { .. } => 2,
            X402Message::PriceOffer { .. } => 3,
            X402Message::LockedPayment { .. } => 4,
            X402Message::RequestBlock { .. } => 5,
            X402Message::PieceChunk { .. } => 6,
            X402Message::PaymentReveal { .. } => 7,
            X402Message::PaymentAck => 8,
            X402Message::Have { .. } => 9,
        }
    }

    /// Create a new Handshake message
    pub fn new_handshake(peer_id: PeerId, pubkey: PubKey) -> Self {
        X402Message::Handshake { peer_id, pubkey }
    }

    /// Create a new AuthProof message
    pub fn new_auth_proof(signature: Signature) -> Self {
        X402Message::AuthProof { signature }
    }

    /// Create a new PriceInquiry message
    pub fn new_price_inquiry(piece_id: u32) -> Self {
        X402Message::PriceInquiry { piece_id }
    }

    /// Create a new PriceOffer message
    pub fn new_price_offer(piece_id: u32, fee: u64) -> Self {
        X402Message::PriceOffer { piece_id, fee }
    }

    /// Create a new LockedPayment message
    pub fn new_locked_payment(piece_id: u32, amount: u64, hashlock: Hash) -> Self {
        X402Message::LockedPayment {
            piece_id,
            amount,
            hashlock,
        }
    }

    /// Create a new RequestBlock message
    pub fn new_request_block(piece_id: u32, block: u32) -> Self {
        X402Message::RequestBlock { piece_id, block }
    }

    /// Create a new PieceChunk message
    pub fn new_piece_chunk(block: u32, data: Vec<u8>, proof: Proof) -> Self {
        X402Message::PieceChunk { block, data, proof }
    }

    /// Create a new PaymentReveal message
    pub fn new_payment_reveal(preimage: Hash) -> Self {
        X402Message::PaymentReveal { preimage }
    }

    /// Create a new PaymentAck message
    pub fn new_payment_ack() -> Self {
        X402Message::PaymentAck
    }

    /// Create a new Have message
    pub fn new_have(piece_id: u32) -> Self {
        X402Message::Have { piece_id }
    }

    /// Verify if a preimage matches a hashlock
    pub fn verify_hashlock(preimage: &Hash, hashlock: &Hash) -> bool {
        use sha2::{Sha256, Digest};
        let computed_hash = Sha256::digest(preimage);
        computed_hash.as_slice() == hashlock.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_types() {
        let peer_id = PeerId::new(None, None);
        let pubkey = [0u8; 32];
        let msg = X402Message::new_handshake(peer_id, pubkey);
        assert_eq!(msg.message_type(), 0);

        let msg = X402Message::new_price_inquiry(1);
        assert_eq!(msg.message_type(), 2);

        let msg = X402Message::new_payment_ack();
        assert_eq!(msg.message_type(), 8);
    }

    #[test]
    fn test_hashlock_verification() {
        let preimage = [1u8; 32];
        use sha2::{Sha256, Digest};
        let hashlock: Hash = Sha256::digest(&preimage).into();
        
        assert!(X402Message::verify_hashlock(&preimage, &hashlock));
        
        let wrong_preimage = [2u8; 32];
        assert!(!X402Message::verify_hashlock(&wrong_preimage, &hashlock));
    }

    #[test]
    fn test_proof_verification() {
        use sha2::{Sha256, Digest};
        let data = b"test data";
        let hash: Hash = Sha256::digest(data).into();
        let proof = Proof::new(hash, vec![]);
        
        assert!(proof.verify(data));
        assert!(!proof.verify(b"wrong data"));
    }
}
