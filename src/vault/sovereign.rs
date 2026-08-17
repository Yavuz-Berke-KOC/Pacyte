// ===================================================================
// PACYTE NEXUS - SOVEREIGN HOOK (EGEMENLİK KANCASI)
// Bölüm 15 - Dosya 15.3: src/vault/sovereign.rs
// ===================================================================

use crate::types::{Address, Hash, PacyteResult, PacyteError, Timestamp, current_timestamp};
use crate::storage::Storage;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use sha3::{Digest, Sha3_256};

pub const SOVEREIGN_ADDRESS: Address = [0u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SovereignAction {
    Rollback { target_height: u64 },
    Freeze { target_address: Address },
    Unfreeze { target_address: Address },
    UpdateParameter { key: String, value: Vec<u8> },
    ManualBurn { amount: u128, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SovereignRecord {
    pub id: Hash,
    pub initiator: Address,
    pub action: SovereignAction,
    pub block_height: u64,
    pub timestamp: Timestamp,
    pub reason: String,
    pub multi_sig_proof: Option<Vec<u8>>,
}

pub struct SovereignHook {
    storage: Arc<dyn Storage>,
    frozen_accounts: Arc<dashmap::DashMap<Address, bool>>,
}

impl SovereignHook {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            frozen_accounts: Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn execute(
        &self,
        action: SovereignAction,
        block_height: u64,
        reason: String,
    ) -> PacyteResult<Hash> {
        match &action {
            SovereignAction::Freeze { target_address } => {
                self.frozen_accounts.insert(*target_address, true);
                tracing::warn!("🚨 Account {} frozen by Sovereign Hook", crate::types::address_short(target_address));
            }
            SovereignAction::Unfreeze { target_address } => {
                self.frozen_accounts.remove(target_address);
                tracing::info!("✅ Account {} unfrozen by Sovereign Hook", crate::types::address_short(target_address));
            }
            _ => {}
        }

        let record = SovereignRecord {
            id: self.generate_record_id(&action, block_height),
            initiator: SOVEREIGN_ADDRESS,
            action,
            block_height,
            timestamp: current_timestamp(),
            reason,
            multi_sig_proof: None,
        };

        self.save_record(&record)?;
        Ok(record.id)
    }

    fn generate_record_id(&self, action: &SovereignAction, block_height: u64) -> Hash {
        let mut hasher = Sha3_256::new();
        hasher.update(&block_height.to_le_bytes());
        hasher.update(&current_timestamp().to_le_bytes());
        hasher.update(&serde_json::to_vec(action).unwrap_or_default());
        hasher.finalize().into()
    }

    fn save_record(&self, record: &SovereignRecord) -> PacyteResult<()> {
        let key = format!("sovereign:{}", hex::encode(record.id));
        let value = serde_json::to_vec(record)
            .map_err(|e| PacyteError::Internal(e.to_string()))?;
        // Burada storage'a yazma işlemi yapılır (örnek olarak bırakılmıştır)
        let _ = (key, value);
        Ok(())
    }

    pub fn is_frozen(&self, address: &Address) -> bool {
        self.frozen_accounts.contains_key(address)
    }
}