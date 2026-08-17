// ===================================================================
// PACYTE NEXUS - MEMORY CACHE
// ===================================================================

use std::collections::HashMap;
use std::hash::Hash;                    // ← trait olarak (generic bound için)
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::types::{BlockHeight, Address};
use crate::types::Hash as BlockHash;   // ← type alias (HashMap key için)
use crate::types::block::Block;
use crate::types::transaction::Transaction;
use crate::types::account::Account;

// ===================================================================
// CACHE TRAIT
// ===================================================================

pub trait Cache<K, V>: Send + Sync
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    fn get(&self, key: &K) -> Option<V>;
    fn put(&self, key: K, value: V);
    fn remove(&self, key: &K);
    fn clear(&self);
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn contains_key(&self, key: &K) -> bool;
    fn hit_rate(&self) -> f64;
}

// ===================================================================
// LRU CACHE
// ===================================================================

struct LruEntry<V> {
    value: V,
    last_access: Instant,
}

pub struct LruCache<K, V> {
    max_size: usize,
    ttl: Option<Duration>,
    map: Arc<RwLock<HashMap<K, LruEntry<V>>>>,
    hits: Arc<RwLock<u64>>,
    misses: Arc<RwLock<u64>>,
}

impl<K, V> LruCache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            ttl: None,
            map: Arc::new(RwLock::new(HashMap::new())),
            hits: Arc::new(RwLock::new(0)),
            misses: Arc::new(RwLock::new(0)),
        }
    }
    
    pub fn with_ttl(max_size: usize, ttl: Duration) -> Self {
        Self {
            max_size,
            ttl: Some(ttl),
            map: Arc::new(RwLock::new(HashMap::new())),
            hits: Arc::new(RwLock::new(0)),
            misses: Arc::new(RwLock::new(0)),
        }
    }
    
    fn evict_expired(&self) {
        let mut map = self.map.write();
        let now = Instant::now();
        
        if let Some(ttl) = self.ttl {
            map.retain(|_, entry| now.duration_since(entry.last_access) < ttl);
        }
    }
    
    fn evict_lru(&self) {
        let mut map = self.map.write();
        
        if map.len() > self.max_size {
            // En eski erişilenleri bul ve sil
            let mut keys_to_remove: Vec<K> = Vec::new();
{
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by_key(|(_, entry)| entry.last_access);
    let to_remove = entries.len().saturating_sub(self.max_size);
    for (key, _) in entries.iter().take(to_remove) {
        keys_to_remove.push((*key).clone());
    }
}
for key in keys_to_remove {
    map.remove(&key);
}
}
    }
}

impl<K, V> Cache<K, V> for LruCache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    fn get(&self, key: &K) -> Option<V> {
        self.evict_expired();
        
        let mut map = self.map.write();
        if let Some(entry) = map.get_mut(key) {
            entry.last_access = Instant::now();
            *self.hits.write() += 1;
            Some(entry.value.clone())
        } else {
            *self.misses.write() += 1;
            None
        }
    }
    
    fn put(&self, key: K, value: V) {
        self.evict_expired();
        
        {
            let mut map = self.map.write();
            map.insert(key, LruEntry {
                value,
                last_access: Instant::now(),
            });
        }
        
        self.evict_lru();
    }
    
    fn remove(&self, key: &K) {
        self.map.write().remove(key);
    }
    
    fn clear(&self) {
        self.map.write().clear();
    }
    
    fn len(&self) -> usize {
        self.map.read().len()
    }
    
    fn contains_key(&self, key: &K) -> bool {
        self.evict_expired();
        self.map.read().contains_key(key)
    }
    
    fn hit_rate(&self) -> f64 {
        let hits = *self.hits.read() as f64;
        let misses = *self.misses.read() as f64;
        let total = hits + misses;
        
        if total > 0.0 {
            hits / total
        } else {
            0.0
        }
    }
}

// ===================================================================
// TYPED CACHES
// ===================================================================

pub struct BlockCache {
    cache: LruCache<BlockHeight, Block>,
    hash_index: Arc<RwLock<HashMap<BlockHash, BlockHeight>>>,
}

impl BlockCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: LruCache::new(max_size),
            hash_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn get(&self, height: BlockHeight) -> Option<Block> {
        self.cache.get(&height)
    }
    
    pub fn get_by_hash(&self, hash: &BlockHash) -> Option<Block> {
        let height = self.hash_index.read().get(hash).copied()?;
        self.cache.get(&height)
    }
    
    pub fn put(&self, block: Block) {
        let height = block.header.height;
        let hash = block.hash();
        
        self.hash_index.write().insert(hash, height);
        self.cache.put(height, block);
    }
    
    pub fn remove(&self, height: BlockHeight) {
        if let Some(block) = self.cache.get(&height) {
            self.hash_index.write().remove(&block.hash());
        }
        self.cache.remove(&height);
    }
    
    pub fn clear(&self) {
        self.cache.clear();
        self.hash_index.write().clear();
    }
    
    pub fn len(&self) -> usize {
        self.cache.len()
    }
    
    pub fn contains(&self, height: BlockHeight) -> bool {
        self.cache.contains_key(&height)
    }
}

