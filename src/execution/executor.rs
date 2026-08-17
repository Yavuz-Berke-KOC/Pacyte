// ===================================================================
// PACYTE NEXUS - İŞLEM YÜRÜTÜCÜ
// ===================================================================

use crate::vault::Vault;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

use crate::types::{
    PacyteError, PacyteResult, Address, Hash, BlockHeight, Timestamp,
};
use crate::types::transaction::Transaction;
use crate::types::account::Account;
use crate::storage::StateManager;
use crate::vault::VaultManager;

use super::{
    ExecutionContext, ExecutionResult, ExecutionError, Executor,
    VM, GasCalculator, GasMeter, Log, StateChange, Event,
    ContractStorage, CodeStorage,
};

// ===================================================================
// TRANSACTION EXECUTOR
// ===================================================================

pub struct TransactionExecutor {
    state_manager: Arc<StateManager>,
    vault_manager: Arc<VaultManager>,
    gas_calculator: GasCalculator,
    
    // Contract storage
    contract_storage: Arc<dyn ContractStorage>,
    code_storage: Arc<dyn CodeStorage>,
    
    // Cache
    account_cache: Arc<RwLock<HashMap<Address, Account>>>,
}

impl TransactionExecutor {
    pub fn new(
        state_manager: Arc<StateManager>,
        vault_manager: Arc<VaultManager>,
        contract_storage: Arc<dyn ContractStorage>,
        code_storage: Arc<dyn CodeStorage>,
    ) -> Self {
        Self {
            state_manager,
            vault_manager,
            gas_calculator: GasCalculator::default(),
            contract_storage,
            code_storage,
            account_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// İşlemi doğrula (ön kontroller)
    async fn validate_transaction(
        &self,
        tx: &Transaction,
        context: &mut ExecutionContext,
    ) -> PacyteResult<()> {
        // Gönderici hesabını al
        let from_account = self.get_account(&tx.from).await?
            .ok_or_else(|| ExecutionError::ContractNotFound(tx.from))?;
        
        // Nonce kontrolü
        if tx.nonce < from_account.nonce {
            return Err(ExecutionError::NonceTooLow {
                expected: from_account.nonce,
                actual: tx.nonce,
            }.into());
        }
        
        // Bakiye kontrolü
        let total_cost = tx.amount.saturating_add(tx.fee);
        if from_account.balance < total_cost {
            return Err(ExecutionError::InsufficientBalance {
                required: total_cost,
                available: from_account.balance,
            }.into());
        }
        
        // Gas limit kontrolü
        let intrinsic_gas = self.gas_calculator.intrinsic_gas(
            tx,
            self.code_storage.code_exists(&tx.to)
        );
        
        if tx.fee as u64 / context.gas_limit < 1 {
            // Gas price çok düşük
        }
        
        context.gas_limit = context.gas_limit.max(intrinsic_gas);
        
        Ok(())
    }
    
    /// Transfer işlemini çalıştır
    async fn execute_transfer(
        &self,
        tx: &Transaction,
        context: &mut ExecutionContext,
    ) -> PacyteResult<ExecutionResult> {
        // Gas kullan
        let intrinsic_gas = self.gas_calculator.intrinsic_gas(tx, false);
        if !context.use_gas(intrinsic_gas) {
            return Ok(ExecutionResult::failure(
                ExecutionError::OutOfGas {
                    used: context.gas_used,
                    limit: context.gas_limit,
                },
                context.gas_used,
            ));
        }
        
        // Transferi gerçekleştir
        let transfer_result = self.vault_manager.transfer(
            &tx.from,
            &tx.to,
            tx.amount,
            tx.fee,
        ).await?;
        
        // Nonce'i güncelle
        let mut from_account = self.get_account(&tx.from).await?.unwrap();
        from_account.nonce += 1;
        self.set_account(tx.from, from_account).await?;
        
        Ok(ExecutionResult::success(
            context.gas_used,
            Vec::new(),
        ))
    }
    
    /// Contract çağrısını çalıştır
    async fn execute_contract_call(
        &self,
        tx: &Transaction,
        context: &mut ExecutionContext,
    ) -> PacyteResult<ExecutionResult> {
        // Contract kodunu al
        let code = self.code_storage.get_code(&tx.to)
            .ok_or_else(|| ExecutionError::ContractNotFound(tx.to))?;
        
        // Intrinsic gas
        let intrinsic_gas = self.gas_calculator.intrinsic_gas(tx, false);
        if !context.use_gas(intrinsic_gas) {
            return Ok(ExecutionResult::failure(
                ExecutionError::OutOfGas {
                    used: context.gas_used,
                    limit: context.gas_limit,
                },
                context.gas_used,
            ));
        }
        
        // Transfer varsa önce onu yap
        if tx.amount > 0 {
            self.vault_manager.transfer(
                &tx.from,
                &tx.to,
                tx.amount,
                0, // Fee ayrıca hesaplanır
            ).await?;
        }
        
        // VM'i çalıştır
        let mut vm = VM::new(code, context.remaining_gas());
        
        let vm_result = vm.run();
        
        let gas_used = vm.gas_used();
        context.use_gas(gas_used);
        
        match vm_result {
            Ok(return_data) => {
                // Nonce güncelle
                let mut from_account = self.get_account(&tx.from).await?.unwrap();
                from_account.nonce += 1;
                self.set_account(tx.from, from_account).await?;
                
                Ok(ExecutionResult::success(
                    context.gas_used,
                    return_data,
                ))
            }
            Err(e) => {
                Ok(ExecutionResult::failure(e, context.gas_used))
            }
        }
    }
    
    /// Contract deploy et
    async fn execute_contract_creation(
        &self,
        tx: &Transaction,
        context: &mut ExecutionContext,
    ) -> PacyteResult<ExecutionResult> {
        // Intrinsic gas
        let intrinsic_gas = self.gas_calculator.intrinsic_gas(tx, true);
        if !context.use_gas(intrinsic_gas) {
            return Ok(ExecutionResult::failure(
                ExecutionError::OutOfGas {
                    used: context.gas_used,
                    limit: context.gas_limit,
                },
                context.gas_used,
            ));
        }
        
        // Contract adresi oluştur (sender + nonce)
        let contract_address = self.generate_contract_address(&tx.from, tx.nonce);
        
        // Init code'u çalıştır
        let init_code = &tx.signature; // Basitleştirilmiş - gerçekte input data
        
        let mut vm = VM::new(init_code.to_vec(), context.remaining_gas());
        
        let vm_result = vm.run();
        let gas_used = vm.gas_used();
        context.use_gas(gas_used);
        
        match vm_result {
            Ok(runtime_code) => {
                // Contract kodunu kaydet
                self.code_storage.set_code(&contract_address, &runtime_code);
                
                // Gönderici nonce'ini güncelle
                let mut from_account = self.get_account(&tx.from).await?.unwrap();
                from_account.nonce += 1;
                from_account.balance = from_account.balance
                    .saturating_sub(tx.amount)
                    .saturating_sub(tx.fee);
                self.set_account(tx.from, from_account).await?;
                
                // Contract hesabını oluştur
                let contract_account = Account {
                    address: contract_address,
                    balance: tx.amount,
                    nonce: 1,
                    staked: 0,
                    created_at: context.block_timestamp,
                    last_activity: context.block_timestamp,
                    is_dormant: false,
                    is_validator: false,
                    validator_key: None,
                    account_type: crate::types::account::AccountType::User,
                };
                self.set_account(contract_address, contract_account).await?;
                
                Ok(ExecutionResult::success(
                    context.gas_used,
                    contract_address.to_vec(),
                ))
            }
            Err(e) => {
                Ok(ExecutionResult::failure(e, context.gas_used))
            }
        }
    }
    
    /// Contract adresi oluştur (CREATE)
    fn generate_contract_address(&self, sender: &Address, nonce: u64) -> Address {
        use sha3::{Digest, Keccak256};
        
        let mut hasher = Keccak256::new();
        hasher.update(sender);
        hasher.update(&nonce.to_be_bytes());
        
        let hash: [u8; 32] = hasher.finalize().into();
        
        let mut address = [0u8; 32];
        address[12..].copy_from_slice(&hash[12..]);
        address
    }
    
    /// Contract adresi oluştur (CREATE2)
    fn generate_contract_address2(
        &self,
        sender: &Address,
        salt: &[u8; 32],
        init_code_hash: &Hash,
    ) -> Address {
        use sha3::{Digest, Keccak256};
        
        let mut hasher = Keccak256::new();
        hasher.update(&[0xff]);
        hasher.update(sender);
        hasher.update(salt);
        hasher.update(init_code_hash);
        
        let hash: [u8; 32] = hasher.finalize().into();
        
        let mut address = [0u8; 32];
        address[12..].copy_from_slice(&hash[12..]);
        address
    }
    
    /// Hesap getir (cache'li)
    async fn get_account(&self, address: &Address) -> PacyteResult<Option<Account>> {
        // Cache kontrol
        {
            let cache = self.account_cache.read();
            if let Some(account) = cache.get(address) {
                return Ok(Some(account.clone()));
            }
        }
        
        // State'den al
        let account = self.state_manager.get_account(address).await?;
        
        // Cache'e ekle
        if let Some(ref acc) = account {
            let mut cache = self.account_cache.write();
            cache.insert(*address, acc.clone());
        }
        
        Ok(account)
    }
    
    /// Hesap kaydet (cache'li)
    async fn set_account(&self, address: Address, account: Account) -> PacyteResult<()> {
        // Cache'e ekle
        {
            let mut cache = self.account_cache.write();
            cache.insert(address, account.clone());
        }
        
        // State'e kaydet
        self.state_manager.set_account(address, account).await
    }
}

#[async_trait::async_trait]
impl Executor for TransactionExecutor {
    async fn execute_transaction(
        &self,
        tx: &Transaction,
        context: &mut ExecutionContext,
    ) -> PacyteResult<ExecutionResult> {
        // Validasyon
        self.validate_transaction(tx, context).await?;
        
        // İşlem tipine göre yürüt
        let result = if self.code_storage.code_exists(&tx.to) {
            // Contract çağrısı
            self.execute_contract_call(tx, context).await?
        } else if tx.to == [0u8; 32] {
            // Contract deploy
            self.execute_contract_creation(tx, context).await?
        } else {
            // Basit transfer
            self.execute_transfer(tx, context).await?
        };
        
        Ok(result)
    }
    
    async fn call_contract(
        &self,
        contract: &Address,
        input: &[u8],
        caller: &Address,
        gas_limit: u64,
    ) -> PacyteResult<ExecutionResult> {
        let code = self.code_storage.get_code(contract)
            .ok_or_else(|| ExecutionError::ContractNotFound(*contract))?;
        
        let mut vm = VM::new(code, gas_limit);
        
        match vm.run() {
            Ok(return_data) => {
                Ok(ExecutionResult::success(vm.gas_used(), return_data))
            }
            Err(e) => {
                Ok(ExecutionResult::failure(e, vm.gas_used()))
            }
        }
    }
    
    async fn deploy_contract(
        &self,
        code: &[u8],
        deployer: &Address,
        gas_limit: u64,
        context: &mut ExecutionContext,
    ) -> PacyteResult<(Address, ExecutionResult)> {
        let mut vm = VM::new(code.to_vec(), gas_limit);
        
        let vm_result = vm.run();
        let gas_used = vm.gas_used();
        
        match vm_result {
            Ok(runtime_code) => {
                let address = self.generate_contract_address(deployer, context.tx_index as u64);
                
                self.code_storage.set_code(&address, &runtime_code);
                
                Ok((address, ExecutionResult::success(gas_used, address.to_vec())))
            }
            Err(e) => {
                Ok(([0u8; 32], ExecutionResult::failure(e, gas_used)))
            }
        }
    }
    
    fn estimate_gas(&self, tx: &Transaction) -> u64 {
        self.gas_calculator.estimate_gas(
            tx,
            tx.to == [0u8; 32] || self.code_storage.code_exists(&tx.to)
        )
    }
}

// ===================================================================
// BLOCK EXECUTOR
// ===================================================================

pub struct BlockExecutor {
    tx_executor: Arc<TransactionExecutor>,
}

impl BlockExecutor {
    pub fn new(tx_executor: Arc<TransactionExecutor>) -> Self {
        Self { tx_executor }
    }
    
    /// Bloktaki tüm işlemleri çalıştır
    pub async fn execute_block(
        &self,
        transactions: &[Transaction],
        block_height: BlockHeight,
        block_timestamp: Timestamp,
        block_hash: Hash,
    ) -> PacyteResult<BlockExecutionResult> {
        let mut results = Vec::new();
        let mut total_gas_used = 0u64;
        let mut total_fees = 0u128;
        let mut all_logs = Vec::new();
        let mut all_events = Vec::new();
        
        for (idx, tx) in transactions.iter().enumerate() {
            let mut context = ExecutionContext::new(
                block_height,
                block_timestamp,
                tx.from,
                10_000_000, // Block gas limit
            );
            context.block_hash = block_hash;
            context.tx_index = idx;
            
            let result = self.tx_executor.execute_transaction(tx, &mut context).await?;
            
            total_gas_used += result.gas_used;
            total_fees += tx.fee;
            
            all_logs.extend(result.logs.clone());
            all_events.extend(result.events.clone());
            
	    results.push(result.clone());

            
            if !result.success {
                // Başarısız işlem - bloğu durdur
                break;
            }
        }
        
        Ok(BlockExecutionResult {
            block_height,
            block_hash,
            transaction_count: results.len(),
            total_gas_used,
            total_fees,
            results,
            logs: all_logs,
            events: all_events,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BlockExecutionResult {
    pub block_height: BlockHeight,
    pub block_hash: Hash,
    pub transaction_count: usize,
    pub total_gas_used: u64,
    pub total_fees: u128,
    pub results: Vec<ExecutionResult>,
    pub logs: Vec<Log>,
    pub events: Vec<Event>,
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_contract_address() {
        let executor = TransactionExecutor::new(
            Arc::new(crate::storage::StateManager::new(Arc::new(crate::storage::MemoryStorage::new()))),
            Arc::new(crate::vault::VaultManager::new(
                Arc::new(crate::storage::MemoryStorage::new()),
                Arc::new(crate::storage::StateManager::new(Arc::new(crate::storage::MemoryStorage::new()))),
            )),
            Arc::new(MockContractStorage),
            Arc::new(MockCodeStorage),
        );
        
        let sender = [1u8; 32];
        let address = executor.generate_contract_address(&sender, 0);
        
        assert_ne!(address, [0u8; 32]);
        assert_eq!(address[0..12], [0u8; 12]);
    }
}

// Mock storage'lar
struct MockContractStorage;
impl ContractStorage for MockContractStorage {
    fn get(&self, _: &Address, _: &[u8]) -> Option<Vec<u8>> { None }
    fn set(&self, _: &Address, _: &[u8], _: &[u8]) {}
    fn delete(&self, _: &Address, _: &[u8]) {}
    fn has(&self, _: &Address, _: &[u8]) -> bool { false }
}

struct MockCodeStorage;
impl CodeStorage for MockCodeStorage {
    fn get_code(&self, _: &Address) -> Option<Vec<u8>> { None }
    fn set_code(&self, _: &Address, _: &[u8]) {}
    fn get_code_hash(&self, _: &Address) -> Option<Hash> { None }
    fn code_exists(&self, _: &Address) -> bool { false }
}