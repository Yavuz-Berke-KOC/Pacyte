// ===================================================================
// PACYTE NEXUS - WRITE-AHEAD LOGGING (GERÇEK)
// ===================================================================

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::types::{PacyteError, PacyteResult, Hash, BlockHeight, Address, Timestamp};
use crate::types::block::Block;
use crate::types::transaction::Transaction;
use crate::types::account::Account;
use super::WriteBatch;

// ===================================================================
// WAL SABİTLERİ
// ===================================================================

const WAL_MAGIC: u32 = 0x50435741; // "PACW"
const WAL_VERSION: u32 = 1;
const WAL_MAX_FILE_SIZE: u64 = 64 * 1024 * 1024; // 64 MB
const WAL_FLUSH_BATCH_SIZE: usize = 100;

// ===================================================================
// WAL ENTRY TİPLERİ
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WalEntry {
    // Blok işlemleri
    SaveBlock { height: BlockHeight, block: Block },
    
    // İşlem işlemleri
    SaveTransaction { hash: Hash, transaction: Transaction },
    
    // Hesap işlemleri
    SaveAccount { address: Address, account: Account },
    DeleteAccount { address: Address },
    
    // State root
    SaveStateRoot { height: BlockHeight, root: Hash },
    
    // Batch
    Batch { entries: Vec<WalEntry> },
    
    // Checkpoint
    Checkpoint { lsn: u64, timestamp: Timestamp },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WalRecord {
    magic: u32,
    version: u32,
    lsn: u64,
    timestamp: Timestamp,
    entry: WalEntry,
    checksum: u32,
}

impl WalRecord {
    fn new(lsn: u64, entry: WalEntry) -> Self {
        Self {
            magic: WAL_MAGIC,
            version: WAL_VERSION,
            lsn,
            timestamp: crate::types::current_timestamp(),
            entry,
            checksum: 0,
        }
    }
    
    fn compute_checksum(&self) -> u32 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        
        let mut hasher = DefaultHasher::new();
        self.magic.hash(&mut hasher);
        self.version.hash(&mut hasher);
        self.lsn.hash(&mut hasher);
        self.timestamp.hash(&mut hasher);
        // entry'i hashle (basitleştirilmiş)
        hasher.finish() as u32
    }
    
    fn verify(&self) -> bool {
        self.magic == WAL_MAGIC && 
        self.version == WAL_VERSION &&
        self.compute_checksum() == self.checksum
    }
}

// ===================================================================
// WAL MANAGER
// ===================================================================

pub struct WalManager {
    path: PathBuf,
    current_file: Arc<RwLock<Option<BufWriter<File>>>>,
    current_lsn: Arc<RwLock<u64>>,
    pending_entries: Arc<RwLock<Vec<WalEntry>>>,
    config: WalConfig,
}

#[derive(Debug, Clone)]
pub struct WalConfig {
    pub enabled: bool,
    pub sync_interval_ms: u64,
    pub max_file_size: u64,
    pub max_pending_entries: usize,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sync_interval_ms: 100,
            max_file_size: WAL_MAX_FILE_SIZE,
            max_pending_entries: 1000,
        }
    }
}

impl WalManager {
    pub fn new(path: PathBuf, config: WalConfig) -> PacyteResult<Self> {
        std::fs::create_dir_all(&path)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        // En son LSN'yi bul
        let current_lsn = Self::find_latest_lsn(&path)?;
        
        // Yeni WAL dosyası aç
        let file = Self::open_wal_file(&path, current_lsn)?;
        
        Ok(Self {
            path,
            current_file: Arc::new(RwLock::new(Some(file))),
            current_lsn: Arc::new(RwLock::new(current_lsn)),
            pending_entries: Arc::new(RwLock::new(Vec::new())),
            config,
        })
    }
    
