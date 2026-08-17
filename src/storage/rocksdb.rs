// ===================================================================
// PACYTE NEXUS - ROCKSDB ENTEGRASYONU (GERÇEK)
// ===================================================================

use rocksdb::{
    DB, Options, WriteBatch as RocksWriteBatch, 
    BlockBasedOptions, Cache, ColumnFamily, ColumnFamilyDescriptor,
    Direction, IteratorMode, ReadOptions, WriteOptions,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::RwLock;

use crate::types::{PacyteError, PacyteResult, Hash, BlockHeight, Address};
use crate::types::block::Block;
use crate::types::transaction::Transaction;
use crate::types::account::Account;
use super::{
    Storage, StorageConfig, StorageStats, WriteBatch as PacyteWriteBatch,
    CompressionType,
};

// ===================================================================
// COLUMN FAMILY İSİMLERİ
// ===================================================================

const CF_BLOCKS: &str = "blocks";
const CF_TRANSACTIONS: &str = "transactions";
const CF_ACCOUNTS: &str = "accounts";
const CF_STATE_ROOTS: &str = "state_roots";
const CF_METADATA: &str = "metadata";

const KEY_LATEST_BLOCK: &[u8] = b"latest_block";
const KEY_BLOCK_HEIGHT: &[u8] = b"block_height";

// ===================================================================
// ROCKSDB STORAGE
// ===================================================================

pub struct RocksDBStorage {
    db: Arc<DB>,
    path: PathBuf,
    config: StorageConfig,
    stats: Arc<RwLock<StorageStats>>,
    block_cache: Arc<dashmap::DashMap<BlockHeight, Block>>,
    account_cache: Arc<dashmap::DashMap<Address, Account>>,
}

impl RocksDBStorage {
    pub fn new(path: PathBuf, config: StorageConfig) -> PacyteResult<Self> {
        std::fs::create_dir_all(&path)
            .map_err(|e| PacyteError::RocksDBError(format!("Failed to create dir: {}", e)))?;
        
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.set_max_open_files(config.max_open_files);
        opts.set_compression_type(match config.compression {
            CompressionType::None => rocksdb::DBCompressionType::None,
            CompressionType::Snappy => rocksdb::DBCompressionType::Snappy,
            CompressionType::Lz4 => rocksdb::DBCompressionType::Lz4,
            CompressionType::Zstd => rocksdb::DBCompressionType::Zstd,
        });
        
        // Block cache ayarları
        let mut block_opts = BlockBasedOptions::default();
        if config.cache_size_mb > 0 {
            let cache = Cache::new_lru_cache(config.cache_size_mb * 1024 * 1024);
            block_opts.set_block_cache(&cache);
        }
        opts.set_block_based_table_factory(&block_opts);
        
        // WAL ayarları
        if !config.wal_enabled {
            //opts.set_disable_wal(true);
        }
        
        // Write options
        let mut write_opts = WriteOptions::default();
        write_opts.set_sync(config.sync_writes);
        
        // Column family'leri aç
        let cf_names = vec![
            CF_BLOCKS,
            CF_TRANSACTIONS,
            CF_ACCOUNTS,
            CF_STATE_ROOTS,
            CF_METADATA,
        ];
        
        let db = DB::open_cf_descriptors(
            &opts,
            &path,
            cf_names.iter().map(|name| ColumnFamilyDescriptor::new(*name, opts.clone())),
        ).map_err(|e| PacyteError::RocksDBError(format!("Failed to open DB: {}", e)))?;
        
        // Mevcut yüksekliği oku
        let block_height = Self::read_metadata(&db, KEY_BLOCK_HEIGHT).unwrap_or(0);
        
        let storage = Self {
            db: Arc::new(db),
            path,
            config,
            stats: Arc::new(RwLock::new(StorageStats {
                total_blocks: block_height + 1,
                ..Default::default()
            })),
            block_cache: Arc::new(dashmap::DashMap::new()),
            account_cache: Arc::new(dashmap::DashMap::new()),
        };
        
        // İstatistikleri güncelle
        storage.update_stats();
        
        Ok(storage)
    }
    
    fn cf_handle(&self, name: &str) -> Arc<ColumnFamily> {
    let cf_ptr = self.db.cf_handle(name).expect("CF should exist") as *const ColumnFamily;
    unsafe { Arc::from_raw(cf_ptr) }
}
    
    fn read_metadata(db: &DB, key: &[u8]) -> Option<u64> {
        let cf = db.cf_handle(CF_METADATA)?;
        db.get_cf(&*cf, key)
            .ok()
            .flatten()
            .and_then(|bytes| {
                if bytes.len() == 8 {
                    Some(u64::from_le_bytes(bytes.try_into().ok()?))
                } else {
                    None
                }
            })
    }
    
    fn write_metadata(&self, key: &[u8], value: u64) -> PacyteResult<()> {
        let cf = self.cf_handle(CF_METADATA);
        self.db.put_cf(&*cf, key, value.to_le_bytes())
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))
    }
    
    fn update_stats(&self) {
        let mut stats = self.stats.write();
        
        // Blok sayısı
        if let Ok(Some(bytes)) = self.db.get(b"stats:total_blocks") {
            if bytes.len() == 8 {
                stats.total_blocks = u64::from_le_bytes(bytes.try_into().unwrap());
            }
        }
        
        // İşlem sayısı
        if let Ok(Some(bytes)) = self.db.get(b"stats:total_transactions") {
            if bytes.len() == 8 {
                stats.total_transactions = u64::from_le_bytes(bytes.try_into().unwrap());
            }
        }
        
        // Disk kullanımı
        stats.disk_usage_bytes = self.get_disk_usage();
    }
    
    fn get_disk_usage(&self) -> u64 {
        use std::fs;
        
        let mut total = 0;
        if let Ok(entries) = fs::read_dir(&self.path) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    total += metadata.len();
                }
            }
        }
        total
    }
    
    fn increment_stat(&self, key: &[u8], delta: u64) -> PacyteResult<()> {
        let current = self.db.get(key)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?
            .and_then(|b| {
                if b.len() == 8 {
                    Some(u64::from_le_bytes(b.try_into().unwrap()))
                } else {
                    None
                }
            })
            .unwrap_or(0);
        
        self.db.put(key, (current + delta).to_le_bytes())
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))
    }
}

