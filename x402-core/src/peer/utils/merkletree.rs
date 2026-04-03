use sha2::{Digest, Sha256};

pub fn hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

pub struct MerkleTree {
    pub layers: Vec<Vec<[u8; 32]>>,
}

impl MerkleTree {
    pub fn new(leaves: Vec<[u8; 32]>) -> Self {
        let mut layers = vec![leaves];

        while layers.last().unwrap().len() > 1 {
            let current = layers.last().unwrap();
            let mut next_layer = vec![];

            for i in (0..current.len()).step_by(2) {
                let left = current[i];
                let right = if i + 1 < current.len() {
                    current[i + 1]
                } else {
                    current[i] // duplicate the last hash if we have an odd number of nodes
                };

                let mut combined = vec![];
                combined.extend_from_slice(&left);
                combined.extend_from_slice(&right);

                next_layer.push(hash(&combined));
            }

            layers.push(next_layer);
        }

        Self { layers }
    }

    pub fn root(&self) -> [u8; 32] {
        self.layers.last().unwrap()[0]
    }
}

pub fn get_proof(tree: &MerkleTree, mut index: usize) -> Vec<[u8; 32]> {
    let mut proof = vec![];

    for layer in &tree.layers {
        if layer.len() == 1 {
            break;
        }

        let sibling = if index.is_multiple_of(2) {
            if index + 1 < layer.len() {
                layer[index + 1]
            } else {
                layer[index]
            }
        } else {
            layer[index - 1]
        };

        proof.push(sibling);

        index /= 2;
    }

    proof
}

pub fn verify_proof(
    leaf: [u8; 32],
    proof: Vec<[u8; 32]>,
    mut index: usize,
    root: [u8; 32],
) -> bool {
    let mut computed = leaf;

    for sibling in proof {
        let mut combined = vec![];

        if index.is_multiple_of(2) {
            combined.extend_from_slice(&computed);
            combined.extend_from_slice(&sibling);
        } else {
            combined.extend_from_slice(&sibling);
            combined.extend_from_slice(&computed);
        }

        computed = hash(&combined);
        index /= 2;
    }

    computed == root
}
