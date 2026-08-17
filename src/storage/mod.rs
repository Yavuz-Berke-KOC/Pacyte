// ===================================================================
// PACYTE NEXUS - STORAGE MODÜLÜ
// ===================================================================

pub mod rocksdb;
pub mod state;
pub mod wal;
pub mod cache;
pub mod snapshot;
pub mod migration;

// Re-export'lar
pub use rocksdb::*;
pub use state::*;
pub use wal::*;
pub use cache::*;
pub use snapshot::*;
pub use migration::*;

use crate::types::{PacyteError, PacyteResult, Hash, BlockHeight, Address};
use crate::types::block::Block;
use crate::types::transaction::Transaction;
use crate::types::account::Account;
use std::path::PathBuf;
use std::sync::Arc;

// ===================================================================
// STORAGE TRAIT'LERİ
// ===================================================================

/// Ana storage trait'i
#[async_trait::async_trait]
pub trait Storage: Send + Sync {
    // Blok işlemleri
    async fn save_block(&self, block: &Block) -> PacyteResult<()>;
    async fn get_block(&self, height: BlockHeight) -> PacyteResult<Option<Block>>;
    async fn get_block_by_hash(&self, hash: &Hash) -> PacyteResult<Option<Block>>;
    async fn get_latest_block(&self) -> PacyteResult<Option<Block>>;
    async fn get_block_height(&self) -> PacyteResult<BlockHeight>;
    
    // İşlem işlemleri
    async fn save_transaction(&self, tx: &Transaction) -> PacyteResult<()>;
    async fn get_transaction(&self, hash: &Hash) -> PacyteResult<Option<Transaction>>;
    async fn get_transactions_by_block(&self, height: BlockHeight) -> PacyteResult<Vec<Transaction>>;
    
    // Hesap işlemleri
    async fn save_account(&self, address: &Address, account: &Account) -> PacyteResult<()>;
    async fn get_account(&self, address: &Address) -> PacyteResult<Option<Account>>;
    async fn delete_account(&self, address: &Address) -> PacyteResult<()>;
    async fn account_exists(&self, address: &Address) -> PacyteResult<bool>;
    
    // State root işlemleri
    async fn save_state_root(&self, height: BlockHeight, root: &Hash) -> PacyteResult<()>;
    async fn get_state_root(&self, height: BlockHeight) -> PacyteResult<Option<Hash>>;
    
    // Batch işlemler
    async fn write_batch(&self, batch: WriteBatch) -> PacyteResult<()>;
    
    // Snapshot
    async fn create_snapshot(&self, path: &PathBuf) -> PacyteResult<()>;
    async fn restore_from_snapshot(&self, path: &PathBuf) -> PacyteResult<()>;
    
    // İstatistikler
    fn stats(&self) -> StorageStats;
    
    // Kapatma
    async fn close(&self) -> PacyteResult<()>;
}

// ===================================================================
// WRITE BATCH
// ===================================================================

#[derive(Debug, Default, Clone)]
pub struct WriteBatch {
    pub blocks: Vec<(BlockHeight, Block)>,
    pub transactions: Vec<(Hash, Transaction)>,
    pub accounts: Vec<(Address, Option<Account>)>, // None = delete
    pub state_roots: Vec<(BlockHeight, Hash)>,
}

impl WriteBatch {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty() && 
        self.transactions.is_empty() && 
        self.accounts.is_empty() && 
        self.state_roots.is_empty()
    }
    
    pub fn len(&self) -> usize {
        self.blocks.len() + 
        self.transactions.len() + 
        self.accounts.len() + 
        self.state_roots.len()
    }
    
    pub fn add_block(&mut self, block: Block) {
        self.blocks.push((block.header.height, block));
    }
    
    pub fn add_transaction(&mut self, tx: Transaction) {
        self.transactions.push((tx.hash(), tx));
    }
    
    pub fn add_account(&mut self, address: Address, account: Account) {
        self.accounts.push((address, Some(account)));
    }
    
    pub fn delete_account(&mut self, address: Address) {
        self.accounts.push((address, None));
    }
    
    pub fn add_state_root(&mut self, height: BlockHeight, root: Hash) {
        self.state_roots.push((height, root));
    }
    
    pub fn merge(&mut self, other: WriteBatch) {
        self.blocks.extend(other.blocks);
        self.transactions.extend(other.transactions);
        self.accounts.extend(other.accounts);
        self.state_roots.extend(other.state_roots);
    }
    
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.transactions.clear();
        self.accounts.clear();
        self.state_roots.clear();
    }
}

