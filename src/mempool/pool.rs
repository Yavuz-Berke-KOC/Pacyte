// ===================================================================
// PACYTE NEXUS - İŞLEM HAVUZU (GERÇEK)
// ===================================================================

use std::collections::{HashMap, HashSet, BTreeMap};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use dashmap::DashMap;

use crate::types::{
    PacyteError, PacyteResult, Hash, Address, Timestamp, current_timestamp,
};
use crate::types::transaction::{Transaction, PooledTransaction, TxValidationResult};
use crate::types::account::Account;
use crate::storage::StateManager;
use super::{
    Mempool, MempoolConfig, MempoolStats, AddTxResult,
    MempoolValidator, FeeCalculator, TransactionOrdering,
};

// ===================================================================
// POOLED TRANSACTION WRAPPER
// ===================================================================

#[derive(Debug, Clone)]
struct PooledTx {
    tx: Transaction,
    added_at: Timestamp,
    hash: Hash,
    size: usize,
    fee_per_byte: f64,
    priority: f64,
}

impl PooledTx {
    fn new(tx: Transaction) -> Self {
        let hash = tx.hash();
        let size = tx.size();
        let fee_per_byte = tx.fee as f64 / size as f64;
        let priority = fee_per_byte;
        
        Self {
            tx,
            added_at: current_timestamp(),
            hash,
            size,
            fee_per_byte,
            priority,
        }
    }
    
    fn age_secs(&self) -> u64 {
        current_timestamp().saturating_sub(self.added_at)
    }
}

// ===================================================================
// MEMPOOL IMPLEMENTATION
// ===================================================================

pub struct MempoolImpl {
    config: MempoolConfig,
    state: Arc<StateManager>,
    
    // Ana işlem havuzu (hash -> pooled tx)
    transactions: Arc<DashMap<Hash, PooledTx>>,
    
    // Adres başına nonce takibi (address -> pending nonces)
    pending_nonces: Arc<DashMap<Address, HashSet<u64>>>,
    
    // Adres başına işlem sayısı
    tx_count_by_address: Arc<DashMap<Address, usize>>,
    
    // Fee sıralı indeks (priority -> hash list)
    priority_index: Arc<RwLock<BTreeMap<u64, Vec<Hash>>>>,
    
    // Validator
    validator: MempoolValidator,
    
    // İstatistikler
    stats: Arc<RwLock<MempoolStats>>,
    rejected_count: Arc<RwLock<u64>>,
    replaced_count: Arc<RwLock<u64>>,
}

impl MempoolImpl {
    pub fn new(config: MempoolConfig, state: Arc<StateManager>) -> Self {
        Self {
            config: config.clone(),
            state: state.clone(),
            transactions: Arc::new(DashMap::new()),
            pending_nonces: Arc::new(DashMap::new()),
            tx_count_by_address: Arc::new(DashMap::new()),
            priority_index: Arc::new(RwLock::new(BTreeMap::new())),
            validator: MempoolValidator::new(config.clone(), state),
            stats: Arc::new(RwLock::new(MempoolStats::default())),
            rejected_count: Arc::new(RwLock::new(0)),
            replaced_count: Arc::new(RwLock::new(0)),
        }
    }
    
    /// İşlemi validate et
    async fn validate_transaction(&self, tx: &Transaction) -> Result<(), String> {
        // Temel validasyon
        if !tx.validate_basic(self.config.max_tx_age_secs) {
            return Err("Basic validation failed".to_string());
        }
        
        // İmza doğrulama
        if !self.validator.verify_signature(tx).await {
            return Err("Invalid signature".to_string());
        }
        
        // Bakiye kontrolü
        if !self.validator.check_balance(tx).await {
            return Err("Insufficient balance".to_string());
        }
        
        // Nonce kontrolü
        match self.validator.check_nonce(tx).await {
            TxValidationResult::Valid => {}
            other => return Err(other.error_message()),
        }
        
        // Fee kontrolü
        if !self.validator.check_fee(tx) {
            return Err("Fee too low".to_string());
        }
        
        Ok(())
    }
    
