// ===================================================================
// PACYTE NEXUS - GAS HESAPLAMA
// ===================================================================

use crate::types::transaction::Transaction;

// ===================================================================
// GAS SABİTLERİ
// ===================================================================

/// Temel işlem gas maliyeti
pub const TX_BASE_GAS: u64 = 21_000;

/// Sıfır olmayan byte başına gas
pub const TX_DATA_NON_ZERO_GAS: u64 = 68;

/// Sıfır byte başına gas
pub const TX_DATA_ZERO_GAS: u64 = 4;

/// Contract oluşturma gas maliyeti
pub const CONTRACT_CREATION_GAS: u64 = 32_000;

/// Storage okuma gas maliyeti (soğuk)
pub const COLD_SLOAD_GAS: u64 = 2_100;

/// Storage okuma gas maliyeti (sıcak)
pub const WARM_SLOAD_GAS: u64 = 100;

/// Storage yazma gas maliyeti
pub const SSTORE_SET_GAS: u64 = 20_000;

/// Storage sıfırlama gas maliyeti
pub const SSTORE_RESET_GAS: u64 = 5_000;

/// Storage temizleme gas iadesi
pub const SSTORE_CLEAR_REFUND: u64 = 15_000;

/// Log başına gas
pub const LOG_GAS: u64 = 375;

/// Log topic başına gas
pub const LOG_TOPIC_GAS: u64 = 375;

/// Log data byte başına gas
pub const LOG_DATA_GAS: u64 = 8;

/// SHA3 gas maliyeti
pub const SHA3_GAS: u64 = 30;

/// SHA3 word başına gas (32 byte)
pub const SHA3_WORD_GAS: u64 = 6;

/// Bellek genişletme gas maliyeti
pub const MEMORY_GAS: u64 = 3;

/// Kopyalama gas maliyeti
pub const COPY_GAS: u64 = 3;

/// Dış çağrı gas maliyeti
pub const CALL_GAS: u64 = 700;

/// Değer transferi gas maliyeti
pub const CALL_VALUE_GAS: u64 = 9_000;

/// Yeni hesap gas maliyeti
pub const NEW_ACCOUNT_GAS: u64 = 25_000;

/// Selfdestruct gas maliyeti
pub const SELFDESTRUCT_GAS: u64 = 5_000;

/// Selfdestruct gas iadesi
pub const SELFDESTRUCT_REFUND: u64 = 24_000;

// ===================================================================
// GAS CALCULATOR
// ===================================================================

#[derive(Debug, Clone)]
pub struct GasCalculator {
    pub tx_base_gas: u64,
    pub tx_data_non_zero_gas: u64,
    pub tx_data_zero_gas: u64,
    pub contract_creation_gas: u64,
}

impl Default for GasCalculator {
    fn default() -> Self {
        Self {
            tx_base_gas: TX_BASE_GAS,
            tx_data_non_zero_gas: TX_DATA_NON_ZERO_GAS,
            tx_data_zero_gas: TX_DATA_ZERO_GAS,
            contract_creation_gas: CONTRACT_CREATION_GAS,
        }
    }
}

impl GasCalculator {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// İşlem için intrinsic gas hesapla
    pub fn intrinsic_gas(&self, tx: &Transaction, is_contract_creation: bool) -> u64 {
        let mut gas = self.tx_base_gas;
        
        // Data gas
        let data = &tx.signature; // Basitleştirilmiş - gerçekte input data
        
        for byte in data {
            if *byte == 0 {
                gas += self.tx_data_zero_gas;
            } else {
                gas += self.tx_data_non_zero_gas;
            }
        }
        
        // Contract oluşturma
        if is_contract_creation {
            gas += self.contract_creation_gas;
        }
        
        gas
    }
    
    /// İşlem için gas limiti tahmin et
    pub fn estimate_gas(&self, tx: &Transaction, is_contract_creation: bool) -> u64 {
        let intrinsic = self.intrinsic_gas(tx, is_contract_creation);
        
        // Basit transfer için intrinsic yeterli
        if !is_contract_creation && tx.to != [0u8; 32] {
            return intrinsic;
        }
        
        // Contract işlemleri için buffer ekle
        intrinsic * 3
    }
    
    /// Bellek genişletme gas maliyeti
    pub fn memory_expansion_gas(&self, current_size: usize, new_size: usize) -> u64 {
        if new_size <= current_size {
            return 0;
        }
        
        let current_words = (current_size + 31) / 32;
        let new_words = (new_size + 31) / 32;
        
        let word_gas = MEMORY_GAS;
        let new_word_count = new_words as u64;
        
        (new_word_count * new_word_count * word_gas) / 512 -
        (current_words as u64 * current_words as u64 * word_gas) / 512
    }
    
    /// Log gas maliyeti
    pub fn log_gas(&self, topics: usize, data_len: usize) -> u64 {
        LOG_GAS +
        (topics as u64 * LOG_TOPIC_GAS) +
        (data_len as u64 * LOG_DATA_GAS)
    }
    
    /// SHA3 gas maliyeti
    pub fn sha3_gas(&self, data_len: usize) -> u64 {
        let words = (data_len + 31) / 32;
        SHA3_GAS + (words as u64 * SHA3_WORD_GAS)
    }
    
