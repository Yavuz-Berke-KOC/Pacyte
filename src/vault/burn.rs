// ===================================================================
// PACYTE NEXUS - BURN MEKANİZMASI
// ===================================================================

use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::types::{PacyteError, PacyteResult, BlockHeight, Timestamp, current_timestamp};
use super::{BurnReason, SupplyPhase, TOTAL_SUPPLY};

// ===================================================================
// BURN MANAGER
// ===================================================================

pub struct BurnManager {
    total_burned: Arc<RwLock<u128>>,
    burn_history: Arc<RwLock<Vec<BurnRecord>>>,
    current_supply: Arc<RwLock<u128>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BurnRecord {
    pub amount: u128,
    pub reason: BurnReason,
    pub timestamp: Timestamp,
    pub block_height: Option<BlockHeight>,
    pub remaining_supply: u128,
    pub phase: SupplyPhase,
}

impl BurnManager {
    pub fn new(initial_supply: u128) -> Self {
        Self {
            total_burned: Arc::new(RwLock::new(0)),
            burn_history: Arc::new(RwLock::new(Vec::new())),
            current_supply: Arc::new(RwLock::new(initial_supply)),
        }
    }
    
    /// Burn işlemi gerçekleştir
    pub fn burn(
        &self,
        amount: u128,
        reason: BurnReason,
        block_height: Option<BlockHeight>,
    ) -> PacyteResult<BurnRecord> {
        if amount == 0 {
            return Err(PacyteError::InvalidAmount);
        }
        
        let mut supply = self.current_supply.write();
        
        if *supply < amount {
            return Err(PacyteError::InsufficientSupply);
        }
        
        *supply -= amount;
        *self.total_burned.write() += amount;
        
        let phase = SupplyPhase::from_supply(*supply);
        
        let record = BurnRecord {
            amount,
            reason: reason.clone(),
            timestamp: current_timestamp(),
            block_height,
            remaining_supply: *supply,
            phase,
        };
        
        self.burn_history.write().push(record.clone());
        
        tracing::info!(
            "🔥 BURN: {} PAC | Reason: {} | Phase: {:?} | Remaining: {}",
            amount, reason, phase, *supply
        );
        
        Ok(record)
    }
    
    /// Toplam yakılan miktarı getir
    pub fn total_burned(&self) -> u128 {
        *self.total_burned.read()
    }
    
    /// Mevcut arzı getir
    pub fn current_supply(&self) -> u128 {
        *self.current_supply.read()
    }
    
    /// Burn yüzdesini hesapla
    pub fn burn_percentage(&self) -> f64 {
        let burned = self.total_burned();
        let initial = TOTAL_SUPPLY;
        
        if initial == 0 {
            0.0
        } else {
            (burned as f64 / initial as f64) * 100.0
        }
    }
    
    /// Burn geçmişini getir
    pub fn get_history(&self, limit: usize) -> Vec<BurnRecord> {
        let history = self.burn_history.read();
        history.iter().rev().take(limit).cloned().collect()
    }
    
    /// Belirli bir sebep için toplam yakılan
    pub fn total_burned_by_reason(&self, reason: BurnReason) -> u128 {
        self.burn_history.read()
            .iter()
            .filter(|r| r.reason == reason)
            .map(|r| r.amount)
            .sum()
    }
    
    /// Faz bazında yakılan miktarlar
    pub fn burned_by_phase(&self) -> Vec<(SupplyPhase, u128)> {
        let mut phase_totals = vec![
            (SupplyPhase::GreatBurn, 0),
            (SupplyPhase::Transition, 0),
            (SupplyPhase::GoldenEra, 0),
        ];
        
        for record in self.burn_history.read().iter() {
            for (phase, total) in &mut phase_totals {
                if *phase == record.phase {
                    *total += record.amount;
                    break;
                }
            }
        }
        
        phase_totals
    }
    