// ===================================================================
// STORAGE İSTATİSTİKLERİ
// ===================================================================

#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    pub total_blocks: u64,
    pub total_transactions: u64,
    pub total_accounts: u64,
    pub disk_usage_bytes: u64,
    pub cache_hit_rate: f64,
    pub avg_read_latency_us: u64,
    pub avg_write_latency_us: u64,
}

impl std::fmt::Display for StorageStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Storage Stats: blocks={}, txs={}, accounts={}, disk={:.2}MB, cache_hit={:.1}%",
            self.total_blocks,
            self.total_transactions,
            self.total_accounts,
            self.disk_usage_bytes as f64 / 1_048_576.0,
            self.cache_hit_rate * 100.0
        )
    }
}

// ===================================================================
// STORAGE FABRİKASI
// ===================================================================

pub struct StorageFactory;

impl StorageFactory {
    pub fn create_rocksdb(path: PathBuf, config: StorageConfig) -> PacyteResult<Arc<dyn Storage>> {
        RocksDBStorage::new(path, config).map(|s| Arc::new(s) as Arc<dyn Storage>)
    }
    
    pub fn create_memory() -> Arc<dyn Storage> {
        Arc::new(MemoryStorage::new())
    }
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub cache_size_mb: usize,
    pub max_open_files: i32,
    pub wal_enabled: bool,
    pub compression: CompressionType,
    pub sync_writes: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            cache_size_mb: 512,
            max_open_files: 1000,
            wal_enabled: true,
            compression: CompressionType::Lz4,
            sync_writes: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionType {
    None,
    Snappy,
    Lz4,
    Zstd,
}

// ===================================================================
// MEMORY STORAGE (Testler için)
// ===================================================================

pub struct MemoryStorage {
    blocks: parking_lot::RwLock<Vec<Block>>,
    transactions: dashmap::DashMap<Hash, Transaction>,
    accounts: dashmap::DashMap<Address, Account>,
    state_roots: dashmap::DashMap<BlockHeight, Hash>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            blocks: parking_lot::RwLock::new(Vec::new()),
            transactions: dashmap::DashMap::new(),
            accounts: dashmap::DashMap::new(),
            state_roots: dashmap::DashMap::new(),
        }
    }
}

#[async_trait::async_trait]
impl Storage for MemoryStorage {
    async fn save_block(&self, block: &Block) -> PacyteResult<()> {
        let mut blocks = self.blocks.write();
        if block.header.height as usize >= blocks.len() {
            blocks.resize(block.header.height as usize + 1, Block::genesis());
        }
        blocks[block.header.height as usize] = block.clone();
        Ok(())
    }
    
    async fn get_block(&self, height: BlockHeight) -> PacyteResult<Option<Block>> {
        let blocks = self.blocks.read();
        Ok(blocks.get(height as usize).cloned())
    }
    
    async fn get_block_by_hash(&self, hash: &Hash) -> PacyteResult<Option<Block>> {
        let blocks = self.blocks.read();
        Ok(blocks.iter().find(|b| b.hash() == *hash).cloned())
    }
    
    async fn get_latest_block(&self) -> PacyteResult<Option<Block>> {
        let blocks = self.blocks.read();
        Ok(blocks.last().cloned())
    }
    
    async fn get_block_height(&self) -> PacyteResult<BlockHeight> {
        let blocks = self.blocks.read();
        Ok(blocks.len().saturating_sub(1) as BlockHeight)
    }
    
    async fn save_transaction(&self, tx: &Transaction) -> PacyteResult<()> {
        self.transactions.insert(tx.hash(), tx.clone());
        Ok(())
    }
    
    async fn get_transaction(&self, hash: &Hash) -> PacyteResult<Option<Transaction>> {
        Ok(self.transactions.get(hash).map(|r| r.clone()))
    }
    
    async fn get_transactions_by_block(&self, height: BlockHeight) -> PacyteResult<Vec<Transaction>> {
        if let Some(block) = self.get_block(height).await? {
            Ok(block.body.transactions)
        } else {
            Ok(Vec::new())
        }
    }
    
