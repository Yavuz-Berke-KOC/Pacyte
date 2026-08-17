// ===================================================================
// PACYTE NEXUS - İŞLEM SIRALAMA
// ===================================================================

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::types::{Address, Hash};
use crate::types::transaction::Transaction;

// ===================================================================
// TRANSACTION ORDERING
// ===================================================================

pub struct TransactionOrdering {
    fee_priority_weight: f64,
    age_priority_weight: f64,
    size_priority_weight: f64,
}

impl Default for TransactionOrdering {
    fn default() -> Self {
        Self {
            fee_priority_weight: 0.7,
            age_priority_weight: 0.2,
            size_priority_weight: 0.1,
        }
    }
}

impl TransactionOrdering {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// İşlem için öncelik skoru hesapla
    pub fn calculate_priority(&self, fee: u128, size: usize, age_secs: u64) -> f64 {
        let fee_score = fee as f64 / size as f64;
        let age_score = (age_secs as f64 / 60.0).min(100.0); // Max 100 dakika
        let size_score = 1.0 / (size as f64).sqrt();
        
        fee_score * self.fee_priority_weight +
        age_score * self.age_priority_weight +
        size_score * self.size_priority_weight
    }
    
    /// Nonce sırasına göre sırala
    pub fn sort_by_nonce(&self, txs: &mut [Transaction]) {
        // Önce adrese göre grupla
        let mut by_address: HashMap<Address, Vec<Transaction>> = HashMap::new();
        
        for tx in txs.iter() {
            by_address.entry(tx.from)
                .or_insert_with(Vec::new)
                .push(tx.clone());
        }
        
        // Her grup için nonce'e göre sırala
        for (_, mut group) in by_address {
            group.sort_by_key(|tx| tx.nonce);
        }
        
        // Sonra global sıralama (fee + age)
        txs.sort_by(|a, b| {
            let a_priority = self.calculate_priority(a.fee, a.size(), 0);
            let b_priority = self.calculate_priority(b.fee, b.size(), 0);
            b_priority.partial_cmp(&a_priority).unwrap_or(Ordering::Equal)
        });
    }
    
    /// Bağımlılıkları çöz (nonce zinciri)
    pub fn resolve_dependencies(&self, txs: &[Transaction]) -> Vec<Vec<Transaction>> {
        let mut chains: HashMap<Address, Vec<Transaction>> = HashMap::new();
        
        for tx in txs {
            chains.entry(tx.from)
                .or_insert_with(Vec::new)
                .push(tx.clone());
        }
        
        // Her zinciri nonce'e göre sırala
        for chain in chains.values_mut() {
            chain.sort_by_key(|tx| tx.nonce);
        }
        
        // Zincirleri listeye çevir
        chains.into_values().collect()
    }
}

// ===================================================================
// PRIORITY QUEUE
// ===================================================================

struct PriorityTx {
    hash: Hash,
    priority: f64,
    timestamp: u64,
}

impl PartialEq for PriorityTx {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.timestamp == other.timestamp
    }
}

impl Eq for PriorityTx {}

impl PartialOrd for PriorityTx {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityTx {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .partial_cmp(&other.priority)
            .unwrap_or(Ordering::Equal)
            .then_with(|| self.timestamp.cmp(&other.timestamp))
    }
}

pub struct TransactionQueue {
    heap: BinaryHeap<PriorityTx>,
    ordering: TransactionOrdering,
}

impl TransactionQueue {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            ordering: TransactionOrdering::new(),
        }
    }
    
    pub fn push(&mut self, hash: Hash, fee: u128, size: usize, timestamp: u64) {
        let priority = self.ordering.calculate_priority(fee, size, 0);
        self.heap.push(PriorityTx {
            hash,
            priority,
            timestamp,
        });
    }
    
    pub fn pop(&mut self) -> Option<Hash> {
        self.heap.pop().map(|pt| pt.hash)
    }
    
    pub fn peek(&self) -> Option<&Hash> {
        self.heap.peek().map(|pt| &pt.hash)
    }
    
    pub fn len(&self) -> usize {
        self.heap.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
    
    pub fn clear(&mut self) {
        self.heap.clear();
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_calculation() {
        let ordering = TransactionOrdering::new();
        
        let p1 = ordering.calculate_priority(1000, 250, 0);
        let p2 = ordering.calculate_priority(100, 250, 0);
        
        // Yüksek fee daha yüksek öncelik
        assert!(p1 > p2);
    }
    
    #[test]
    fn test_transaction_queue() {
        let mut queue = TransactionQueue::new();
        
        queue.push([1u8; 32], 1000, 250, 1);
        queue.push([2u8; 32], 100, 250, 2);
        queue.push([3u8; 32], 500, 250, 3);
        
        assert_eq!(queue.len(), 3);
        
        // En yüksek öncelikli önce çıkmalı
        let first = queue.pop().unwrap();
        assert_eq!(first, [1u8; 32]);
        
        let second = queue.pop().unwrap();
        assert_eq!(second, [3u8; 32]);
        
        let third = queue.pop().unwrap();
        assert_eq!(third, [2u8; 32]);
    }
}