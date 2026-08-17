// ===================================================================
// PACYTE NEXUS - MEMPOOL VALIDATOR
// ===================================================================

use std::sync::Arc;

use crate::types::{Address, PacyteResult};
use crate::types::transaction::{Transaction, TxValidationResult};
use crate::storage::StateManager;
use crate::crypto::{Ed25519Verifier, Dilithium5Verifier};

use super::MempoolConfig;

// ===================================================================
// MEMPOOL VALIDATOR
// ===================================================================

pub struct MempoolValidator {
    config: MempoolConfig,
    state: Arc<StateManager>,
}

impl MempoolValidator {
    pub fn new(config: MempoolConfig, state: Arc<StateManager>) -> Self {
        Self { config, state }
    }
    
    /// İmza doğrulama
    pub async fn verify_signature(&self, tx: &Transaction) -> bool {
        if tx.signature.is_empty() {
            return false;
        }
        
        // İmza tipini belirle
        match tx.signature.len() {
            64 => {
                // Ed25519
                Ed25519Verifier::verify(
                    &tx.sighash(),
                    &tx.signature,
                    &tx.from,
                )
            }
            4595 => {
                // Dilithium5
                Dilithium5Verifier::verify(
                    &tx.sighash(),
                    &tx.signature,
                    &tx.from,
                )
            }
            _ => false,
        }
    }
    
    /// Bakiye kontrolü
    pub async fn check_balance(&self, tx: &Transaction) -> bool {
        let balance = self.state.get_balance(&tx.from).await.unwrap_or(0);
        balance >= tx.total_cost()
    }
    
    /// Nonce kontrolü
    pub async fn check_nonce(&self, tx: &Transaction) -> TxValidationResult {
        let current_nonce = self.state.get_nonce(&tx.from).await.unwrap_or(0);
        
        if tx.nonce < current_nonce {
            TxValidationResult::InvalidNonce {
                expected: current_nonce,
                got: tx.nonce,
            }
        } else if tx.nonce > current_nonce + self.config.max_nonce_gap {
            TxValidationResult::InvalidNonce {
                expected: current_nonce,
                got: tx.nonce,
            }
        } else {
            TxValidationResult::Valid
        }
    }
    
    /// Fee kontrolü
    pub fn check_fee(&self, tx: &Transaction) -> bool {
        let fee_per_byte = tx.fee / tx.size() as u128;
        fee_per_byte >= self.config.min_fee_per_byte as u128
    }
    
    /// Tam validasyon
    pub async fn validate(&self, tx: &Transaction) -> TxValidationResult {
        // Temel validasyon
        if !tx.validate_basic(self.config.max_tx_age_secs) {
            return TxValidationResult::Other("Basic validation failed".to_string());
        }
        
        // İmza
        if !self.verify_signature(tx).await {
            return TxValidationResult::InvalidSignature;
        }
        
        // Bakiye
        if !self.check_balance(tx).await {
            let balance = self.state.get_balance(&tx.from).await.unwrap_or(0);
            return TxValidationResult::InsufficientBalance {
                required: tx.total_cost(),
                available: balance,
            };
        }
        
        // Nonce
        let nonce_result = self.check_nonce(tx).await;
        if !nonce_result.is_valid() {
            return nonce_result;
        }
        
        // Fee
        if !self.check_fee(tx) {
            return TxValidationResult::FeeTooLow {
                fee: tx.fee,
                min_fee: (self.config.min_fee_per_byte as u128) * tx.size() as u128,
            };
        }
        
        // Dormancy kontrolü
        if let Some(account) = self.state.get_account(&tx.from).await.unwrap() {
            if account.is_dormant {
                return TxValidationResult::AccountDormant { years: 6 };
            }
        }
        
        TxValidationResult::Valid
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStorage, StateManager};
    use crate::crypto::Ed25519Signer;
    use crate::types::account::Account;

    #[tokio::test]
    async fn test_validate_transaction() {
        let storage = Arc::new(MemoryStorage::new());
        let state = Arc::new(StateManager::new(storage));
        
        let alice = Ed25519Signer::generate();
        let bob = Ed25519Signer::generate();
        
        // Alice'e bakiye ver
        let mut account = Account::new(alice.address(), 10000);
        state.set_account(alice.address(), account).await.unwrap();
        
        let config = MempoolConfig::default();
        let validator = MempoolValidator::new(config, state);
        
        let mut tx = Transaction::new(
            alice.address(),
            bob.address(),
            1000,
            10,
            0,
        );
        
        // İmzasız geçersiz
        let result = validator.validate(&tx).await;
        assert!(!result.is_valid());
        
        // İmzala
        let sig = alice.sign(&tx.sighash());
        tx.sign(sig);
        
        // Geçerli olmalı
        let result = validator.validate(&tx).await;
        assert!(result.is_valid());
    }
}