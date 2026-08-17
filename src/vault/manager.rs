// ===================================================================
// PACYTE NEXUS - VAULT MANAGER (GERÇEK)
// Bölüm 7 - Dosya 7.2: src/vault/manager.rs
// ===================================================================

// manager.rs başına (geçici):
const FOUNDER_VESTING_ADDRESS: Address = [1u8; 32];
const FOUNDER_ALLOCATION: u128 = 55_000_000_000_000;

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use dashmap::DashMap;

use crate::types::{
    PacyteError, PacyteResult, Address, BlockHeight, Hash, Timestamp, current_timestamp,
};
use crate::types::account::{Account, AccountType};
use crate::storage::{Storage, StateManager};
use super::{
    Vault, SupplyPhase, TransferResult, FeeDistribution, BurnReason,
    TOTAL_SUPPLY, GENESIS_BALANCE, GENESIS_VAULT_ADDRESS, MIN_BALANCE,
};


// ===================================================================
// VAULT MANAGER
// ===================================================================

pub struct VaultManager {
    storage: Arc<dyn Storage>,
    state_manager: Arc<StateManager>,
    
    // Cache
    total_supply: Arc<RwLock<u128>>,
    genesis_balance: Arc<RwLock<u128>>,
    
    // İstatistikler
    total_burned: Arc<RwLock<u128>>,
    total_fees_collected: Arc<RwLock<u128>>,
    total_transfers: Arc<RwLock<u64>>,
}

impl VaultManager {
    pub fn new(storage: Arc<dyn Storage>, state_manager: Arc<StateManager>) -> Self {
        Self {
            storage,
            state_manager,
            total_supply: Arc::new(RwLock::new(TOTAL_SUPPLY)),
            genesis_balance: Arc::new(RwLock::new(GENESIS_BALANCE)),
            total_burned: Arc::new(RwLock::new(0)),
            total_fees_collected: Arc::new(RwLock::new(0)),
            total_transfers: Arc::new(RwLock::new(0)),
        }
    }
    
    // ===================================================================
    // GENESIS INITIALIZATION (GÜNCELLENDİ)
    // ===================================================================
    pub async fn initialize_genesis(&self) -> PacyteResult<()> {
        // 1. Protokol Hazinesi (Topluluk Kasası)
        let genesis_account = Account::genesis_sovereign(GENESIS_BALANCE);
        self.state_manager.set_account(GENESIS_VAULT_ADDRESS, genesis_account).await?;
        
        // 2. KURUCU KİLİT HESABI (YENİ EKLENDİ)
        // Bu hesap, 55 Milyon PNX'i tutacak. Bu bir akıllı kontrat adresi olacak.
        let mut founder_vesting = Account::new(FOUNDER_VESTING_ADDRESS, FOUNDER_ALLOCATION);
        founder_vesting.account_type = AccountType::System;
        self.state_manager.set_account(FOUNDER_VESTING_ADDRESS, founder_vesting).await?;
        
        tracing::info!("✅ Genesis Vault created with {} PAC", GENESIS_BALANCE / 1_000_000);
        tracing::info!("🔒 Founder Vesting created with {} PAC (Locked)", FOUNDER_ALLOCATION / 1_000_000);
        Ok(())
    }
    
