use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use super::{Address, Hash, Signature, Timestamp};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub from: Address,
    pub to: Address,
    pub amount: u128,
    pub fee: u128,
    pub nonce: u64,
    pub signature: Signature,
    pub timestamp: Timestamp,
}

impl Transaction {
    pub fn new(from: Address, to: Address, amount: u128, fee: u128, nonce: u64) -> Self {
        Self { from, to, amount, fee, nonce, signature: Vec::new(), timestamp: current_timestamp() }
    }
    pub fn sighash(&self) -> Hash {
        let mut hasher = Sha3_256::new();
        hasher.update(&self.from);
        hasher.update(&self.to);
        hasher.update(&self.amount.to_le_bytes());
        hasher.update(&self.fee.to_le_bytes());
        hasher.update(&self.nonce.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.finalize().into()
    }
    pub fn hash(&self) -> Hash {
        let mut hasher = Sha3_256::new();
        hasher.update(&self.sighash());
        hasher.update(&self.signature);
        hasher.finalize().into()
    }
    pub fn sign(&mut self, signature: Signature) { self.signature = signature; }
    pub fn validate_basic(&self, max_tx_age_secs: u64) -> bool {
        if self.from == [0u8; 32] || self.to == [0u8; 32] || self.from == self.to { return false; }
        if self.amount == 0 || self.amount.checked_add(self.fee).is_none() { return false; }
        let now = current_timestamp();
        if self.timestamp > now + 30 || now - self.timestamp > max_tx_age_secs { return false; }
        !self.signature.is_empty() && self.signature.len() <= 5000
    }
    pub fn size(&self) -> usize { bincode::serialize(self).map(|v| v.len()).unwrap_or(0) }
    pub fn total_cost(&self) -> u128 { self.amount.saturating_add(self.fee) }
    pub fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> { serde_json::from_str(json) }
}

#[derive(Debug, Clone)]
pub struct PooledTransaction {
    pub tx: Transaction,
    pub added_at: Timestamp,
    pub hash: Hash,
    pub priority: f64,
}

impl PooledTransaction {
    pub fn new(tx: Transaction) -> Self {
        let hash = tx.hash();
        let priority = tx.fee as f64 / tx.size() as f64;
        Self { tx, added_at: current_timestamp(), hash, priority }
    }
    pub fn age_secs(&self) -> u64 { current_timestamp().saturating_sub(self.added_at) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxValidationResult {
    Valid,
    InsufficientBalance { required: u128, available: u128 },
    InvalidNonce { expected: u64, got: u64 },
    InvalidSignature,
    Expired { age_secs: u64, max_age_secs: u64 },
    TooLarge { size: usize, max_size: usize },
    FeeTooLow { fee: u128, min_fee: u128 },
    AccountDormant { years: u32 },
    Other(String),
}

impl TxValidationResult {
    pub fn is_valid(&self) -> bool {
        matches!(self, TxValidationResult::Valid)
    }
    
    pub fn error_message(&self) -> String {
        match self {
            TxValidationResult::Valid => "Valid".to_string(),
            TxValidationResult::InsufficientBalance { required, available } => {
                format!("Insufficient balance: required={}, available={}", required, available)
            }
            TxValidationResult::InvalidNonce { expected, got } => {
                format!("Invalid nonce: expected={}, got={}", expected, got)
            }
            TxValidationResult::InvalidSignature => "Invalid signature".to_string(),
            TxValidationResult::Expired { age_secs, max_age_secs } => {
                format!("Expired: age={}s, max={}s", age_secs, max_age_secs)
            }
            TxValidationResult::TooLarge { size, max_size } => {
                format!("Too large: size={}, max={}", size, max_size)
            }
            TxValidationResult::FeeTooLow { fee, min_fee } => {
                format!("Fee too low: fee={}, min={}", fee, min_fee)
            }
            TxValidationResult::AccountDormant { years } => {
                format!("Account dormant for {} years", years)
            }
            TxValidationResult::Other(msg) => msg.clone(),
        }
    }
}


fn current_timestamp() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}