// ===================================================================
// PACYTE NEXUS - MEMPOOL PRUNING
// ===================================================================

use std::collections::HashSet;
use std::sync::Arc;

use crate::types::{Hash, Address, Timestamp, current_timestamp};
use crate::types::transaction::Transaction;
use super::{Mempool, MempoolConfig};

// ===================================================================
// PRUNING STRATEGY
// ===================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruningStrategy {
    /// Zaman aşımına göre
    ByAge,
    /// Fee'ye göre (düşük fee'liler)
    ByFee,
    /// Nonce'e göre (eski nonce'ler)
    ByNonce,
    /// Kapasite aşımı
    ByCapacity,
}

pub struct Pruner {
    config: MempoolConfig,
}

impl Pruner {
    pub fn new(config: MempoolConfig) -> Self {
        Self { config }
    }
    
    /// Hangi işlemlerin silineceğini belirle
    pub fn select_for_pruning(
        &self,
        transactions: &[Transaction],
        current_time: Timestamp,
    ) -> Vec<Hash> {
        let mut to_prune = Vec::new();
        
        // 1. Zaman aşımı
        for tx in transactions {
            let age = current_time.saturating_sub(tx.timestamp);
            if age > self.config.max_tx_age_secs {
                to_prune.push(tx.hash());
            }
        }
        
        // 2. Düşük fee'liler (kapasite aşımı varsa)
        if transactions.len() > self.config.max_size {
            let mut sorted: Vec<_> = transactions.iter().collect();
            sorted.sort_by(|a, b| {
                let a_fee_per_byte = a.fee as f64 / a.size() as f64;
                let b_fee_per_byte = b.fee as f64 / b.size() as f64;
                a_fee_per_byte.partial_cmp(&b_fee_per_byte).unwrap()
            });
            
            let excess = transactions.len() - self.config.max_size;
            for tx in sorted.iter().take(excess) {
                to_prune.push(tx.hash());
            }
        }
        
        // 3. Adres başına limit aşımı
        let mut count_by_address: std::collections::HashMap<Address, usize> = std::collections::HashMap::new();
        
        for tx in transactions {
            *count_by_address.entry(tx.from).or_insert(0) += 1;
        }
        
        for (address, count) in count_by_address {
            if count > self.config.max_tx_per_address {
                // Bu adresten en eski işlemleri sil
                let mut address_txs: Vec<_> = transactions
                    .iter()
                    .filter(|tx| tx.from == address)
                    .collect();
                
                address_txs.sort_by_key(|tx| tx.timestamp);
                
                let excess = count - self.config.max_tx_per_address;
                for tx in address_txs.iter().take(excess) {
                    to_prune.push(tx.hash());
                }
            }
        }
        
        // Tekrarları temizle
        let unique: HashSet<_> = to_prune.into_iter().collect();
        unique.into_iter().collect()
    }
    
    /// Belirli bir adresten eski nonce'leri temizle
    pub fn prune_old_nonces(
        &self,
        transactions: &[Transaction],
        address: &Address,
        current_nonce: u64,
    ) -> Vec<Hash> {
        transactions
            .iter()
            .filter(|tx| tx.from == *address && tx.nonce <= current_nonce)
            .map(|tx| tx.hash())
            .collect()
    }
    
    /// Kapasite aşımı durumunda temizle
    pub fn prune_capacity(&self, transactions: &[Transaction]) -> Vec<Hash> {
        if transactions.len() <= self.config.max_size {
            return Vec::new();
        }
        
        let excess = transactions.len() - self.config.max_size;
        
        // Fee'ye göre sırala, en düşükleri sil
        let mut sorted: Vec<_> = transactions.iter().collect();
        sorted.sort_by(|a, b| {
            let a_priority = a.fee as f64 / a.size() as f64;
            let b_priority = b.fee as f64 / b.size() as f64;
            a_priority.partial_cmp(&b_priority).unwrap()
        });
        
        sorted.iter()
            .take(excess)
            .map(|tx| tx.hash())
            .collect()
    }
}

// ===================================================================
// BACKGROUND PRUNER
// ===================================================================

pub struct BackgroundPruner {
    mempool: Arc<dyn Mempool>,
    interval_secs: u64,
}

impl BackgroundPruner {
    pub fn new(mempool: Arc<dyn Mempool>, interval_secs: u64) -> Self {
        Self {
            mempool,
            interval_secs,
        }
    }
    
    pub async fn start(self) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                std::time::Duration::from_secs(self.interval_secs)
            );
            
            loop {
                interval.tick().await;
                
                let now = current_timestamp();
                let pruned = self.mempool.prune_expired(now).await;
                
                if pruned > 0 {
                    tracing::debug!("Pruned {} expired transactions from mempool", pruned);
                }
            }
        });
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::transaction::Transaction;

    #[test]
    fn test_select_for_pruning_age() {
        let config = MempoolConfig {
            max_tx_age_secs: 60,
            ..Default::default()
        };
        
        let pruner = Pruner::new(config);
        
        let now = current_timestamp();
        let old_tx = Transaction {
            timestamp: now - 120,
            ..Transaction::new([1u8; 32], [2u8; 32], 1000, 10, 0)
        };
        
        let new_tx = Transaction {
            timestamp: now,
            ..Transaction::new([1u8; 32], [2u8; 32], 1000, 10, 0)
        };
        
        let to_prune = pruner.select_for_pruning(&[old_tx.clone(), new_tx], now);
        
        assert_eq!(to_prune.len(), 1);
        assert_eq!(to_prune[0], old_tx.hash());
    }
    
    #[test]
    fn test_prune_capacity() {
        let config = MempoolConfig {
            max_size: 2,
            ..Default::default()
        };
        
        let pruner = Pruner::new(config);
        
        let tx1 = Transaction::new([1u8; 32], [2u8; 32], 1000, 10, 0);
        let tx2 = Transaction::new([1u8; 32], [2u8; 32], 1000, 20, 0);
        let tx3 = Transaction::new([1u8; 32], [2u8; 32], 1000, 30, 0);
        
        let txs = vec![tx1, tx2, tx3];
        let to_prune = pruner.prune_capacity(&txs);
        
        assert_eq!(to_prune.len(), 1); // 3 tane var, 2 kapasite, 1 silinmeli
    }
}