    /// Aynı adresten daha yüksek fee'li işlem varsa değiştir
    fn should_replace(&self, existing: &PooledTx, new: &Transaction) -> bool {
        // Aynı nonce'li işlem varsa
        if existing.tx.nonce == new.nonce {
            // Daha yüksek fee varsa değiştir
            let new_fee_per_byte = new.fee as f64 / new.size() as f64;
            new_fee_per_byte > existing.fee_per_byte * 1.1 // %10 daha yüksek
        } else {
            false
        }
    }
    
    /// Priority index'e ekle
    fn add_to_priority_index(&self, hash: Hash, priority: f64) {
        let priority_key = (priority * 1_000_000.0) as u64;
        
        let mut index = self.priority_index.write();
        index.entry(priority_key)
            .or_insert_with(Vec::new)
            .push(hash);
    }
    
    /// Priority index'ten çıkar
    fn remove_from_priority_index(&self, hash: &Hash) {
        let mut index = self.priority_index.write();
        for (_, hashes) in index.iter_mut() {
            if let Some(pos) = hashes.iter().position(|h| h == hash) {
                hashes.swap_remove(pos);
                break;
            }
        }
    }
    
    /// İstatistikleri güncelle
    fn update_stats(&self) {
        let mut stats = self.stats.write();
        stats.total_transactions = self.transactions.len();
        
        let mut total_size = 0;
        let mut total_fee_per_byte = 0.0;
        let mut oldest_age = 0u64;
        
        for entry in self.transactions.iter() {
            let tx = entry.value();
            total_size += tx.size;
            total_fee_per_byte += tx.fee_per_byte;
            oldest_age = oldest_age.max(tx.age_secs());
        }
        
        stats.total_size_bytes = total_size;
        stats.avg_fee_per_byte = if stats.total_transactions > 0 {
            total_fee_per_byte / stats.total_transactions as f64
        } else {
            0.0
        };
        stats.oldest_tx_age_secs = oldest_age;
        stats.rejected_count = *self.rejected_count.read();
        stats.replaced_count = *self.replaced_count.read();
    }
}

#[async_trait::async_trait]
impl Mempool for MempoolImpl {
    async fn add_transaction(&self, tx: Transaction) -> Result<AddTxResult, PacyteError> {
        let hash = tx.hash();
        
        // Zaten var mı?
        if self.transactions.contains_key(&hash) {
            return Ok(AddTxResult::AlreadyExists);
        }
        
        // Kapasite kontrolü
        if self.transactions.len() >= self.config.max_size {
            return Ok(AddTxResult::Rejected {
                reason: "Mempool full".to_string()
            });
        }
        
        // Adres başına işlem limiti
        let tx_count = self.tx_count_by_address
            .get(&tx.from)
            .map(|c| *c)
            .unwrap_or(0);
        
        if tx_count >= self.config.max_tx_per_address {
            return Ok(AddTxResult::Rejected {
                reason: format!("Too many pending txs from address (max: {})", 
                    self.config.max_tx_per_address)
            });
        }
        
        // Validate et
        if let Err(reason) = self.validate_transaction(&tx).await {
            *self.rejected_count.write() += 1;
            return Ok(AddTxResult::Rejected { reason });
        }
        
        // Aynı adres/nonce'li işlem var mı? (replacement)
        let mut replaced_hash = None;
        
        if let Some(pending) = self.pending_nonces.get(&tx.from) {
            if pending.contains(&tx.nonce) {
                // Mevcut işlemi bul
                for entry in self.transactions.iter() {
                    let pooled = entry.value();
                    if pooled.tx.from == tx.from && pooled.tx.nonce == tx.nonce {
                        if self.should_replace(pooled, &tx) {
                            replaced_hash = Some(pooled.hash);
                        } else {
                            return Ok(AddTxResult::Rejected {
                                reason: "Replacement fee too low".to_string()
                            });
                        }
                        break;
                    }
                }
            }
        }
        
        // Eski işlemi sil (replacement)
        if let Some(old_hash) = replaced_hash {
            self.remove_transaction(&old_hash).await;
            *self.replaced_count.write() += 1;
        }
        
        // Yeni işlemi ekle
        let pooled = PooledTx::new(tx.clone());
        
        // Nonce takibi
        self.pending_nonces
            .entry(tx.from)
            .or_insert_with(HashSet::new)
            .insert(tx.nonce);
        
        // Adres sayacı
        *self.tx_count_by_address
            .entry(tx.from)
            .or_insert(0) += 1;
        
        // Priority index'e ekle
        self.add_to_priority_index(hash, pooled.priority);
        
        // Ana havuza ekle
        self.transactions.insert(hash, pooled);
        
        // İstatistikleri güncelle
        self.update_stats();
        
        if replaced_hash.is_some() {
            Ok(AddTxResult::Replaced { old_hash: replaced_hash.unwrap() })
        } else {
            Ok(AddTxResult::Added)
        }
    }
    
