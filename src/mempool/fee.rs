// ===================================================================
// PACYTE NEXUS - FEE HESAPLAMA
// ===================================================================

use crate::types::transaction::Transaction;
use crate::types::BlockHeight;

// ===================================================================
// FEE CALCULATOR
// ===================================================================

pub struct FeeCalculator {
    base_fee: u128,
    min_fee: u128,
    fee_per_byte: u128,
    congestion_multiplier: f64,
}

impl FeeCalculator {
    pub fn new() -> Self {
        Self {
            base_fee: 1000,
            min_fee: 100,
            fee_per_byte: 1,
            congestion_multiplier: 1.0,
        }
    }
    
    /// İşlem için minimum fee hesapla
    pub fn calculate_min_fee(&self, tx_size: usize) -> u128 {
        let byte_fee = tx_size as u128 * self.fee_per_byte;
        self.base_fee.max(byte_fee).max(self.min_fee)
    }
    
    /// Önerilen fee'yi hesapla (ağ yoğunluğuna göre)
    pub fn calculate_recommended_fee(&self, tx_size: usize, mempool_size: usize, max_mempool: usize) -> u128 {
        let congestion = mempool_size as f64 / max_mempool as f64;
        let multiplier = 1.0 + congestion * 2.0;
        
        let min_fee = self.calculate_min_fee(tx_size);
        (min_fee as f64 * multiplier) as u128
    }
    
    /// Priority fee hesapla (öncelikli işlemler için)
    pub fn calculate_priority_fee(&self, tx_size: usize, target_blocks: u32) -> u128 {
        // Hedef blok sayısına göre fee çarpanı
        let multiplier = match target_blocks {
            1 => 5.0,  // Sonraki blok
            2 => 3.0,
            3 => 2.0,
            _ => 1.5,
        };
        
        let min_fee = self.calculate_min_fee(tx_size);
        (min_fee as f64 * multiplier) as u128
    }
    
    /// Fee piyasası analizi
    pub fn analyze_fee_market(&self, pending_txs: &[Transaction]) -> FeeMarketStats {
        if pending_txs.is_empty() {
            return FeeMarketStats::default();
        }
        
        let mut fees: Vec<f64> = pending_txs
            .iter()
            .map(|tx| tx.fee as f64 / tx.size() as f64)
            .collect();
        
        fees.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let median = fees[fees.len() / 2];
        let avg = fees.iter().sum::<f64>() / fees.len() as f64;
        let min = fees[0];
        let max = fees[fees.len() - 1];
        
        // Percentiles
        let p25 = fees[fees.len() / 4];
        let p75 = fees[fees.len() * 3 / 4];
        let p90 = fees[fees.len() * 9 / 10];
        
        FeeMarketStats {
            median_fee_per_byte: median,
            average_fee_per_byte: avg,
            min_fee_per_byte: min,
            max_fee_per_byte: max,
            p25_fee_per_byte: p25,
            p75_fee_per_byte: p75,
            p90_fee_per_byte: p90,
            total_pending: pending_txs.len(),
        }
    }
    
    /// Congestion durumunu güncelle
    pub fn update_congestion(&mut self, mempool_size: usize, max_mempool: usize) {
        self.congestion_multiplier = 1.0 + (mempool_size as f64 / max_mempool as f64) * 3.0;
    }
    
    /// Mevcut base fee'yi getir
    pub fn base_fee(&self) -> u128 {
        (self.base_fee as f64 * self.congestion_multiplier) as u128
    }
}

#[derive(Debug, Clone, Default)]
pub struct FeeMarketStats {
    pub median_fee_per_byte: f64,
    pub average_fee_per_byte: f64,
    pub min_fee_per_byte: f64,
    pub max_fee_per_byte: f64,
    pub p25_fee_per_byte: f64,
    pub p75_fee_per_byte: f64,
    pub p90_fee_per_byte: f64,
    pub total_pending: usize,
}

impl FeeMarketStats {
    pub fn recommended_fee_for_priority(&self, priority: FeePriority) -> f64 {
        match priority {
            FeePriority::Low => self.p25_fee_per_byte,
            FeePriority::Normal => self.median_fee_per_byte,
            FeePriority::High => self.p75_fee_per_byte,
            FeePriority::Urgent => self.p90_fee_per_byte,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeePriority {
    Low,
    Normal,
    High,
    Urgent,
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_min_fee() {
        let calc = FeeCalculator::new();
        
        let fee = calc.calculate_min_fee(250);
        assert!(fee >= 250);
        assert!(fee >= calc.min_fee);
    }
    
    #[test]
    fn test_fee_market_stats() {
        let calc = FeeCalculator::new();
        
        let stats = calc.analyze_fee_market(&[]);
        assert_eq!(stats.total_pending, 0);
    }
}