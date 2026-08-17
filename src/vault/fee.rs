// ===================================================================
// PACYTE NEXUS - FEE YÖNETİMİ
// ===================================================================

use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::types::{PacyteError, PacyteResult, BlockHeight, Timestamp, current_timestamp};
use super::{SupplyPhase, TOTAL_SUPPLY};

// ===================================================================
// FEE CONFIG
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfig {
    pub base_fee: u128,
    pub min_fee: u128,
    pub fee_per_byte: u128,
    pub congestion_multiplier: f64,
    pub max_fee_multiplier: f64,
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            base_fee: 1_000,           // 0.001 PAC
            min_fee: 100,              // 0.0001 PAC
            fee_per_byte: 1,           // 0.000001 PAC per byte
            congestion_multiplier: 1.0,
            max_fee_multiplier: 10.0,
        }
    }
}

// ===================================================================
// FEE MANAGER
// ===================================================================

pub struct FeeManager {
    config: Arc<RwLock<FeeConfig>>,
    fee_history: Arc<RwLock<Vec<FeeRecord>>>,
    total_fees_collected: Arc<RwLock<u128>>,
    total_fees_burned: Arc<RwLock<u128>>,
    total_fees_distributed: Arc<RwLock<u128>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeRecord {
    pub block_height: BlockHeight,
    pub total_fee: u128,
    pub burned: u128,
    pub to_validators: u128,
    pub to_genesis: u128,
    pub avg_fee_per_tx: u128,
    pub tx_count: usize,
    pub timestamp: Timestamp,
    pub phase: SupplyPhase,
}

impl FeeManager {
    pub fn new(config: FeeConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            fee_history: Arc::new(RwLock::new(Vec::new())),
            total_fees_collected: Arc::new(RwLock::new(0)),
            total_fees_burned: Arc::new(RwLock::new(0)),
            total_fees_distributed: Arc::new(RwLock::new(0)),
        }
    }
    
    /// İşlem için fee hesapla
    pub fn calculate_fee(&self, tx_size: usize, network_load: f64) -> u128 {
        let config = self.config.read();
        
        let base = config.base_fee;
        let byte_fee = tx_size as u128 * config.fee_per_byte;
        let base_fee = base.max(byte_fee).max(config.min_fee);
        
        // Ağ yüküne göre çarpan
        let load_multiplier = 1.0 + (network_load * 2.0);
        let multiplier = (load_multiplier * config.congestion_multiplier)
            .min(config.max_fee_multiplier);
        
        (base_fee as f64 * multiplier) as u128
    }
    
    /// Önerilen fee'yi hesapla (kullanıcılar için)
    pub fn recommended_fee(&self, tx_size: usize, priority: FeePriority) -> u128 {
        let config = self.config.read();
        let base = self.calculate_fee(tx_size, 0.5);
        
        match priority {
            FeePriority::Low => base / 2,
            FeePriority::Normal => base,
            FeePriority::High => base * 2,
            FeePriority::Urgent => base * 5,
        }
    }
    
    /// Blok sonunda fee kaydı oluştur
    pub fn record_block_fees(
        &self,
        block_height: BlockHeight,
        total_fee: u128,
        burned: u128,
        to_validators: u128,
        to_genesis: u128,
        tx_count: usize,
    ) {
        let avg_fee = if tx_count > 0 {
            total_fee / tx_count as u128
        } else {
            0
        };
        
        let record = FeeRecord {
            block_height,
            total_fee,
            burned,
            to_validators,
            to_genesis,
            avg_fee_per_tx: avg_fee,
            tx_count,
            timestamp: current_timestamp(),
            phase: SupplyPhase::from_supply(TOTAL_SUPPLY),
        };
        
        self.fee_history.write().push(record);
        
        *self.total_fees_collected.write() += total_fee;
        *self.total_fees_burned.write() += burned;
        *self.total_fees_distributed.write() += to_validators + to_genesis;
    }
    
    /// Congestion durumunu güncelle
    pub fn update_congestion(&self, mempool_size: usize, max_mempool: usize) {
        let mut config = self.config.write();
        config.congestion_multiplier = 1.0 + (mempool_size as f64 / max_mempool as f64) * 3.0;
    }
    
    /// Fee konfigürasyonunu güncelle
    pub fn update_config(&self, new_config: FeeConfig) {
        *self.config.write() = new_config;
    }
    
    /// Fee istatistiklerini getir
    pub fn stats(&self) -> FeeStats {
        let history = self.fee_history.read();
        
        let avg_fee = if history.is_empty() {
            0
        } else {
            history.iter().map(|r| r.avg_fee_per_tx).sum::<u128>() / history.len() as u128
        };
        
        FeeStats {
            total_collected: *self.total_fees_collected.read(),
            total_burned: *self.total_fees_burned.read(),
            total_distributed: *self.total_fees_distributed.read(),
            current_base_fee: self.config.read().base_fee,
            current_congestion: self.config.read().congestion_multiplier,
            average_fee: avg_fee,
            history_length: history.len(),
        }
    }
    
    /// Belirli bir blok aralığındaki fee'leri getir
    pub fn get_fees_in_range(&self, start: BlockHeight, end: BlockHeight) -> Vec<FeeRecord> {
        self.fee_history.read()
            .iter()
            .filter(|r| r.block_height >= start && r.block_height <= end)
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeePriority {
    Low,
    Normal,
    High,
    Urgent,
}

#[derive(Debug, Clone)]
pub struct FeeStats {
    pub total_collected: u128,
    pub total_burned: u128,
    pub total_distributed: u128,
    pub current_base_fee: u128,
    pub current_congestion: f64,
    pub average_fee: u128,
    pub history_length: usize,
}

impl std::fmt::Display for FeeStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Fees: collected={} PAC, burned={} PAC, distributed={} PAC, base={}, congestion={:.2}",
            self.total_collected,
            self.total_burned,
            self.total_distributed,
            self.current_base_fee,
            self.current_congestion
        )
    }
}

impl Default for FeeManager {
    fn default() -> Self {
        Self::new(FeeConfig::default())
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_fee() {
        let manager = FeeManager::default();
        
        let fee = manager.calculate_fee(250, 0.5);
        assert!(fee >= 100); // min_fee
        
        let fee_high_load = manager.calculate_fee(250, 0.9);
        assert!(fee_high_load > fee);
    }
    
    #[test]
    fn test_recommended_fee() {
        let manager = FeeManager::default();
        
        let normal = manager.recommended_fee(250, FeePriority::Normal);
        let urgent = manager.recommended_fee(250, FeePriority::Urgent);
        
        assert!(urgent > normal);
        assert_eq!(urgent, normal * 5);
    }
    
    #[test]
    fn test_record_fees() {
        let manager = FeeManager::default();
        
        manager.record_block_fees(100, 1_000_000, 500_000, 400_000, 100_000, 10);
        
        let stats = manager.stats();
        assert_eq!(stats.total_collected, 1_000_000);
        assert_eq!(stats.total_burned, 500_000);
        assert_eq!(stats.history_length, 1);
    }
    
    #[test]
    fn test_update_congestion() {
        let manager = FeeManager::default();
        
        manager.update_congestion(5000, 10000);
        
        let config = manager.config.read();
        assert!(config.congestion_multiplier > 1.0);
    }
}