    async fn remove_transaction(&self, hash: &Hash) -> Option<Transaction> {
        if let Some((_, pooled)) = self.transactions.remove(hash) {
            // Nonce takibinden çıkar
            if let Some(mut pending) = self.pending_nonces.get_mut(&pooled.tx.from) {
                pending.remove(&pooled.tx.nonce);
            }
            
            // Adres sayacını azalt
            if let Some(mut count) = self.tx_count_by_address.get_mut(&pooled.tx.from) {
                *count = count.saturating_sub(1);
            }
            
            // Priority index'ten çıkar
            self.remove_from_priority_index(hash);
            
            self.update_stats();
            
            Some(pooled.tx)
        } else {
            None
        }
    }
    
    fn get_transaction(&self, hash: &Hash) -> Option<Transaction> {
        self.transactions
            .get(hash)
            .map(|entry| entry.tx.clone())
    }
    
    fn get_all_transactions(&self) -> Vec<Transaction> {
        self.transactions
            .iter()
            .map(|entry| entry.tx.clone())
            .collect()
    }
    
    async fn select_for_block(&self, max_count: usize, max_size: usize) -> Vec<Transaction> {
        let mut selected = Vec::new();
        let mut selected_hashes = HashSet::new();
        let mut current_size = 0;
        let mut nonce_tracker: HashMap<Address, u64> = HashMap::new();
        
        // Priority index'ten sıralı olarak al
        let index = self.priority_index.read();
        
        for (_, hashes) in index.iter().rev() {
            for hash in hashes {
                if selected.len() >= max_count || current_size >= max_size {
                    break;
                }
                
                if selected_hashes.contains(hash) {
                    continue;
                }
                
                if let Some(pooled) = self.transactions.get(hash) {
                    let tx = &pooled.tx;
                    
                    // Nonce sırası kontrolü
                    let expected_nonce = nonce_tracker
                        .get(&tx.from)
                        .map(|n| n + 1)
                        .unwrap_or_else(|| {
                            // State'den mevcut nonce'i al
                            // (basitleştirilmiş)
                            0
                        });
                    
                    if tx.nonce < expected_nonce {
                        continue; // Eski nonce
                    }
                    
                    if tx.nonce > expected_nonce + self.config.max_nonce_gap {
                        continue; // Çok ileri nonce
                    }
                    
                    // Boyut kontrolü
                    if current_size + pooled.size > max_size {
                        continue;
                    }
                    
                    selected.push(tx.clone());
                    selected_hashes.insert(*hash);
                    current_size += pooled.size;
                    nonce_tracker.insert(tx.from, tx.nonce);
                }
            }
        }
        
        selected
    }
    
    fn size(&self) -> usize {
        self.transactions.len()
    }
    