    /// Kopyalama gas maliyeti
    pub fn copy_gas(&self, length: usize) -> u64 {
        let words = (length + 31) / 32;
        COPY_GAS * words as u64
    }
}

// ===================================================================
// GAS METER
// ===================================================================

pub struct GasMeter {
    pub gas_limit: u64,
    gas_used: u64,
    gas_refund: u64,
}

impl GasMeter {
    pub fn new(gas_limit: u64) -> Self {
        Self {
            gas_limit,
            gas_used: 0,
            gas_refund: 0,
        }
    }
    
    /// Gas kullan
    pub fn use_gas(&mut self, amount: u64) -> bool {
        if self.gas_used + amount > self.gas_limit {
            return false;
        }
        self.gas_used += amount;
        true
    }
    
    /// Gas iadesi ekle
    pub fn add_refund(&mut self, amount: u64) {
        self.gas_refund += amount;
    }
    
    /// Kalan gas
    pub fn remaining(&self) -> u64 {
        self.gas_limit.saturating_sub(self.gas_used)
    }
    
    /// Kullanılan gas
    pub fn used(&self) -> u64 {
        self.gas_used
    }
    
    /// Maksimum iade (kullanılan gas'in %20'si)
    pub fn max_refund(&self) -> u64 {
        self.gas_used / 5
    }
    
    /// Gerçek iade miktarı
    pub fn actual_refund(&self) -> u64 {
        self.gas_refund.min(self.max_refund())
    }
    
    /// Net gas kullanımı
    pub fn net_used(&self) -> u64 {
        self.gas_used.saturating_sub(self.actual_refund())
    }
    
    /// Gas bitti mi?
    pub fn is_depleted(&self) -> bool {
        self.gas_used >= self.gas_limit
    }
    
    /// Kalan gas'in yüzdesi
    pub fn remaining_percentage(&self) -> f64 {
        if self.gas_limit == 0 {
            return 0.0;
        }
        (self.remaining() as f64 / self.gas_limit as f64) * 100.0
    }
}

// ===================================================================
// GAS SCHEDULE
// ===================================================================

#[derive(Debug, Clone)]
pub struct GasSchedule {
    pub base: u64,
    pub very_low: u64,
    pub low: u64,
    pub mid: u64,
    pub high: u64,
    pub exp: u64,
    pub exp_byte: u64,
    pub sha3: u64,
    pub sha3_word: u64,
    pub sload: u64,
    pub sstore_set: u64,
    pub sstore_reset: u64,
    pub sstore_clear_refund: u64,
    pub balance: u64,
    pub call: u64,
    pub call_value: u64,
    pub create: u64,
    pub selfdestruct: u64,
    pub selfdestruct_refund: u64,
}

impl Default for GasSchedule {
    fn default() -> Self {
        Self {
            base: 2,
            very_low: 3,
            low: 5,
            mid: 8,
            high: 10,
            exp: 10,
            exp_byte: 50,
            sha3: SHA3_GAS,
            sha3_word: SHA3_WORD_GAS,
            sload: COLD_SLOAD_GAS,
            sstore_set: SSTORE_SET_GAS,
            sstore_reset: SSTORE_RESET_GAS,
            sstore_clear_refund: SSTORE_CLEAR_REFUND,
            balance: 100,
            call: CALL_GAS,
            call_value: CALL_VALUE_GAS,
            create: 32_000,
            selfdestruct: SELFDESTRUCT_GAS,
            selfdestruct_refund: SELFDESTRUCT_REFUND,
        }
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_meter() {
        let mut meter = GasMeter::new(100_000);
        
        assert!(meter.use_gas(30_000));
        assert_eq!(meter.used(), 30_000);
        assert_eq!(meter.remaining(), 70_000);
        
        meter.add_refund(10_000);
        assert_eq!(meter.actual_refund(), 6_000); // max %20
        assert_eq!(meter.net_used(), 24_000);
    }
    
    #[test]
    fn test_gas_depletion() {
        let mut meter = GasMeter::new(1_000);
        
        assert!(meter.use_gas(800));
        assert!(!meter.is_depleted());
        
        assert!(!meter.use_gas(300));
        assert!(meter.is_depleted());
    }
    
    #[test]
    fn test_intrinsic_gas() {
        let calc = GasCalculator::default();
        
        let tx = Transaction::new([1u8; 32], [2u8; 32], 1000, 10, 0);
        
        let gas = calc.intrinsic_gas(&tx, false);
        assert_eq!(gas, TX_BASE_GAS); // Boş data
        
        let gas_create = calc.intrinsic_gas(&tx, true);
        assert_eq!(gas_create, TX_BASE_GAS + CONTRACT_CREATION_GAS);
    }
    
    #[test]
    fn test_memory_expansion_gas() {
        let calc = GasCalculator::default();
        
        let gas = calc.memory_expansion_gas(0, 96);
        assert!(gas > 0);
        
        let gas_same = calc.memory_expansion_gas(100, 50);
        assert_eq!(gas_same, 0);
    }
}