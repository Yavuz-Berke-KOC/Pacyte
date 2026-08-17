// ===================================================================
// PACYTE NEXUS - MEMPOOL MODÜLÜ
// ===================================================================

pub mod pool;
pub mod validator;
pub mod fee;
pub mod ordering;
pub mod pruning;

// Re-export'lar
pub use pool::*;
pub use validator::*;
pub use fee::*;
pub use ordering::*;
pub use pruning::*;

use crate::types::{PacyteError, PacyteResult, Hash, Address, Timestamp};
use crate::types::transaction::{Transaction, PooledTransaction, TxValidationResult};
use crate::types::account::Account;
use crate::storage::StateManager;
use std::sync::Arc;

// ===================================================================
// MEMPOOL TRAIT
// ===================================================================

#[async_trait::async_trait]
pub trait Mempool: Send + Sync {
    /// İşlem ekle
    async fn add_transaction(&self, tx: Transaction) -> Result<AddTxResult, PacyteError>;
    
    /// İşlem çıkar
    async fn remove_transaction(&self, hash: &Hash) -> Option<Transaction>;
    
    /// İşlem getir
    fn get_transaction(&self, hash: &Hash) -> Option<Transaction>;
    
    /// Tüm işlemleri getir
    fn get_all_transactions(&self) -> Vec<Transaction>;
    
    /// Blok için işlem seç (fee sıralı)
    async fn select_for_block(&self, max_count: usize, max_size: usize) -> Vec<Transaction>;
    
    /// İşlem sayısı
    fn size(&self) -> usize;
    
    /// Mempool'u temizle (blok işlendikten sonra)
    async fn cleanup(&self, processed_txs: &[Hash]);
    
    /// Zaman aşımına uğrayanları temizle
    async fn prune_expired(&self, current_time: Timestamp) -> usize;
    
    /// Belirli adresteki işlemleri getir
    fn get_transactions_by_address(&self, address: &Address) -> Vec<Transaction>;
    
    /// İstatistikler
    fn stats(&self) -> MempoolStats;
}

// ===================================================================
// MEMPOOL KONFİGÜRASYONU
// ===================================================================

#[derive(Debug, Clone)]
pub struct MempoolConfig {
    pub max_size: usize,
    pub max_tx_age_secs: u64,
    pub min_fee_per_byte: u64,
    pub max_tx_per_address: usize,
    pub enable_fee_priority: bool,
    pub enable_nonce_gap_filling: bool,
    pub max_nonce_gap: u64,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_size: 10000,
            max_tx_age_secs: 3600, // 1 saat
            min_fee_per_byte: 1,
            max_tx_per_address: 100,
            enable_fee_priority: true,
            enable_nonce_gap_filling: true,
            max_nonce_gap: 10,
        }
    }
}

// ===================================================================
// İŞLEM EKLEME SONUCU
// ===================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddTxResult {
    Added,
    AlreadyExists,
    Replaced { old_hash: Hash },
    Rejected { reason: String },
}

impl AddTxResult {
    pub fn is_added(&self) -> bool {
        matches!(self, Self::Added | Self::Replaced { .. })
    }
}

// ===================================================================
// MEMPOOL İSTATİSTİKLERİ
// ===================================================================

#[derive(Debug, Clone, Default)]
pub struct MempoolStats {
    pub total_transactions: usize,
    pub total_size_bytes: usize,
    pub avg_fee_per_byte: f64,
    pub oldest_tx_age_secs: u64,
    pub rejected_count: u64,
    pub replaced_count: u64,
    pub pending_by_address: std::collections::HashMap<Address, usize>,
}

impl std::fmt::Display for MempoolStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Mempool: txs={}, size={}KB, avg_fee={:.2}, oldest={}s",
            self.total_transactions,
            self.total_size_bytes / 1024,
            self.avg_fee_per_byte,
            self.oldest_tx_age_secs
        )
    }
}