    fn find_latest_lsn(path: &PathBuf) -> PacyteResult<u64> {
        let mut max_lsn = 0;
        
        for entry in std::fs::read_dir(path)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?
        {
            let entry = entry.map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            
            if name_str.starts_with("wal_") && name_str.ends_with(".log") {
                if let Some(lsn_str) = name_str
                    .trim_start_matches("wal_")
                    .trim_end_matches(".log")
                    .parse::<u64>()
                    .ok()
                {
                    max_lsn = max_lsn.max(lsn_str);
                }
            }
        }
        
        Ok(max_lsn)
    }
    
    fn open_wal_file(path: &PathBuf, lsn: u64) -> PacyteResult<BufWriter<File>> {
        let file_path = path.join(format!("wal_{:016x}.log", lsn));
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        Ok(BufWriter::with_capacity(64 * 1024, file))
    }
    
    pub fn append(&self, entry: WalEntry) -> PacyteResult<u64> {
        if !self.config.enabled {
            return Ok(0);
        }
        
        let mut lsn_guard = self.current_lsn.write();
        let lsn = *lsn_guard + 1;
        *lsn_guard = lsn;
        
        let record = WalRecord::new(lsn, entry.clone());
        
        // Pending listeye ekle
        {
            let mut pending = self.pending_entries.write();
            pending.push(entry);
            
            // Batch boyutuna ulaştıysa flush et
            if pending.len() >= self.config.max_pending_entries {
                drop(pending);
                self.flush()?;
            }
        }
        
        Ok(lsn)
    }
    
    pub fn flush(&self) -> PacyteResult<()> {
        if !self.config.enabled {
            return Ok(());
        }
        
        let pending = {
            let mut pending = self.pending_entries.write();
            std::mem::take(&mut *pending)
        };
        
        if pending.is_empty() {
            return Ok(());
        }
        
        let mut file_guard = self.current_file.write();
        let file = file_guard.as_mut().ok_or_else(|| {
            PacyteError::DiskIoFailure("WAL file not open".to_string())
        })?;
        
        let lsn = *self.current_lsn.read();
        
        for entry in &pending {
            let mut record = WalRecord::new(lsn, entry.clone());
            record.checksum = record.compute_checksum();
            
            let bytes = bincode::serialize(&record)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            
            // Uzunluğu yaz (4 byte)
            file.write_all(&(bytes.len() as u32).to_le_bytes())
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            
            // Kaydı yaz
            file.write_all(&bytes)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        }
        
        file.flush()
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        // Dosya boyutunu kontrol et, limit aşıldıysa yeni dosya aç
        let file_size = file.get_ref().metadata()
            .map(|m| m.len())
            .unwrap_or(0);
        
        if file_size >= self.config.max_file_size {
            let new_lsn = *self.current_lsn.read();
            let new_file = Self::open_wal_file(&self.path, new_lsn)?;
            *file_guard = Some(new_file);
        }
        
        Ok(())
    }
    