    async fn save_account(&self, address: &Address, account: &Account) -> PacyteResult<()> {
        self.accounts.insert(*address, account.clone());
        Ok(())
    }
    
    async fn get_account(&self, address: &Address) -> PacyteResult<Option<Account>> {
        Ok(self.accounts.get(address).map(|r| r.clone()))
    }
    
    async fn delete_account(&self, address: &Address) -> PacyteResult<()> {
        self.accounts.remove(address);
        Ok(())
    }
    
    async fn account_exists(&self, address: &Address) -> PacyteResult<bool> {
        Ok(self.accounts.contains_key(address))
    }
    
    async fn save_state_root(&self, height: BlockHeight, root: &Hash) -> PacyteResult<()> {
        self.state_roots.insert(height, *root);
        Ok(())
    }
    
    async fn get_state_root(&self, height: BlockHeight) -> PacyteResult<Option<Hash>> {
        Ok(self.state_roots.get(&height).map(|r| *r))
    }
    
    async fn write_batch(&self, batch: WriteBatch) -> PacyteResult<()> {
        for (_, block) in batch.blocks {
            self.save_block(&block).await?;
        }
        for (_, tx) in batch.transactions {
            self.save_transaction(&tx).await?;
        }
        for (addr, account_opt) in batch.accounts {
            match account_opt {
                Some(acc) => self.save_account(&addr, &acc).await?,
                None => self.delete_account(&addr).await?,
            }
        }
        for (height, root) in batch.state_roots {
            self.save_state_root(height, &root).await?;
        }
        Ok(())
    }
    
    async fn create_snapshot(&self, _path: &PathBuf) -> PacyteResult<()> {
        Err(PacyteError::NotImplemented("Memory storage snapshot".to_string()))
    }
    
    async fn restore_from_snapshot(&self, _path: &PathBuf) -> PacyteResult<()> {
        Err(PacyteError::NotImplemented("Memory storage restore".to_string()))
    }
    
    fn stats(&self) -> StorageStats {
        StorageStats {
            total_blocks: self.blocks.read().len() as u64,
            total_transactions: self.transactions.len() as u64,
            total_accounts: self.accounts.len() as u64,
            disk_usage_bytes: 0,
            cache_hit_rate: 1.0,
            avg_read_latency_us: 1,
            avg_write_latency_us: 1,
        }
    }
    
    async fn close(&self) -> PacyteResult<()> {
        Ok(())
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::account::Account;
    use crate::types::block::Block;
    use crate::types::transaction::Transaction;

    #[tokio::test]
    async fn test_memory_storage_block() {
        let storage = MemoryStorage::new();
        
        let block = Block::genesis();
        storage.save_block(&block).await.unwrap();
        
        let retrieved = storage.get_block(0).await.unwrap().unwrap();
        assert_eq!(block.hash(), retrieved.hash());
        
        let latest = storage.get_latest_block().await.unwrap().unwrap();
        assert_eq!(block.hash(), latest.hash());
        
        assert_eq!(storage.get_block_height().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_memory_storage_account() {
        let storage = MemoryStorage::new();
        
        let addr = [1u8; 32];
        let account = Account::new(addr, 1000);
        
        storage.save_account(&addr, &account).await.unwrap();
        
        assert!(storage.account_exists(&addr).await.unwrap());
        
        let retrieved = storage.get_account(&addr).await.unwrap().unwrap();
        assert_eq!(retrieved.balance, 1000);
        
        storage.delete_account(&addr).await.unwrap();
        assert!(!storage.account_exists(&addr).await.unwrap());
    }

    #[tokio::test]
    async fn test_write_batch() {
        let storage = MemoryStorage::new();
        
        let mut batch = WriteBatch::new();
        
        let addr = [1u8; 32];
        let account = Account::new(addr, 1000);
        batch.add_account(addr, account);
        
        let block = Block::genesis();
        batch.add_block(block.clone());
        
        storage.write_batch(batch).await.unwrap();
        
        assert!(storage.account_exists(&addr).await.unwrap());
        assert_eq!(storage.get_block_height().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_stats() {
        let storage = MemoryStorage::new();
        
        let stats = storage.stats();
        assert_eq!(stats.total_blocks, 0);
        assert_eq!(stats.total_accounts, 0);
    }
}