// ===================================================================
// PACYTE NEXUS - SNAPSHOT YÖNETİMİ
// ===================================================================

use std::sync::Arc;
use sha3::Digest; // Sha3_256::new() için
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

use crate::types::{PacyteError, PacyteResult, Hash, BlockHeight, Timestamp};
use crate::types::block::Block;
use crate::types::account::Account;
use crate::storage::{Storage, WriteBatch};

// ===================================================================
// SNAPSHOT MANIFEST
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub version: u32,
    pub height: BlockHeight,
    pub block_hash: Hash,
    pub state_root: Hash,
    pub timestamp: Timestamp,
    pub total_accounts: u64,
    pub total_supply: u128,
    pub checksum: Hash,
}

impl SnapshotManifest {
    pub fn new(height: BlockHeight, block_hash: Hash, state_root: Hash) -> Self {
        Self {
            version: 1,
            height,
            block_hash,
            state_root,
            timestamp: crate::types::current_timestamp(),
            total_accounts: 0,
            total_supply: 0,
            checksum: [0u8; 32],
        }
    }
    
    pub fn compute_checksum(&mut self) {
        let mut hasher = sha3::Sha3_256::new();
        hasher.update(&self.version.to_le_bytes());
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.block_hash);
        hasher.update(&self.state_root);
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.update(&self.total_accounts.to_le_bytes());
        hasher.update(&self.total_supply.to_le_bytes());
        self.checksum = hasher.finalize().into();
    }
    
    pub fn verify(&self) -> bool {
        let mut copy = self.clone();
        copy.checksum = [0u8; 32];
        copy.compute_checksum();
        copy.checksum == self.checksum
    }
}

// ===================================================================
// SNAPSHOT MANAGER
// ===================================================================

pub struct SnapshotManager {
    snapshot_dir: PathBuf,
    storage: Arc<dyn Storage>,
}

impl SnapshotManager {
    pub fn new(snapshot_dir: PathBuf, storage: Arc<dyn Storage>) -> PacyteResult<Self> {
        fs::create_dir_all(&snapshot_dir)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        Ok(Self {
            snapshot_dir,
            storage,
        })
    }
    
    /// Snapshot oluştur
    pub async fn create_snapshot(&self, height: BlockHeight) -> PacyteResult<SnapshotManifest> {
        let block = self.storage.get_block(height).await?
            .ok_or_else(|| PacyteError::BlockNotFound(height))?;
        
        let state_root = self.storage.get_state_root(height).await?
            .ok_or_else(|| PacyteError::StateRootMismatch {
                expected: format!("{}", height),
                actual: "Not found".to_string(),
            })?;
        
        let mut manifest = SnapshotManifest::new(height, block.hash(), state_root);
        
        // Snapshot dizini oluştur
        let snapshot_path = self.snapshot_dir.join(format!("snapshot_{}", height));
        fs::create_dir_all(&snapshot_path)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        // Blokları kaydet
        self.save_blocks(&snapshot_path, height).await?;
        
        // State'i kaydet
        let (total_accounts, total_supply) = self.save_state(&snapshot_path, height).await?;
        
        manifest.total_accounts = total_accounts;
        manifest.total_supply = total_supply;
        manifest.compute_checksum();
        
        // Manifest'i kaydet
        let manifest_path = snapshot_path.join("manifest.json");
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        fs::write(manifest_path, manifest_json)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        tracing::info!(
            "Snapshot created at height {}: {} accounts, {} supply",
            height, total_accounts, total_supply
        );
        
        Ok(manifest)
    }
    
    async fn save_blocks(&self, path: &Path, max_height: BlockHeight) -> PacyteResult<()> {
        let blocks_path = path.join("blocks.bin");
        let mut file = BufWriter::new(
            File::create(blocks_path)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?
        );
        
        for height in 0..=max_height {
            if let Some(block) = self.storage.get_block(height).await? {
                let bytes = bincode::serialize(&block)
                    .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
                
                file.write_all(&(bytes.len() as u32).to_le_bytes())
                    .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
                file.write_all(&bytes)
                    .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            }
        }
        
        file.flush()
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        Ok(())
    }
    
    async fn save_state(&self, path: &Path, height: BlockHeight) -> PacyteResult<(u64, u128)> {
        let state_path = path.join("state.bin");
        let mut file = BufWriter::new(
            File::create(state_path)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?
        );
        
        // Not: Gerçek implementasyonda state iterasyonu yapılmalı
        // Bu basitleştirilmiş versiyon
        
        let mut total_accounts = 0u64;
        let mut total_supply = 0u128;
        
        // Placeholder - gerçek implementasyon storage'dan iterate eder
        file.write_all(&total_accounts.to_le_bytes())
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        file.write_all(&total_supply.to_le_bytes())
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        file.flush()
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        Ok((total_accounts, total_supply))
    }
    