    /// Bakiye transferi (iç fonksiyon)
    async fn execute_transfer(
        &self,
        from: &Address,
        to: &Address,
        amount: u128,
        fee: u128,
    ) -> PacyteResult<TransferResult> {
        let total_deduct = amount.saturating_add(fee);
        
        // Gönderici hesabını al
        let mut from_account = self.state_manager.get_account(from).await?
            .ok_or_else(|| PacyteError::AccountNotFound(format!("{:?}", from)))?;
        
        // Dormancy kontrolü
        if from_account.is_dormant {
            return Err(PacyteError::DormantAccount(6));
        }
        
        // Bakiye kontrolü
        if from_account.balance < total_deduct {
            return Err(PacyteError::InsufficientBalance {
                required: total_deduct,
                available: from_account.balance,
            });
        }
        
        // Nonce kontrolü ve artırımı
        from_account.increment_nonce();
        
        // Bakiyeyi düş
        from_account.balance -= total_deduct;
        from_account.record_activity();
        
        // Alıcı hesabını al veya oluştur
        let mut to_account = self.state_manager.get_account(to).await?
            .unwrap_or_else(|| Account::new(*to, 0));
        
        // Bakiyeyi ekle
        to_account.balance = to_account.balance.saturating_add(amount);
        to_account.record_activity();
        
        // Fee dağıtımını hesapla
        let phase = self.current_phase();
        let burn_rate = phase.burn_rate(*self.total_supply.read());
        let genesis_rate = phase.genesis_rate(*self.total_supply.read());
        
        let burned = (fee as f64 * burn_rate) as u128;
        let to_genesis = (fee as f64 * genesis_rate) as u128;
        let to_validators = fee.saturating_sub(burned).saturating_sub(to_genesis);
        
        // Burn işlemi
        if burned > 0 {
            *self.total_supply.write() = self.total_supply.read().saturating_sub(burned);
            *self.total_burned.write() += burned;
        }
        
        // Genesis'e fee aktar
        if to_genesis > 0 {
            let mut genesis = self.state_manager.get_account(&GENESIS_VAULT_ADDRESS).await?
                .unwrap_or_else(|| Account::genesis_sovereign(0));
            genesis.balance = genesis.balance.saturating_add(to_genesis);
            self.state_manager.set_account(GENESIS_VAULT_ADDRESS, genesis).await?;
        }
        
        // Hesapları kaydet
        self.state_manager.set_account(*from, from_account).await?;
        self.state_manager.set_account(*to, to_account).await?;
        
        // İstatistikleri güncelle
        *self.total_fees_collected.write() += fee;
        *self.total_transfers.write() += 1;
        
        Ok(TransferResult {
            success: true,
            from_balance_after: self.state_manager.get_balance(from).await?,
            to_balance_after: self.state_manager.get_balance(to).await?,
            fee_burned: burned,
            fee_to_validator: to_validators,
            fee_to_genesis: to_genesis,
        })
    }
}

#[async_trait::async_trait]
impl Vault for VaultManager {
    async fn transfer(
        &self,
        from: &Address,
        to: &Address,
        amount: u128,
        fee: u128,
    ) -> PacyteResult<TransferResult> {
        // Minimum bakiye kontrolü
        if amount < MIN_BALANCE {
            return Err(PacyteError::AmountTooSmall(amount, MIN_BALANCE));
        }
        
        // Kendine transfer kontrolü
        if from == to {
            return Err(PacyteError::SelfTransfer);
        }
        
        self.execute_transfer(from, to, amount, fee).await
    }
    
    async fn get_balance(&self, address: &Address) -> PacyteResult<u128> {
        self.state_manager.get_balance(address).await
    }
    
    async fn get_account(&self, address: &Address) -> PacyteResult<Option<Account>> {
        self.state_manager.get_account(address).await
    }
    
    async fn total_supply(&self) -> PacyteResult<u128> {
        Ok(*self.total_supply.read())
    }
    
    fn current_phase(&self) -> SupplyPhase {
        SupplyPhase::from_supply(*self.total_supply.read())
    }
    
    async fn distribute_fees(&self, total_fee: u128, block_height: BlockHeight) -> PacyteResult<FeeDistribution> {
        let phase = self.current_phase();
        let burn_rate = phase.burn_rate(*self.total_supply.read());
        let node_rate = phase.node_rate(*self.total_supply.read());
        let genesis_rate = phase.genesis_rate(*self.total_supply.read());
        
        let burned = (total_fee as f64 * burn_rate) as u128;
        let to_validators = (total_fee as f64 * node_rate) as u128;
        let to_genesis = total_fee.saturating_sub(burned).saturating_sub(to_validators);
        
        // Burn
        if burned > 0 {
            self.burn(burned, BurnReason::TransactionFee).await?;
        }
        
        // Genesis'e aktar
        if to_genesis > 0 {
            let mut genesis = self.state_manager.get_account(&GENESIS_VAULT_ADDRESS).await?
                .unwrap_or_else(|| Account::genesis_sovereign(0));
            genesis.balance = genesis.balance.saturating_add(to_genesis);
            self.state_manager.set_account(GENESIS_VAULT_ADDRESS, genesis).await?;
        }

// Token kontratı senkronizasyonu için log
tracing::debug!(
    "Fee distribution: total={}, burned={}, to_validators={}, to_genesis={}, phase={:?}",
    total_fee, burned, to_validators, to_genesis, phase
);
        
        Ok(FeeDistribution {
            total_fee,
            burned,
            to_validators,
            to_genesis,
            phase,
        })
    }
    
    async fn burn(&self, amount: u128, reason: BurnReason) -> PacyteResult<()> {
        if amount == 0 {
            return Ok(());
        }
        
        let mut supply = self.total_supply.write();
        if *supply < amount {
            return Err(PacyteError::InsufficientSupply);
        }
        
        *supply -= amount;
        *self.total_burned.write() += amount;
        
        tracing::info!("🔥 Burned {} PAC | Reason: {} | Remaining: {}", amount, reason, *supply);
        
        Ok(())
    }
    
