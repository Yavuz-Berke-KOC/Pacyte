// ===================================================================
// PACYTE NEXUS - MERKLE AĞACI (GERÇEK)
// ===================================================================

use sha3::{Digest, Sha3_256};
use std::collections::VecDeque;

use crate::types::Hash;
use crate::crypto::hash::hash_merkle_pair;

// ===================================================================
// MERKLE AĞACI
// ===================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleTree {
    /// Tüm seviyeler (yapraklar en altta)
    levels: Vec<Vec<Hash>>,
    
    /// Yaprak sayısı
    leaf_count: usize,
    
    /// Merkle kökü
    root: Hash,
}

impl MerkleTree {
    /// Yeni Merkle ağacı oluşturur (yapraklardan)
    pub fn new(leaves: &[Hash]) -> Self {
        if leaves.is_empty() {
            return Self {
                levels: vec![vec![]],
                leaf_count: 0,
                root: [0u8; 32],
            };
        }
        
        let mut levels = Vec::new();
        let mut current_level: Vec<Hash> = leaves.to_vec();
        
        // Yaprak seviyesini ekle
        levels.push(current_level.clone());
        
        // Köke kadar çık
        while current_level.len() > 1 {
            let mut next_level = Vec::new();
            
            for chunk in current_level.chunks(2) {
                if chunk.len() == 2 {
                    next_level.push(hash_merkle_pair(&chunk[0], &chunk[1]));
                } else {
                    // Tek kalan elemanı kendisiyle eşle
                    next_level.push(hash_merkle_pair(&chunk[0], &chunk[0]));
                }
            }
            
            levels.push(next_level.clone());
            current_level = next_level;
        }
        
        let root = current_level.first().copied().unwrap_or([0u8; 32]);
        
        Self {
            levels,
            leaf_count: leaves.len(),
            root,
        }
    }
    
    /// İşlemlerden Merkle ağacı oluşturur
    pub fn from_transactions<T: AsRef<[u8]>>(transactions: &[T]) -> Self {
        let leaves: Vec<Hash> = transactions
            .iter()
            .map(|tx| {
                let mut hasher = Sha3_256::new();
                hasher.update(tx.as_ref());
                hasher.finalize().into()
            })
            .collect();
        
        Self::new(&leaves)
    }
    
    /// Merkle kökünü döndürür
    pub fn root(&self) -> Hash {
        self.root
    }
    
    /// Yaprak sayısını döndürür
    pub fn leaf_count(&self) -> usize {
        self.leaf_count
    }
    
    /// Belirli bir yaprak için Merkle kanıtı oluşturur
    pub fn generate_proof(&self, leaf_index: usize) -> Option<MerkleProof> {
        if leaf_index >= self.leaf_count {
            return None;
        }
        
        let mut proof = Vec::new();
        let mut index = leaf_index;
        
        // Her seviye için kardeş hash'i ekle (son seviye kök hariç)
        for level in 0..self.levels.len() - 1 {
            let is_left = index % 2 == 0;
            let sibling_index = if is_left { index + 1 } else { index - 1 };
            
            // Sınır kontrolü
            if sibling_index < self.levels[level].len() {
                proof.push(ProofNode {
                    hash: self.levels[level][sibling_index],
                    is_left_sibling: !is_left,
                });
            } else {
                // Tek sayıda eleman varsa, son eleman kendisiyle eşlenir
                proof.push(ProofNode {
                    hash: self.levels[level][index],
                    is_left_sibling: false,
                });
            }
            
            index /= 2;
        }
        
        Some(MerkleProof {
            leaf_index,
            leaf_hash: self.levels[0][leaf_index],
            proof_nodes: proof,
            root: self.root,
        })
    }
    
    /// Ağacın geçerli olup olmadığını kontrol eder
    pub fn verify(&self) -> bool {
        if self.leaf_count == 0 {
            return self.root == [0u8; 32];
        }
        
        // Yeniden hesapla ve karşılaştır
        let recalculated = Self::new(&self.levels[0]);
        recalculated.root == self.root
    }
    
    /// Belirli bir hash'in ağaçta olup olmadığını kontrol eder
    pub fn contains(&self, hash: &Hash) -> bool {
        self.levels[0].contains(hash)
    }
    
