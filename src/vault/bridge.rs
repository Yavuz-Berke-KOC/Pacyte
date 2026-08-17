// ===================================================================
// PACYTE NEXUS - CROSS-SHARD BRIDGE
// ===================================================================

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::types::{
    PacyteError, PacyteResult, Address, Hash, ShardId, Timestamp, current_timestamp,
};

// ===================================================================
// BRIDGE SABİTLERİ
// ===================================================================

const BRIDGE_TIMEOUT_SECS: u64 = 60;
const MAX_BRIDGE_AMOUNT: u128 = 10_000_000_000_000; // 10M PAC
const BRIDGE_FEE_BPS: u128 = 10; // 0.1%

// ===================================================================
// BRIDGE TRANSACTION
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransaction {
    pub id: u64,
    pub sender: Address,
    pub recipient: Address,
    pub amount: u128,
    pub source_shard: ShardId,
    pub target_shard: ShardId,
    pub status: BridgeStatus,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    pub hash: Hash,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BridgeStatus {
    Pending,
    Locked,
    Relayed,
    Finalized,
    Reverting,
    Reverted,
    Expired,
}

impl std::fmt::Display for BridgeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl BridgeTransaction {
    pub fn new(
        id: u64,
        sender: Address,
        recipient: Address,
        amount: u128,
        source_shard: ShardId,
        target_shard: ShardId,
    ) -> Self {
        let now = current_timestamp();
        Self {
            id,
            sender,
            recipient,
            amount,
            source_shard,
            target_shard,
            status: BridgeStatus::Pending,
            created_at: now,
            expires_at: now + BRIDGE_TIMEOUT_SECS,
            hash: [0u8; 32],
        }
    }
    
    pub fn is_expired(&self, current_time: Timestamp) -> bool {
        current_time > self.expires_at
    }
    
    pub fn time_remaining(&self, current_time: Timestamp) -> u64 {
        if current_time >= self.expires_at {
            0
        } else {
            self.expires_at - current_time
        }
    }
    
    pub fn calculate_fee(&self) -> u128 {
        (self.amount * BRIDGE_FEE_BPS) / 10000
    }
}

// ===================================================================
// BRIDGE MANAGER
// ===================================================================

pub struct BridgeManager {
    // Bridge işlemleri (ID -> Transaction)
    bridges: Arc<RwLock<HashMap<u64, BridgeTransaction>>>,
    
    // Shard başına pending işlemler
    pending_by_shard: Arc<RwLock<HashMap<ShardId, Vec<u64>>>>,
    
    // İşlenmiş bridge'ler (hash -> status)
    processed: Arc<RwLock<HashMap<Hash, BridgeStatus>>>,
    
    // Sayaç
    next_bridge_id: Arc<RwLock<u64>>,
    
    // İstatistikler
    total_bridged: Arc<RwLock<u128>>,
    total_failed: Arc<RwLock<u64>>,
}

impl BridgeManager {
    pub fn new() -> Self {
        Self {
            bridges: Arc::new(RwLock::new(HashMap::new())),
            pending_by_shard: Arc::new(RwLock::new(HashMap::new())),
            processed: Arc::new(RwLock::new(HashMap::new())),
            next_bridge_id: Arc::new(RwLock::new(1)),
            total_bridged: Arc::new(RwLock::new(0)),
            total_failed: Arc::new(RwLock::new(0)),
        }
    }
    
    /// Yeni bridge işlemi başlat
    pub fn initiate_bridge(
        &self,
        sender: Address,
        recipient: Address,
        amount: u128,
        source_shard: ShardId,
        target_shard: ShardId,
    ) -> PacyteResult<BridgeTransaction> {
        // Maksimum miktar kontrolü
        if amount > MAX_BRIDGE_AMOUNT {
            return Err(PacyteError::BridgeAmountTooLarge(amount, MAX_BRIDGE_AMOUNT));
        }
        
        // Shard kontrolü
        if source_shard == target_shard {
            return Err(PacyteError::SameShardTransfer);
        }
        
        let id = {
            let mut next = self.next_bridge_id.write();
            let id = *next;
            *next += 1;
            id
        };
        
        let tx = BridgeTransaction::new(
            id,
            sender,
            recipient,
            amount,
            source_shard,
            target_shard,
        );
        
        // Kaydet
        {
            let mut bridges = self.bridges.write();
            bridges.insert(id, tx.clone());
        }
        
        // Pending listeye ekle
        {
            let mut pending = self.pending_by_shard.write();
            pending.entry(source_shard)
                .or_insert_with(Vec::new)
                .push(id);
        }
        
        tracing::info!(
            "🌉 Bridge initiated: {} -> {} ({} PAC) | ID: {}",
            source_shard, target_shard, amount, id
        );
        
        Ok(tx)
    }
    
    /// Bridge'i lockla (kaynak shard'da)
    pub fn lock_bridge(&self, bridge_id: u64) -> PacyteResult<()> {
        let mut bridges = self.bridges.write();
        
        let tx = bridges.get_mut(&bridge_id)
            .ok_or_else(|| PacyteError::BridgeNotFound(bridge_id))?;
        
        if tx.status != BridgeStatus::Pending {
            return Err(PacyteError::InvalidBridgeStatus {
                expected: BridgeStatus::Pending.to_string(),
                actual: tx.status.to_string(),
            });
        }
        
        tx.status = BridgeStatus::Locked;
        
        tracing::debug!("Bridge {} locked", bridge_id);
        
        Ok(())
    }
    
    /// Bridge'i relay et (hedef shard'a gönder)
    pub fn relay_bridge(&self, bridge_id: u64) -> PacyteResult<()> {
        let mut bridges = self.bridges.write();
        
        let tx = bridges.get_mut(&bridge_id)
            .ok_or_else(|| PacyteError::BridgeNotFound(bridge_id))?;
        
        if tx.status != BridgeStatus::Locked {
            return Err(PacyteError::InvalidBridgeStatus {
                expected: BridgeStatus::Locked.to_string(),
                actual: tx.status.to_string(),
            });
        }
        
        tx.status = BridgeStatus::Relayed;
        
        // Pending'den çıkar, hedef shard'a ekle
        {
            let mut pending = self.pending_by_shard.write();
            
            if let Some(list) = pending.get_mut(&tx.source_shard) {
                list.retain(|id| *id != bridge_id);
            }
            
            pending.entry(tx.target_shard)
                .or_insert_with(Vec::new)
                .push(bridge_id);
        }
        
        tracing::debug!("Bridge {} relayed to shard {}", bridge_id, tx.target_shard);
        
        Ok(())
    }
    
    /// Bridge'i finalize et (hedef shard'da)
    pub fn finalize_bridge(&self, bridge_id: u64) -> PacyteResult<()> {
        let mut bridges = self.bridges.write();
        
        let tx = bridges.get_mut(&bridge_id)
            .ok_or_else(|| PacyteError::BridgeNotFound(bridge_id))?;
        
        if tx.status != BridgeStatus::Relayed {
            return Err(PacyteError::InvalidBridgeStatus {
                expected: BridgeStatus::Relayed.to_string(),
                actual: tx.status.to_string(),
            });
        }
        
        tx.status = BridgeStatus::Finalized;
        
        // İstatistikleri güncelle
        *self.total_bridged.write() += tx.amount;
        
        // Processed olarak işaretle
        self.processed.write().insert(tx.hash, BridgeStatus::Finalized);
        
        // Pending'den çıkar
        {
            let mut pending = self.pending_by_shard.write();
            if let Some(list) = pending.get_mut(&tx.target_shard) {
                list.retain(|id| *id != bridge_id);
            }
        }
        
        tracing::info!("🌉 Bridge {} finalized: {} PAC transferred", bridge_id, tx.amount);
        
        Ok(())
    }
    
    /// Bridge'i geri al (timeout veya hata durumunda)
    pub fn revert_bridge(&self, bridge_id: u64) -> PacyteResult<()> {
        let mut bridges = self.bridges.write();
        
        let tx = bridges.get_mut(&bridge_id)
            .ok_or_else(|| PacyteError::BridgeNotFound(bridge_id))?;
        
        if tx.status == BridgeStatus::Finalized {
            return Err(PacyteError::BridgeAlreadyFinalized);
        }
        
        tx.status = BridgeStatus::Reverting;
        
        tracing::warn!("Bridge {} reverting", bridge_id);
        
        Ok(())
    }
    
    /// Bridge'i reverted olarak işaretle
    pub fn mark_reverted(&self, bridge_id: u64) -> PacyteResult<()> {
        let mut bridges = self.bridges.write();
        
        let tx = bridges.get_mut(&bridge_id)
            .ok_or_else(|| PacyteError::BridgeNotFound(bridge_id))?;
        
        tx.status = BridgeStatus::Reverted;
        
        *self.total_failed.write() += 1;
        
        // Pending'den çıkar
        {
            let mut pending = self.pending_by_shard.write();
            for list in pending.values_mut() {
                list.retain(|id| *id != bridge_id);
            }
        }
        
        tracing::info!("Bridge {} reverted", bridge_id);
        
        Ok(())
    }
    
    /// Timeout olmuş bridge'leri bul
    pub fn find_expired_bridges(&self, current_time: Timestamp) -> Vec<u64> {
        let bridges = self.bridges.read();
        
        bridges
            .iter()
            .filter(|(_, tx)| {
                tx.status != BridgeStatus::Finalized && 
                tx.status != BridgeStatus::Reverted &&
                tx.is_expired(current_time)
            })
            .map(|(id, _)| *id)
            .collect()
    }
    
    /// Bridge'i getir
    pub fn get_bridge(&self, bridge_id: u64) -> Option<BridgeTransaction> {
        self.bridges.read().get(&bridge_id).cloned()
    }
    
    /// Shard'daki pending bridge'leri getir
    pub fn get_pending_for_shard(&self, shard: ShardId) -> Vec<BridgeTransaction> {
        let pending = self.pending_by_shard.read();
        let bridges = self.bridges.read();
        
        pending
            .get(&shard)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| bridges.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }
    
    /// Tüm pending bridge'leri getir
    pub fn get_all_pending(&self) -> Vec<BridgeTransaction> {
        let bridges = self.bridges.read();
        
        bridges
            .values()
            .filter(|tx| {
                tx.status == BridgeStatus::Pending ||
                tx.status == BridgeStatus::Locked ||
                tx.status == BridgeStatus::Relayed
            })
            .cloned()
            .collect()
    }
    
    /// İstatistikleri getir
    pub fn stats(&self) -> BridgeStats {
        let bridges = self.bridges.read();
        
        let mut pending = 0;
        let mut locked = 0;
        let mut relayed = 0;
        let mut finalized = 0;
        let mut reverted = 0;
        
        for tx in bridges.values() {
            match tx.status {
                BridgeStatus::Pending => pending += 1,
                BridgeStatus::Locked => locked += 1,
                BridgeStatus::Relayed => relayed += 1,
                BridgeStatus::Finalized => finalized += 1,
                BridgeStatus::Reverting | BridgeStatus::Reverted => reverted += 1,
                _ => {}
            }
        }
        
        BridgeStats {
            total_bridges: bridges.len() as u64,
            pending,
            locked,
            relayed,
            finalized,
            reverted,
            total_bridged: *self.total_bridged.read(),
            total_failed: *self.total_failed.read(),
        }
    }
}

impl Default for BridgeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct BridgeStats {
    pub total_bridges: u64,
    pub pending: u64,
    pub locked: u64,
    pub relayed: u64,
    pub finalized: u64,
    pub reverted: u64,
    pub total_bridged: u128,
    pub total_failed: u64,
}

impl std::fmt::Display for BridgeStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Bridge: {} total, {} pending, {} locked, {} relayed, {} finalized, {} reverted",
            self.total_bridges,
            self.pending,
            self.locked,
            self.relayed,
            self.finalized,
            self.reverted
        )
    }
}

