// ===================================================================
// PACYTE NEXUS - VAULT MODÜLÜ
// ===================================================================

pub mod manager;
pub mod burn;
pub mod dormancy;
pub mod bridge;
pub mod fee;
pub mod sovereign;

// Re-export'lar
pub use manager::*;
pub use burn::*;
pub use dormancy::*;
pub use bridge::*;
pub use fee::*;
pub use sovereign::*;

use crate::types::{
    PacyteError, PacyteResult, Address, BlockHeight, Hash, Timestamp,
};
use crate::types::account::Account;
use std::sync::Arc;

// ===================================================================
// VAULT SABİTLERİ
// ===================================================================

/// Genesis Sovereign Vault adresi
pub const GENESIS_VAULT_ADDRESS: Address = [0u8; 32];

/// Toplam arz (550 milyon PAC, 6 decimal)
pub const TOTAL_SUPPLY: u128 = 550_000_000_000_000;

/// Genesis bakiyesi (122.5 milyon PAC)
pub const GENESIS_BALANCE: u128 = 122_500_000_000_000;

/// Minimum bakiye (dust threshold)
pub const MIN_BALANCE: u128 = 1_000; // 0.001 PAC

/// Dormancy eşiği (6 yıl)
pub const DORMANCY_YEARS: u32 = 6;
pub const DORMANCY_SECONDS: u64 = DORMANCY_YEARS as u64 * 365 * 24 * 60 * 60;

// ===================================================================
// SUPPLY PHASE
// ===================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SupplyPhase {
    /// Faz 1: Agresif Deflasyon (550M → 400M)
    GreatBurn,
    /// Faz 2: Yapısal Geçiş (400M → 250M)
    Transition,
    /// Faz 3: Altın Çağ (250M sabit)
    GoldenEra,
}

impl SupplyPhase {
    pub fn from_supply(supply: u128) -> Self {
        match supply {
            s if s > 400_000_000_000_000 => SupplyPhase::GreatBurn,
            s if s > 250_000_000_000_000 => SupplyPhase::Transition,
            _ => SupplyPhase::GoldenEra,
        }
    }
    
    pub fn burn_rate(&self, supply: u128) -> f64 {
        match self {
            SupplyPhase::GreatBurn => {
                let progress = (TOTAL_SUPPLY - supply) as f64 / 150_000_000_000_000.0;
                0.60 + progress * 0.30 // %60 → %90
            }
            SupplyPhase::Transition => {
                let progress = (supply - 250_000_000_000_000) as f64 / 150_000_000_000_000.0;
                0.90 * progress // %90 → %0
            }
            SupplyPhase::GoldenEra => 0.0,
        }
    }
    
    pub fn node_rate(&self, supply: u128) -> f64 {
        match self {
            SupplyPhase::GreatBurn => 0.36,
            SupplyPhase::Transition => {
                let burn = self.burn_rate(supply);
                let genesis = self.genesis_rate(supply);
                1.0 - burn - genesis
            }
            SupplyPhase::GoldenEra => 0.90,
        }
    }
    
    pub fn genesis_rate(&self, supply: u128) -> f64 {
        match self {
            SupplyPhase::GreatBurn => {
                let burn = self.burn_rate(supply);
                1.0 - burn - 0.36
            }
            SupplyPhase::Transition => {
                let progress = (400_000_000_000_000 - supply) as f64 / 150_000_000_000_000.0;
                0.01 + progress * 0.09 // %1 → %10
            }
            SupplyPhase::GoldenEra => 0.10,
        }
    }
}

// ===================================================================
// VAULT TRAIT
// ===================================================================

#[async_trait::async_trait]
pub trait Vault: Send + Sync {
    /// Transfer işlemi
    async fn transfer(
        &self,
        from: &Address,
        to: &Address,
        amount: u128,
        fee: u128,
    ) -> PacyteResult<TransferResult>;
    
    /// Bakiye sorgula
    async fn get_balance(&self, address: &Address) -> PacyteResult<u128>;
    
    /// Hesap bilgisi getir
    async fn get_account(&self, address: &Address) -> PacyteResult<Option<Account>>;
    
    /// Toplam arz
    async fn total_supply(&self) -> PacyteResult<u128>;
    
    /// Mevcut faz
    fn current_phase(&self) -> SupplyPhase;
    
    /// Fee dağıtımı yap (blok sonunda)
    async fn distribute_fees(&self, total_fee: u128, block_height: BlockHeight) -> PacyteResult<FeeDistribution>;
    
    /// Burn işlemi
    async fn burn(&self, amount: u128, reason: BurnReason) -> PacyteResult<()>;
    
    /// Dormant hesapları kontrol et ve işle
    async fn process_dormant_accounts(&self, current_time: Timestamp) -> PacyteResult<Vec<Address>>;
    
    /// Bridge işlemi başlat
    async fn initiate_bridge(&self, tx: BridgeTransaction) -> PacyteResult<u64>;
    
    /// Bridge işlemini tamamla
    async fn finalize_bridge(&self, bridge_id: u64) -> PacyteResult<()>;
    
    /// Bridge işlemini geri al
    async fn revert_bridge(&self, bridge_id: u64) -> PacyteResult<()>;
}

// ===================================================================
// TRANSFER SONUCU
// ===================================================================

#[derive(Debug, Clone)]
pub struct TransferResult {
    pub success: bool,
    pub from_balance_after: u128,
    pub to_balance_after: u128,
    pub fee_burned: u128,
    pub fee_to_validator: u128,
    pub fee_to_genesis: u128,
}

// ===================================================================
// FEE DAĞITIMI
// ===================================================================

#[derive(Debug, Clone)]
pub struct FeeDistribution {
    pub total_fee: u128,
    pub burned: u128,
    pub to_validators: u128,
    pub to_genesis: u128,
    pub phase: SupplyPhase,
}

// ===================================================================
// BURN REASON
// ===================================================================

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BurnReason {
    TransactionFee,
    DormantAccount,
    Slashing,
    SovereignDirective,
    BridgeTimeout,
    Other(String),
}

impl std::fmt::Display for BurnReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TransactionFee => write!(f, "transaction_fee"),
            Self::DormantAccount => write!(f, "dormant_account"),
            Self::Slashing => write!(f, "slashing"),
            Self::SovereignDirective => write!(f, "sovereign_directive"),
            Self::BridgeTimeout => write!(f, "bridge_timeout"),
            Self::Other(s) => write!(f, "{}", s),
        }
    }
}