    /// Tüm yaprakları döndürür
    pub fn leaves(&self) -> &[Hash] {
        &self.levels[0]
    }
}

// ===================================================================
// MERKLE KANITI
// ===================================================================

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MerkleProof {
    /// Kanıtlanan yaprağın indeksi
    pub leaf_index: usize,
    
    /// Yaprağın hash'i
    pub leaf_hash: Hash,
    
    /// Kanıt node'ları (yapraktan köke giden yol)
    pub proof_nodes: Vec<ProofNode>,
    
    /// Beklenen Merkle kökü
    pub root: Hash,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProofNode {
    pub hash: Hash,
    pub is_left_sibling: bool,
}

impl MerkleProof {
    /// Kanıtı doğrular
    pub fn verify(&self) -> bool {
        let mut current_hash = self.leaf_hash;
        
        for node in &self.proof_nodes {
            let (left, right) = if node.is_left_sibling {
                (node.hash, current_hash)
            } else {
                (current_hash, node.hash)
            };
            
            current_hash = hash_merkle_pair(&left, &right);
        }
        
        current_hash == self.root
    }
    
    /// Kanıt boyutunu döndürür (byte)
    pub fn size(&self) -> usize {
        self.proof_nodes.len() * 32 + 32 + 8 + 32
    }
    
    /// Kanıtı JSON'a serileştirir
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    
    /// JSON'dan kanıtı parse eder
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

// ===================================================================
// SPARSE MERKLE AĞACI (State için)
// ===================================================================

#[derive(Debug, Clone)]
pub struct SparseMerkleTree {
    pub root: Hash,
    default_hashes: Vec<Hash>, // Her seviye için varsayılan hash
}

impl SparseMerkleTree {
    /// Yeni Sparse Merkle ağacı oluşturur (256 seviye)
    pub fn new() -> Self {
        let mut default_hashes = Vec::with_capacity(257);
        let mut current = [0u8; 32];
        
        default_hashes.push(current);
        
        for _ in 0..256 {
            current = hash_merkle_pair(&current, &current);
            default_hashes.push(current);
        }
        
        Self {
            root: default_hashes[256],
            default_hashes,
        }
    }
    
    /// Anahtar-değer çiftinden kök hesaplar
    pub fn root_from_updates(&self, updates: &[(Hash, Hash)]) -> Hash {
        if updates.is_empty() {
            return self.root;
        }
        
        // Basitleştirilmiş: Sıralı güncellemeleri uygula
        let mut current_root = self.root;
        
        for (key, value) in updates {
            let mut path = *key;
            let mut node = *value;
            
            for depth in 0..256 {
                let bit = (path[31 - depth / 8] >> (7 - (depth % 8))) & 1;
                
                if bit == 0 {
                    node = hash_merkle_pair(&node, &self.default_hashes[depth]);
                } else {
                    node = hash_merkle_pair(&self.default_hashes[depth], &node);
                }
            }
            
            // XOR ile birleştir (gerçek implementasyonda daha karmaşık)
            for i in 0..32 {
                current_root[i] ^= node[i];
            }
        }
        
        current_root
    }
}

impl Default for SparseMerkleTree {
    fn default() -> Self {
        Self::new()
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_merkle_tree() {
        let tree = MerkleTree::new(&[]);
        assert_eq!(tree.root(), [0u8; 32]);
        assert_eq!(tree.leaf_count(), 0);
    }

    #[test]
    fn test_single_leaf() {
        let leaf = hash_merkle_pair(&[1u8; 32], &[1u8; 32]);
        let tree = MerkleTree::new(&[leaf]);
        
        assert_eq!(tree.root(), leaf);
        assert_eq!(tree.leaf_count(), 1);
    }

    #[test]
    fn test_two_leaves() {
        let leaf1 = [1u8; 32];
        let leaf2 = [2u8; 32];
        let tree = MerkleTree::new(&[leaf1, leaf2]);
        
        let expected_root = hash_merkle_pair(&leaf1, &leaf2);
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_four_leaves() {
        let leaves: Vec<Hash> = (0..4).map(|i| [i as u8; 32]).collect();
        let tree = MerkleTree::new(&leaves);
        
        // Manuel hesapla
        let node01 = hash_merkle_pair(&leaves[0], &leaves[1]);
        let node23 = hash_merkle_pair(&leaves[2], &leaves[3]);
        let expected_root = hash_merkle_pair(&node01, &node23);
        
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_odd_leaves() {
        let leaves: Vec<Hash> = (0..3).map(|i| [i as u8; 32]).collect();
        let tree = MerkleTree::new(&leaves);
        
        // 3 yaprak: (0,1) -> 01, (2,2) -> 22, sonra (01,22) -> root
        let node01 = hash_merkle_pair(&leaves[0], &leaves[1]);
        let node22 = hash_merkle_pair(&leaves[2], &leaves[2]);
        let expected_root = hash_merkle_pair(&node01, &node22);
        
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_merkle_proof() {
        let leaves: Vec<Hash> = (0..4).map(|i| [i as u8; 32]).collect();
        let tree = MerkleTree::new(&leaves);
        
        let proof = tree.generate_proof(2).unwrap();
        assert_eq!(proof.leaf_index, 2);
        assert_eq!(proof.leaf_hash, leaves[2]);
        assert!(proof.verify());
    }

    #[test]
    fn test_invalid_proof() {
        let leaves: Vec<Hash> = (0..4).map(|i| [i as u8; 32]).collect();
        let tree = MerkleTree::new(&leaves);
        
        let mut proof = tree.generate_proof(2).unwrap();
        
        // Kanıtı boz
        proof.proof_nodes[0].hash = [255u8; 32];
        assert!(!proof.verify());
    }

    #[test]
    fn test_proof_for_single_leaf() {
        let leaf = [42u8; 32];
        let tree = MerkleTree::new(&[leaf]);
        
        let proof = tree.generate_proof(0).unwrap();
        assert!(proof.verify());
        assert!(proof.proof_nodes.is_empty());
    }

    #[test]
    fn test_verify_tree() {
        let leaves: Vec<Hash> = (0..10).map(|i| [i as u8; 32]).collect();
        let tree = MerkleTree::new(&leaves);
        assert!(tree.verify());
    }

    #[test]
    fn test_contains() {
        let leaves: Vec<Hash> = (0..5).map(|i| [i as u8; 32]).collect();
        let tree = MerkleTree::new(&leaves);
        
        assert!(tree.contains(&leaves[2]));
        assert!(!tree.contains(&[99u8; 32]));
    }

    #[test]
    fn test_from_transactions() {
        let txs = vec![b"tx1", b"tx2", b"tx3"];
        let tree = MerkleTree::from_transactions(&txs);
        
        assert_eq!(tree.leaf_count(), 3);
        assert_ne!(tree.root(), [0u8; 32]);
    }

    #[test]
    fn test_sparse_merkle_tree() {
        let smt = SparseMerkleTree::new();
        
        let updates = vec![
            ([1u8; 32], [10u8; 32]),
            ([2u8; 32], [20u8; 32]),
        ];
        
        let root = smt.root_from_updates(&updates);
        assert_ne!(root, [0u8; 32]);
        
        // Aynı güncellemeler aynı kökü vermeli
        let root2 = smt.root_from_updates(&updates);
        assert_eq!(root, root2);
    }

    #[test]
    fn test_merkle_proof_serialization() {
        let leaves: Vec<Hash> = (0..4).map(|i| [i as u8; 32]).collect();
        let tree = MerkleTree::new(&leaves);
        let proof = tree.generate_proof(1).unwrap();
        
        let json = proof.to_json();
        let parsed = MerkleProof::from_json(&json).unwrap();
        
        assert_eq!(proof.leaf_index, parsed.leaf_index);
        assert_eq!(proof.root, parsed.root);
        assert!(parsed.verify());
    }

    #[test]
    fn test_large_tree() {
        let leaves: Vec<Hash> = (0..10000).map(|i| {
            let mut hash = [0u8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            hash
        }).collect();
        
        let tree = MerkleTree::new(&leaves);
        assert_eq!(tree.leaf_count(), 10000);
        
        // Rastgele bir yaprak için kanıt oluştur
        let proof = tree.generate_proof(5000).unwrap();
        assert!(proof.verify());
    }
}