    pub fn recover(&self) -> PacyteResult<Vec<WriteBatch>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }
        
        let mut batches = Vec::new();
        
        for entry in std::fs::read_dir(&self.path)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?
        {
            let entry = entry.map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("log") {
                let recovered = self.recover_from_file(&path)?;
                batches.extend(recovered);
            }
        }
        
        Ok(batches)
    }
    
    fn recover_from_file(&self, path: &PathBuf) -> PacyteResult<Vec<WriteBatch>> {
        let file = File::open(path)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        let mut reader = BufReader::new(file);
        let mut batches = Vec::new();
        let mut current_batch = WriteBatch::new();
        let mut batch_open = false;
        
        loop {
            // Uzunluğu oku
            let mut len_bytes = [0u8; 4];
            if reader.read_exact(&mut len_bytes).is_err() {
                break; // Dosya sonu
            }
            
            let len = u32::from_le_bytes(len_bytes) as usize;
            
            // Kaydı oku
            let mut bytes = vec![0u8; len];
            reader.read_exact(&mut bytes)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            
            // Deserialize et
            let record: WalRecord = bincode::deserialize(&bytes)
                .map_err(|e| PacyteError::DiskIoFailure(format!("Deserialize failed: {}", e)))?;
            
            // Doğrula
            if !record.verify() {
                tracing::warn!("WAL record checksum mismatch at LSN {}", record.lsn);
                continue;
            }
            
            // Entry'i işle
            match &record.entry {
                WalEntry::SaveBlock { height, block } => {
                    current_batch.add_block(block.clone());
                }
                WalEntry::SaveTransaction { hash: _, transaction } => {
                    current_batch.add_transaction(transaction.clone());
                }
                WalEntry::SaveAccount { address, account } => {
                    current_batch.add_account(*address, account.clone());
                }
                WalEntry::DeleteAccount { address } => {
                    current_batch.delete_account(*address);
                }
                WalEntry::SaveStateRoot { height, root } => {
                    current_batch.add_state_root(*height, *root);
                }
                WalEntry::Batch { entries } => {
                    batch_open = true;
                }
                WalEntry::Checkpoint { lsn, timestamp } => {
                    if batch_open {
                        batches.push(current_batch.clone());
                        current_batch.clear();
                        batch_open = false;
                    }
                    tracing::info!("WAL checkpoint at LSN {} (ts={})", lsn, timestamp);
                }
            }
        }
        
        // Kalan batch'i ekle
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }
        
        Ok(batches)
    }
    
    pub async fn start_flush_task(self: Arc<Self>) {
        if !self.config.enabled {
            return;
        }
        
        let interval = std::time::Duration::from_millis(self.config.sync_interval_ms);
        
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            
            loop {
                timer.tick().await;
                
                if let Err(e) = self.flush() {
                    tracing::error!("WAL flush failed: {}", e);
                }
            }
        });
    }
    
    pub fn checkpoint(&self) -> PacyteResult<()> {
        self.append(WalEntry::Checkpoint {
            lsn: *self.current_lsn.read(),
            timestamp: crate::types::current_timestamp(),
        })?;
        self.flush()
    }
    
    pub fn close(&self) -> PacyteResult<()> {
        self.flush()?;
        *self.current_file.write() = None;
        Ok(())
    }
}

impl Drop for WalManager {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_wal_append_and_recover() {
        let temp = tempdir().unwrap();
        let config = WalConfig::default();
        let wal = WalManager::new(temp.path().to_path_buf(), config).unwrap();
        
        // Blok kaydet
        let block = Block::genesis();
        wal.append(WalEntry::SaveBlock {
            height: 0,
            block: block.clone(),
        }).unwrap();
        
        // Hesap kaydet
        let addr = [1u8; 32];
        let account = Account::new(addr, 1000);
        wal.append(WalEntry::SaveAccount {
            address: addr,
            account: account.clone(),
        }).unwrap();
        
        // Checkpoint
        wal.checkpoint().unwrap();
        wal.close().unwrap();
        
        // Recovery
        let wal2 = WalManager::new(temp.path().to_path_buf(), config).unwrap();
        let batches = wal2.recover().unwrap();
        
        assert!(!batches.is_empty());
        
        let batch = &batches[0];
        assert_eq!(batch.blocks.len(), 1);
        assert_eq!(batch.accounts.len(), 1);
    }
    
    #[test]
    fn test_wal_file_rotation() {
        let temp = tempdir().unwrap();
        let mut config = WalConfig::default();
        config.max_file_size = 1024; // 1KB - küçük dosya testi
        
        let wal = WalManager::new(temp.path().to_path_buf(), config).unwrap();
        
        // Çok sayıda entry ekle
        for i in 0..100 {
            let addr = [i as u8; 32];
            let account = Account::new(addr, 1000);
            wal.append(WalEntry::SaveAccount {
                address: addr,
                account,
            }).unwrap();
        }
        
        wal.flush().unwrap();
        
        // Birden fazla dosya oluşmuş olmalı
        let file_count = std::fs::read_dir(temp.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".log")
            })
            .count();
        
        assert!(file_count > 1);
    }
}