    async fn cleanup(&self, processed_txs: &[Hash]) {
        for hash in processed_txs {
            self.remove_transaction(hash).await;
        }
        
        // Gereksiz nonce'leri temizle
        let mut to_remove = Vec::new();
        
        for entry in self.transactions.iter() {
            let tx = &entry.value().tx;
            
            // State'deki nonce'i kontrol et
            let state_nonce = self.state.get_nonce(&tx.from).await.unwrap_or(0);
            
            if tx.nonce <= state_nonce {
                to_remove.push(*entry.key());
            }
        }
        
        for hash in to_remove {
            self.remove_transaction(&hash).await;
        }
    }
    
    async fn prune_expired(&self, current_time: Timestamp) -> usize {
        let mut expired = Vec::new();
        
        for entry in self.transactions.iter() {
            let pooled = entry.value();
            let age = current_time.saturating_sub(pooled.added_at);
            
            if age > self.config.max_tx_age_secs {
                expired.push(*entry.key());
            }
        }
        
        let count = expired.len();
        for hash in expired {
            self.remove_transaction(&hash).await;
        }
        
        count
    }
    
    fn get_transactions_by_address(&self, address: &Address) -> Vec<Transaction> {
        self.transactions
            .iter()
            .filter(|entry| entry.value().tx.from == *address)
            .map(|entry| entry.value().tx.clone())
            .collect()
    }
    
    fn stats(&self) -> MempoolStats {
        self.stats.read().clone()
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStorage, StateManager};
    use crate::crypto::Ed25519Signer;
    use crate::types::transaction::Transaction;

    async fn setup_test_mempool() -> (Arc<MempoolImpl>, Ed25519Signer, Ed25519Signer) {
        let storage = Arc::new(MemoryStorage::new());
        let state = Arc::new(StateManager::new(storage));
        
        let config = MempoolConfig {
            max_size: 100,
            min_fee_per_byte: 1,
            ..Default::default()
        };
        
        let mempool = Arc::new(MempoolImpl::new(config, state));
        
        let alice = Ed25519Signer::generate();
        let bob = Ed25519Signer::generate();
        
        (mempool, alice, bob)
    }

    #[tokio::test]
    async fn test_add_transaction() {
        let (mempool, alice, bob) = setup_test_mempool().await;
        
        let tx = Transaction::new(
            alice.address(),
            bob.address(),
            1000,
            10,
            0,
        );
        
        let result = mempool.add_transaction(tx.clone()).await.unwrap();
        assert!(result.is_added());
        assert_eq!(mempool.size(), 1);
        
        // Aynı işlemi tekrar ekle
        let result = mempool.add_transaction(tx).await.unwrap();
        assert_eq!(result, AddTxResult::AlreadyExists);
    }
    
    #[tokio::test]
    async fn test_select_for_block() {
        let (mempool, alice, bob) = setup_test_mempool().await;
        
        // Yüksek fee'li işlem
        let tx1 = Transaction::new(alice.address(), bob.address(), 1000, 100, 0);
        
        // Düşük fee'li işlem
        let tx2 = Transaction::new(alice.address(), bob.address(), 500, 10, 1);
        
        mempool.add_transaction(tx2.clone()).await.unwrap();
        mempool.add_transaction(tx1.clone()).await.unwrap();
        
        let selected = mempool.select_for_block(10, 1024 * 1024).await;
        
        // Yüksek fee'li önce seçilmeli
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].fee, 100); // tx1 önce
    }
    
    #[tokio::test]
    async fn test_cleanup() {
        let (mempool, alice, bob) = setup_test_mempool().await;
        
        let tx = Transaction::new(alice.address(), bob.address(), 1000, 10, 0);
        let hash = tx.hash();
        
        mempool.add_transaction(tx).await.unwrap();
        assert_eq!(mempool.size(), 1);
        
        mempool.cleanup(&[hash]).await;
        assert_eq!(mempool.size(), 0);
    }
}