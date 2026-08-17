use crate::types::transaction::Transaction;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Address, BlockHeight, Hash, Signature, Timestamp};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockHeader {
    pub height: BlockHeight,
    pub previous_hash: Hash,
    pub transactions_root: Hash,
    pub state_root: Hash,
    pub timestamp: Timestamp,
    pub proposer: Address,
    pub transaction_count: u32,
    pub block_size: u32,
    pub signature: Signature,
}

impl BlockHeader {
    pub fn hash(&self) -> Hash {
        let mut hasher = Sha3_256::new();
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.previous_hash);
        hasher.update(&self.transactions_root);
        hasher.update(&self.state_root);
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.proposer);
        hasher.update(&self.transaction_count.to_le_bytes());
        hasher.update(&self.block_size.to_le_bytes());
        hasher.finalize().into()
    }
    pub fn signing_hash(&self) -> Hash { self.hash() }
    pub fn validate(&self, parent: &BlockHeader) -> bool {
        if self.height != parent.height + 1 { return false; }
        if self.previous_hash != parent.hash() { return false; }
        let now = current_timestamp();
        if self.timestamp > now + 30 || self.timestamp < parent.timestamp { return false; }
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockBody {
    pub transactions: Vec<Transaction>,
}

impl BlockBody {
    pub fn transactions_root(&self) -> Hash {
        if self.transactions.is_empty() { return [0u8; 32]; }
        let mut hashes: Vec<Hash> = self.transactions.iter().map(|tx| tx.hash()).collect();
        while hashes.len() > 1 {
            let mut next = Vec::new();
            for chunk in hashes.chunks(2) {
                let mut hasher = Sha3_256::new();
                hasher.update(&chunk[0]);
                if chunk.len() > 1 { hasher.update(&chunk[1]); } else { hasher.update(&chunk[0]); }
                next.push(hasher.finalize().into());
            }
            hashes = next;
        }
        hashes[0]
    }
    pub fn total_fees(&self) -> u128 { self.transactions.iter().map(|tx| tx.fee as u128).sum() }
    pub fn size(&self) -> usize { bincode::serialize(self).map(|v| v.len()).unwrap_or(0) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Block {
    pub header: BlockHeader,
    pub body: BlockBody,
}

impl Block {
    pub fn new(height: BlockHeight, previous_hash: Hash, transactions: Vec<Transaction>, proposer: Address) -> Self {
        let body = BlockBody { transactions };
        let header = BlockHeader {
            height,
            previous_hash,
            transactions_root: body.transactions_root(),
            state_root: [0u8; 32],
            timestamp: current_timestamp(),
            proposer,
            transaction_count: body.transactions.len() as u32,
            block_size: body.size() as u32,
            signature: Vec::new(),
        };
        Self { header, body }
    }
    pub fn genesis() -> Self { Self::new(0, [0u8; 32], Vec::new(), [0u8; 32]) }
    pub fn hash(&self) -> Hash { self.header.hash() }
    pub fn set_state_root(&mut self, root: Hash) { self.header.state_root = root; }
    pub fn sign(&mut self, signature: Signature) { self.header.signature = signature; }
    pub fn validate(&self, parent: &Block) -> bool {
        self.header.validate(&parent.header) &&
        self.header.transactions_root == self.body.transactions_root() &&
        self.header.block_size == self.body.size() as u32 &&
        self.header.transaction_count == self.body.transactions.len() as u32
    }
    pub fn to_json(&self) -> String { serde_json::to_string_pretty(self).unwrap_or_default() }
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> { serde_json::from_str(json) }
}

fn current_timestamp() -> Timestamp {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}