pub struct TransactionCache {
    cache: LruCache<BlockHash, Transaction>,
}

impl TransactionCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: LruCache::new(max_size),
        }
    }
    
    pub fn get(&self, hash: &BlockHash) -> Option<Transaction> {
        self.cache.get(hash)
    }
    
    pub fn put(&self, tx: Transaction) {
        self.cache.put(tx.hash(), tx);
    }
    
    pub fn remove(&self, hash: &BlockHash) {
        self.cache.remove(hash);
    }
    
    pub fn clear(&self) {
        self.cache.clear();
    }
    
    pub fn len(&self) -> usize {
        self.cache.len()
    }
}

pub struct AccountCache {
    cache: LruCache<Address, Account>,
}

impl AccountCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: LruCache::new(max_size),
        }
    }
    
    pub fn get(&self, address: &Address) -> Option<Account> {
        self.cache.get(address)
    }
    
    pub fn put(&self, address: Address, account: Account) {
        self.cache.put(address, account);
    }
    
    pub fn remove(&self, address: &Address) {
        self.cache.remove(address);
    }
    
    pub fn clear(&self) {
        self.cache.clear();
    }
    
    pub fn len(&self) -> usize {
        self.cache.len()
    }
}

// ===================================================================
// UNIFIED CACHE MANAGER
// ===================================================================

pub struct CacheManager {
    pub blocks: BlockCache,
    pub transactions: TransactionCache,
    pub accounts: AccountCache,
    config: CacheConfig,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub block_cache_size: usize,
    pub transaction_cache_size: usize,
    pub account_cache_size: usize,
    pub ttl_seconds: Option<u64>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            block_cache_size: 1000,
            transaction_cache_size: 10000,
            account_cache_size: 100000,
            ttl_seconds: Some(3600),
        }
    }
}

impl CacheManager {
    pub fn new(config: CacheConfig) -> Self {
        let ttl = config.ttl_seconds.map(Duration::from_secs);
        
        Self {
            blocks: BlockCache::new(config.block_cache_size),
            transactions: TransactionCache::new(config.transaction_cache_size),
            accounts: AccountCache::new(config.account_cache_size),
            config,
        }
    }
    
    pub fn clear_all(&self) {
        self.blocks.clear();
        self.transactions.clear();
        self.accounts.clear();
    }
    
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            blocks_cached: self.blocks.len(),
            transactions_cached: self.transactions.len(),
            accounts_cached: self.accounts.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub blocks_cached: usize,
    pub transactions_cached: usize,
    pub accounts_cached: usize,
}

impl std::fmt::Display for CacheStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Cache: blocks={}, txs={}, accounts={}",
            self.blocks_cached,
            self.transactions_cached,
            self.accounts_cached
        )
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache() {
        let cache = LruCache::<String, i32>::new(2);
        
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);
        
        assert_eq!(cache.get(&"a".to_string()), Some(1));
        assert_eq!(cache.get(&"b".to_string()), Some(2));
        
        // Kapasite aşımı - en eski silinmeli
        cache.put("c".to_string(), 3);
        assert_eq!(cache.get(&"a".to_string()), None); // a silindi
        assert_eq!(cache.get(&"b".to_string()), Some(2));
        assert_eq!(cache.get(&"c".to_string()), Some(3));
    }
    
    #[test]
    fn test_block_cache() {
        let cache = BlockCache::new(10);
        
        let block = Block::genesis();
        cache.put(block.clone());
        
        assert_eq!(cache.len(), 1);
        assert!(cache.contains(0));
        
        let retrieved = cache.get(0).unwrap();
        assert_eq!(retrieved.hash(), block.hash());
        
        let by_hash = cache.get_by_hash(&block.hash()).unwrap();
        assert_eq!(by_hash.hash(), block.hash());
    }
    
    #[test]
    fn test_cache_stats() {
        let config = CacheConfig::default();
        let manager = CacheManager::new(config);
        
        let stats = manager.stats();
        assert_eq!(stats.blocks_cached, 0);
        assert_eq!(stats.transactions_cached, 0);
        assert_eq!(stats.accounts_cached, 0);
    }
}