    /// Snapshot'tan geri yükle
    pub async fn restore_from_snapshot(&self, height: BlockHeight) -> PacyteResult<()> {
        let snapshot_path = self.snapshot_dir.join(format!("snapshot_{}", height));
        
        if !snapshot_path.exists() {
            return Err(PacyteError::DiskIoFailure(
                format!("Snapshot not found: {}", snapshot_path.display())
            ));
        }
        
        // Manifest'i oku
        let manifest_path = snapshot_path.join("manifest.json");
        let manifest_json = fs::read_to_string(manifest_path)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        let manifest: SnapshotManifest = serde_json::from_str(&manifest_json)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        if !manifest.verify() {
            return Err(PacyteError::DiskIoFailure("Manifest checksum mismatch".to_string()));
        }
        
        tracing::info!("Restoring from snapshot at height {}", height);
        
        // Blokları geri yükle
        self.restore_blocks(&snapshot_path).await?;
        
        // State'i geri yükle
        self.restore_state(&snapshot_path).await?;
        
        tracing::info!("Snapshot restored successfully");
        
        Ok(())
    }
    
    async fn restore_blocks(&self, path: &Path) -> PacyteResult<()> {
        let blocks_path = path.join("blocks.bin");
        let mut file = BufReader::new(
            File::open(blocks_path)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?
        );
        
        let mut batch = WriteBatch::new();
        
        loop {
            let mut len_bytes = [0u8; 4];
            if file.read_exact(&mut len_bytes).is_err() {
                break;
            }
            
            let len = u32::from_le_bytes(len_bytes) as usize;
            let mut bytes = vec![0u8; len];
            file.read_exact(&mut bytes)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            
            let block: Block = bincode::deserialize(&bytes)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            
            batch.add_block(block);
            
            // Batch belirli boyuta ulaşınca yaz
            if batch.len() >= 1000 {
                self.storage.write_batch(batch.clone()).await?;
                batch.clear();
            }
        }
        
        if !batch.is_empty() {
            self.storage.write_batch(batch).await?;
        }
        
        Ok(())
    }
    
    async fn restore_state(&self, path: &Path) -> PacyteResult<()> {
        let state_path = path.join("state.bin");
        let mut file = BufReader::new(
            File::open(state_path)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?
        );
        
        let mut total_accounts_bytes = [0u8; 8];
        file.read_exact(&mut total_accounts_bytes)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        let mut total_supply_bytes = [0u8; 16];
        file.read_exact(&mut total_supply_bytes)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
        
        // State'i geri yükle (basitleştirilmiş)
        Ok(())
    }
    
    /// Mevcut snapshot'ları listele
    pub fn list_snapshots(&self) -> PacyteResult<Vec<SnapshotInfo>> {
        let mut snapshots = Vec::new();
        
        for entry in fs::read_dir(&self.snapshot_dir)
            .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?
        {
            let entry = entry.map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            let path = entry.path();
            
            if path.is_dir() {
                let manifest_path = path.join("manifest.json");
                if manifest_path.exists() {
                    if let Ok(json) = fs::read_to_string(manifest_path) {
                        if let Ok(manifest) = serde_json::from_str::<SnapshotManifest>(&json) {
                            snapshots.push(SnapshotInfo {
                                height: manifest.height,
                                block_hash: manifest.block_hash,
                                timestamp: manifest.timestamp,
                                total_accounts: manifest.total_accounts,
                                path,
                            });
                        }
                    }
                }
            }
        }
        
        snapshots.sort_by_key(|s| s.height);
        Ok(snapshots)
    }
    
    /// Eski snapshot'ları temizle
    pub fn prune_old_snapshots(&self, keep_last: usize) -> PacyteResult<()> {
        let mut snapshots = self.list_snapshots()?;
        
        if snapshots.len() <= keep_last {
            return Ok(());
        }
        
        // En yenileri tut, eskileri sil
        snapshots.sort_by_key(|s| s.height);
        let to_delete = snapshots.len() - keep_last;
        
        for snapshot in snapshots.iter().take(to_delete) {
            fs::remove_dir_all(&snapshot.path)
                .map_err(|e| PacyteError::DiskIoFailure(e.to_string()))?;
            tracing::info!("Pruned snapshot at height {}", snapshot.height);
        }
        
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub height: BlockHeight,
    pub block_hash: Hash,
    pub timestamp: Timestamp,
    pub total_accounts: u64,
    pub path: PathBuf,
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_snapshot_manifest() {
        let mut manifest = SnapshotManifest::new(100, [1u8; 32], [2u8; 32]);
        manifest.total_accounts = 1000;
        manifest.total_supply = 1_000_000;
        manifest.compute_checksum();
        
        assert!(manifest.verify());
        
        // Bozulmuş manifest
        manifest.total_accounts = 999;
        assert!(!manifest.verify());
    }
    
    #[tokio::test]
    async fn test_list_snapshots() {
        let temp = tempdir().unwrap();
        let storage = Arc::new(MemoryStorage::new());
        let manager = SnapshotManager::new(temp.path().to_path_buf(), storage).unwrap();
        
        let snapshots = manager.list_snapshots().unwrap();
        assert!(snapshots.is_empty());
    }
}