#[async_trait::async_trait]
impl Storage for RocksDBStorage {
    async fn save_block(&self, block: &Block) -> PacyteResult<()> {
        let cf = self.cf_handle(CF_BLOCKS);
        let key = block.header.height.to_le_bytes();
        let value = bincode::serialize(block)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        self.db.put_cf(&*cf, key, value)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        // Cache'e ekle
        self.block_cache.insert(block.header.height, block.clone());
        
        // Metadata güncelle
        self.write_metadata(KEY_BLOCK_HEIGHT, block.header.height)?;
        
        // İstatistik güncelle
        self.increment_stat(b"stats:total_blocks", 1)?;
        
        let mut stats = self.stats.write();
        stats.total_blocks = block.header.height + 1;
        
        Ok(())
    }
    
    async fn get_block(&self, height: BlockHeight) -> PacyteResult<Option<Block>> {
        // Önce cache'e bak
        if let Some(block) = self.block_cache.get(&height) {
            return Ok(Some(block.clone()));
        }
        
        let cf = self.cf_handle(CF_BLOCKS);
        let key = height.to_le_bytes();
        
        let bytes = self.db.get_cf(&*cf, key)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        if let Some(bytes) = bytes {
            let block: Block = bincode::deserialize(&bytes)
                .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
            
            // Cache'e ekle
            self.block_cache.insert(height, block.clone());
            
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }
    
    async fn get_block_by_hash(&self, hash: &Hash) -> PacyteResult<Option<Block>> {
        // Hash'e göre arama için tüm blokları taramamız gerek
        // Bu pahalı bir işlem, indeks kullanılmalı
        let cf = self.cf_handle(CF_BLOCKS);
        let mut iter = self.db.iterator_cf(&*cf, IteratorMode::End);
        
        while let Some(Ok((_, value))) = iter.next() {
            if let Ok(block) = bincode::deserialize::<Block>(&value) {
                if block.hash() == *hash {
                    return Ok(Some(block));
                }
            }
        }
        
        Ok(None)
    }
    
    async fn get_latest_block(&self) -> PacyteResult<Option<Block>> {
        let height = self.get_block_height().await?;
        if height == 0 && !self.account_exists(&[0u8; 32]).await? {
            return Ok(None);
        }
        self.get_block(height).await
    }
    
    async fn get_block_height(&self) -> PacyteResult<BlockHeight> {
        let cf = self.cf_handle(CF_METADATA);
        let bytes = self.db.get_cf(&*cf, KEY_BLOCK_HEIGHT)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        Ok(bytes
            .and_then(|b| {
                if b.len() == 8 {
                    Some(u64::from_le_bytes(b.try_into().ok()?))
                } else {
                    None
                }
            })
            .unwrap_or(0))
    }
    
    async fn save_transaction(&self, tx: &Transaction) -> PacyteResult<()> {
        let cf = self.cf_handle(CF_TRANSACTIONS);
        let key = tx.hash();
        let value = bincode::serialize(tx)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        self.db.put_cf(&*cf, key, value)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        self.increment_stat(b"stats:total_transactions", 1)?;
        
        Ok(())
    }
    
    async fn get_transaction(&self, hash: &Hash) -> PacyteResult<Option<Transaction>> {
        let cf = self.cf_handle(CF_TRANSACTIONS);
        
        let bytes = self.db.get_cf(&*cf, hash)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        bytes
            .map(|b| bincode::deserialize(&b).map_err(|e| PacyteError::RocksDBError(e.to_string())))
            .transpose()
    }
    
    async fn get_transactions_by_block(&self, height: BlockHeight) -> PacyteResult<Vec<Transaction>> {
        if let Some(block) = self.get_block(height).await? {
            Ok(block.body.transactions)
        } else {
            Ok(Vec::new())
        }
    }
    
    async fn save_account(&self, address: &Address, account: &Account) -> PacyteResult<()> {
        let cf = self.cf_handle(CF_ACCOUNTS);
        let value = bincode::serialize(account)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        self.db.put_cf(&*cf, address, value)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        // Cache'e ekle
        self.account_cache.insert(*address, account.clone());
        
        Ok(())
    }
    
    async fn get_account(&self, address: &Address) -> PacyteResult<Option<Account>> {
        // Önce cache'e bak
        if let Some(account) = self.account_cache.get(address) {
            return Ok(Some(account.clone()));
        }
        
        let cf = self.cf_handle(CF_ACCOUNTS);
        
        let bytes = self.db.get_cf(&*cf, address)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        if let Some(bytes) = bytes {
            let account: Account = bincode::deserialize(&bytes)
                .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
            
            // Cache'e ekle
            self.account_cache.insert(*address, account.clone());
            
            Ok(Some(account))
        } else {
            Ok(None)
        }
    }
    
    async fn delete_account(&self, address: &Address) -> PacyteResult<()> {
        let cf = self.cf_handle(CF_ACCOUNTS);
        
        self.db.delete_cf(&*cf, address)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        self.account_cache.remove(address);
        
        Ok(())
    }
    
    async fn account_exists(&self, address: &Address) -> PacyteResult<bool> {
        if self.account_cache.contains_key(address) {
            return Ok(true);
        }
        
        let cf = self.cf_handle(CF_ACCOUNTS);
        let result = self.db.get_cf(&*cf, address)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        Ok(result.is_some())
    }
    
    async fn save_state_root(&self, height: BlockHeight, root: &Hash) -> PacyteResult<()> {
        let cf = self.cf_handle(CF_STATE_ROOTS);
        let key = height.to_le_bytes();
        
        self.db.put_cf(&*cf, key, root)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))
    }
    
    async fn get_state_root(&self, height: BlockHeight) -> PacyteResult<Option<Hash>> {
        let cf = self.cf_handle(CF_STATE_ROOTS);
        let key = height.to_le_bytes();
        
        let bytes = self.db.get_cf(&*cf, key)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        bytes
            .map(|b| {
                if b.len() == 32 {
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(&b);
                    Ok(hash)
                } else {
                    Err(PacyteError::RocksDBError("Invalid state root length".to_string()))
                }
            })
            .transpose()
    }
    
    async fn write_batch(&self, batch: PacyteWriteBatch) -> PacyteResult<()> {
        let mut rocks_batch = RocksWriteBatch::default();
        
        let cf_blocks = self.cf_handle(CF_BLOCKS);
        let cf_txs = self.cf_handle(CF_TRANSACTIONS);
        let cf_accounts = self.cf_handle(CF_ACCOUNTS);
        let cf_roots = self.cf_handle(CF_STATE_ROOTS);
        
        for (height, block) in &batch.blocks {
            let key = height.to_le_bytes();
            let value = bincode::serialize(block)
                .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
            rocks_batch.put_cf(&*cf_blocks, key, value);
        }
        
        for (hash, tx) in &batch.transactions {
            let value = bincode::serialize(tx)
                .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
            rocks_batch.put_cf(&*cf_txs, hash, value);
        }
        
        for (address, account_opt) in &batch.accounts {
            match account_opt {
                Some(account) => {
                    let value = bincode::serialize(account)
                        .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
                    rocks_batch.put_cf(&*cf_accounts, address, value);
                }
                None => {
                    rocks_batch.delete_cf(&*cf_accounts, address);
                }
            }
        }
        
        for (height, root) in &batch.state_roots {
            let key = height.to_le_bytes();
            rocks_batch.put_cf(&*cf_roots, key, root);
        }
        
        // Batch'i yaz
        self.db.write(rocks_batch)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        // Cache'leri güncelle
        for (height, block) in batch.blocks {
            self.block_cache.insert(height, block);
        }
        for (address, account_opt) in batch.accounts {
            if let Some(account) = account_opt {
                self.account_cache.insert(address, account);
            } else {
                self.account_cache.remove(&address);
            }
        }
        
        Ok(())
    }
    
    async fn create_snapshot(&self, path: &PathBuf) -> PacyteResult<()> {
        // RocksDB checkpoint
        rocksdb::checkpoint::Checkpoint::new(&self.db)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?
            .create_checkpoint(path)
            .map_err(|e| PacyteError::RocksDBError(e.to_string()))?;
        
        Ok(())
    }
    
    async fn restore_from_snapshot(&self, path: &PathBuf) -> PacyteResult<()> {
        // Mevcut DB'yi kapat ve snapshot'tan geri yükle
        Err(PacyteError::NotImplemented("Restore requires restart".to_string()))
    }
    
    fn stats(&self) -> StorageStats {
        self.stats.read().clone()
    }
    
    async fn close(&self) -> PacyteResult<()> {
        // Cache'leri temizle
        self.block_cache.clear();
        self.account_cache.clear();
        
        // DB'yi kapat
        // Not: Arc<DB> olduğu için diğer referanslar varsa hemen kapanmaz
        Ok(())
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_rocksdb_storage() {
        let temp = tempdir().unwrap();
        let config = StorageConfig::default();
        let storage = RocksDBStorage::new(temp.path().to_path_buf(), config).unwrap();
        
        // Blok kaydet
        let block = Block::genesis();
        storage.save_block(&block).await.unwrap();
        
        // Blok oku
        let retrieved = storage.get_block(0).await.unwrap().unwrap();
        assert_eq!(block.hash(), retrieved.hash());
        
        // Yükseklik kontrolü
        assert_eq!(storage.get_block_height().await.unwrap(), 0);
        
        // Hesap kaydet
        let addr = [1u8; 32];
        let account = Account::new(addr, 1000);
        storage.save_account(&addr, &account).await.unwrap();
        
        // Hesap oku
        let retrieved = storage.get_account(&addr).await.unwrap().unwrap();
        assert_eq!(retrieved.balance, 1000);
        
        // İstatistikler
        let stats = storage.stats();
        assert_eq!(stats.total_blocks, 1);
    }
}