    /// Deflasyon oranını hesapla (yıllık)
    pub fn annual_deflation_rate(&self) -> f64 {
        let history = self.burn_history.read();
        if history.len() < 2 {
            return 0.0;
        }
        
        let first = &history[0];
        let last = &history[history.len() - 1];
        
        let time_diff_secs = last.timestamp.saturating_sub(first.timestamp);
        if time_diff_secs == 0 {
            return 0.0;
        }
        
        let supply_diff = first.remaining_supply.saturating_sub(last.remaining_supply);
        let annual_factor = 365.0 * 24.0 * 3600.0 / time_diff_secs as f64;
        
        (supply_diff as f64 / first.remaining_supply as f64) * annual_factor * 100.0
    }
    
    /// Hedef arza ulaşma tahmini (gün)
    pub fn estimate_days_to_target(&self, target_supply: u128) -> Option<u64> {
        let current = self.current_supply();
        if current <= target_supply {
            return Some(0);
        }
        
        let history = self.burn_history.read();
        if history.len() < 10 {
            return None;
        }
        
        // Son 10 kaydın ortalama burn miktarı
        let recent: Vec<_> = history.iter().rev().take(10).collect();
        let total_burned: u128 = recent.iter().map(|r| r.amount).sum();
        let time_span = recent.first().unwrap().timestamp - recent.last().unwrap().timestamp;
        
        if time_span == 0 || total_burned == 0 {
            return None;
        }
        
        let burn_per_second = total_burned as f64 / time_span as f64;
        let remaining = (current - target_supply) as f64;
        
        Some((remaining / burn_per_second / 86400.0) as u64)
    }
    
    /// Burn istatistiklerini getir
    pub fn stats(&self) -> BurnStats {
        let history = self.burn_history.read();
        
        BurnStats {
            total_burned: self.total_burned(),
            total_burn_events: history.len() as u64,
            current_supply: self.current_supply(),
            burn_percentage: self.burn_percentage(),
            deflation_rate: self.annual_deflation_rate(),
            largest_burn: history.iter().map(|r| r.amount).max().unwrap_or(0),
            average_burn: if history.is_empty() {
                0.0
            } else {
                self.total_burned() as f64 / history.len() as f64
            },
            phase: SupplyPhase::from_supply(self.current_supply()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BurnStats {
    pub total_burned: u128,
    pub total_burn_events: u64,
    pub current_supply: u128,
    pub burn_percentage: f64,
    pub deflation_rate: f64,
    pub largest_burn: u128,
    pub average_burn: f64,
    pub phase: SupplyPhase,
}

impl std::fmt::Display for BurnStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Burn Stats: total={} PAC ({:.2}%), events={}, supply={} PAC, deflation={:.2}%/year",
            self.total_burned,
            self.burn_percentage,
            self.total_burn_events,
            self.current_supply,
            self.deflation_rate
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
    fn test_burn() {
        let manager = BurnManager::new(TOTAL_SUPPLY);
        
        let record = manager.burn(1_000_000, BurnReason::TransactionFee, Some(100)).unwrap();
        
        assert_eq!(record.amount, 1_000_000);
        assert_eq!(manager.total_burned(), 1_000_000);
        assert_eq!(manager.current_supply(), TOTAL_SUPPLY - 1_000_000);
    }
    
    #[test]
    fn test_insufficient_supply() {
        let manager = BurnManager::new(1000);
        
        let result = manager.burn(2000, BurnReason::TransactionFee, None);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_burn_stats() {
        let manager = BurnManager::new(TOTAL_SUPPLY);
        
        manager.burn(1_000_000, BurnReason::TransactionFee, Some(1)).unwrap();
        manager.burn(2_000_000, BurnReason::TransactionFee, Some(2)).unwrap();
        
        let stats = manager.stats();
        assert_eq!(stats.total_burned, 3_000_000);
        assert_eq!(stats.total_burn_events, 2);
        assert_eq!(stats.largest_burn, 2_000_000);
        assert_eq!(stats.average_burn, 1_500_000.0);
    }
    
    #[test]
    fn test_burn_percentage() {
        let manager = BurnManager::new(1000000);
        manager.burn(250000, BurnReason::TransactionFee, None).unwrap();
        
        assert_eq!(manager.burn_percentage(), 25.0);
    }
}