    async fn process_dormant_accounts(&self, current_time: Timestamp) -> PacyteResult<Vec<Address>> {
        let dormant: Vec<Address> = Vec::new(); // TODO: StateManager'a check_all_dormancy metodu
        let mut processed = Vec::new();
        
        for address in &dormant {
            let balance = self.state_manager.get_balance(address).await?;
            if balance > 0 {
                // Dormant hesabı yak
                self.burn(balance, BurnReason::DormantAccount).await?;
                
                // Hesabı sil
                //self.state_manager.delete_account(address).await?;
                
                processed.push(*address);
                
                tracing::info!(
                    "💤 Dormant account {} burned: {} PAC",
                    crate::types::address_short(address),
                    balance
                );
            }
        }
        
        Ok(processed)
    }
    
    async fn initiate_bridge(&self, tx: super::BridgeTransaction) -> PacyteResult<u64> {
        // Bridge modülünde implemente edilecek
        Ok(0)
    }
    
    async fn finalize_bridge(&self, bridge_id: u64) -> PacyteResult<()> {
        Ok(())
    }
    
    async fn revert_bridge(&self, bridge_id: u64) -> PacyteResult<()> {
        Ok(())
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryStorage, StateManager};

    async fn setup_vault() -> VaultManager {
        let storage = Arc::new(MemoryStorage::new());
        let state_manager = Arc::new(StateManager::new(storage.clone()));
        let vault = VaultManager::new(storage, state_manager.clone());
        vault.initialize_genesis().await.unwrap();
        vault
    }

    #[tokio::test]
    async fn test_transfer() {
        let vault = setup_vault().await;
        
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        
        // Genesis'ten Alice'e transfer
        let result = vault.transfer(&GENESIS_VAULT_ADDRESS, &alice, 1000000, 1000).await.unwrap();
        assert!(result.success);
        
        let alice_balance = vault.get_balance(&alice).await.unwrap();
        assert_eq!(alice_balance, 1000000);
        
        // Alice'ten Bob'a transfer
        let result = vault.transfer(&alice, &bob, 500000, 500).await.unwrap();
        assert!(result.success);
        
        let bob_balance = vault.get_balance(&bob).await.unwrap();
        assert_eq!(bob_balance, 500000);
    }
    
    #[tokio::test]
    async fn test_insufficient_balance() {
        let vault = setup_vault().await;
        
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        
        let result = vault.transfer(&alice, &bob, 1000000, 1000).await;
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_self_transfer() {
        let vault = setup_vault().await;
        
        let alice = [1u8; 32];
        
        let result = vault.transfer(&alice, &alice, 1000, 10).await;
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_supply_phase() {
        let vault = setup_vault().await;
        
        assert_eq!(vault.current_phase(), SupplyPhase::GreatBurn);
        
        // Supply'ı değiştir
        *vault.total_supply.write() = 350_000_000_000_000;
        assert_eq!(vault.current_phase(), SupplyPhase::Transition);
        
        *vault.total_supply.write() = 250_000_000_000_000;
        assert_eq!(vault.current_phase(), SupplyPhase::GoldenEra);
    }
}

// ===================================================================
// TOKEN KONTRATI ENTEGRASYONU (Testnet için hazır)
// ===================================================================

//use crate::api::rpc::RpcClient;

impl VaultManager {
    /// Fee dağıtımı sonrası token kontratına bildirim gönder
    pub async fn sync_token_balance(
        &self,
        address: &Address,
        new_balance: u128,
    ) -> PacyteResult<()> {
        // Testnette RPC istemcisi ile kontrata bağlanacak
        // Şimdilik sadece log'a yaz
        tracing::debug!(
            "Token sync: address={:?}, new_balance={}",
            address, new_balance
        );
        Ok(())
    }
    
    /// Titan ödülü dağıtımı için rezerv kontrolü
    pub async fn check_reserve_and_topup(
        &self,
        titan_address: &Address,
        amount: u128,
    ) -> PacyteResult<()> {
        let reserve = self.total_supply.read();
        let titan_rewards_pool = 250_000_000_000_000u128; // 250M PNX (6 decimal)
        
        if *reserve >= titan_rewards_pool && amount > 0 {
            tracing::info!(
                "Titan reward: address={:?}, amount={} (manual distributeTitanReward required)",
                titan_address, amount
            );
        } else {
            tracing::warn!(
                "Insufficient reserve for Titan reward: need={}, available={}",
                amount, *reserve
            );
        }
        Ok(())
    }
}