// ===================================================================
// BRIDGE REAPER (Arka plan temizleyici)
// ===================================================================

pub struct BridgeReaper {
    bridge_manager: Arc<BridgeManager>,
    interval_secs: u64,
}

impl BridgeReaper {
    pub fn new(bridge_manager: Arc<BridgeManager>, interval_secs: u64) -> Self {
        Self {
            bridge_manager,
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
                let expired = self.bridge_manager.find_expired_bridges(now);
                
                for bridge_id in expired {
                    if let Err(e) = self.bridge_manager.revert_bridge(bridge_id) {
                        tracing::error!("Failed to revert bridge {}: {}", bridge_id, e);
                    } else {
                        let _ = self.bridge_manager.mark_reverted(bridge_id);
                        tracing::info!("♻️ Bridge {} expired and reverted", bridge_id);
                    }
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

    #[test]
    fn test_initiate_bridge() {
        let manager = BridgeManager::new();
        
        let sender = [1u8; 32];
        let recipient = [2u8; 32];
        
        let tx = manager.initiate_bridge(
            sender,
            recipient,
            1_000_000,
            1,
            2,
        ).unwrap();
        
        assert_eq!(tx.id, 1);
        assert_eq!(tx.status, BridgeStatus::Pending);
        assert_eq!(tx.source_shard, 1);
        assert_eq!(tx.target_shard, 2);
    }
    
    #[test]
    fn test_bridge_lifecycle() {
        let manager = BridgeManager::new();
        
        let tx = manager.initiate_bridge([1u8; 32], [2u8; 32], 1_000_000, 1, 2).unwrap();
        let id = tx.id;
        
        // Lock
        manager.lock_bridge(id).unwrap();
        assert_eq!(manager.get_bridge(id).unwrap().status, BridgeStatus::Locked);
        
        // Relay
        manager.relay_bridge(id).unwrap();
        assert_eq!(manager.get_bridge(id).unwrap().status, BridgeStatus::Relayed);
        
        // Finalize
        manager.finalize_bridge(id).unwrap();
        assert_eq!(manager.get_bridge(id).unwrap().status, BridgeStatus::Finalized);
    }
    
    #[test]
    fn test_expired_bridges() {
        let manager = BridgeManager::new();
        
        let tx = manager.initiate_bridge([1u8; 32], [2u8; 32], 1_000_000, 1, 2).unwrap();
        
        let future = current_timestamp() + BRIDGE_TIMEOUT_SECS + 10;
        let expired = manager.find_expired_bridges(future);
        
        assert!(expired.contains(&tx.id));
    }
    
    #[test]
    fn test_bridge_stats() {
        let manager = BridgeManager::new();
        
        manager.initiate_bridge([1u8; 32], [2u8; 32], 1_000_000, 1, 2).unwrap();
        manager.initiate_bridge([3u8; 32], [4u8; 32], 2_000_000, 1, 3).unwrap();
        
        let stats = manager.stats();
        assert_eq!(stats.total_bridges, 2);
        assert_eq!(stats.pending, 2);
    }
}