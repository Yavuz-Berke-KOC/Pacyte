// ===================================================================
// PACYTE NEXUS - EXECUTION MODÜLÜ
// ===================================================================

pub mod vm;
pub mod gas;
pub mod executor;
pub mod contract;
pub mod wasm_runtime;
pub mod precompiles;

// Re-export'lar
pub use vm::*;
pub use gas::*;
pub use executor::*;
pub use contract::*;
pub use wasm_runtime::*;
pub use precompiles::*;

use crate::types::{
    PacyteError, PacyteResult, Address, Hash, BlockHeight, Timestamp,
};
use crate::types::transaction::Transaction;
use crate::types::account::Account;
use crate::storage::StateManager;
use std::collections::HashMap;
use std::sync::Arc;

// ===================================================================
// EXECUTION CONTEXT
// ===================================================================

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub block_height: BlockHeight,
    pub block_timestamp: Timestamp,
    pub block_hash: Hash,
    pub tx_index: usize,
    pub origin: Address,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub logs: Vec<Log>,
    pub state_changes: Vec<StateChange>,
    pub events: Vec<Event>,
}

impl ExecutionContext {
    pub fn new(
        block_height: BlockHeight,
        block_timestamp: Timestamp,
        origin: Address,
        gas_limit: u64,
    ) -> Self {
        Self {
            block_height,
            block_timestamp,
            block_hash: [0u8; 32],
            tx_index: 0,
            origin,
            gas_limit,
            gas_used: 0,
            logs: Vec::new(),
            state_changes: Vec::new(),
            events: Vec::new(),
        }
    }
    
    pub fn use_gas(&mut self, amount: u64) -> bool {
        if self.gas_used + amount > self.gas_limit {
            return false;
        }
        self.gas_used += amount;
        true
    }
    
    pub fn remaining_gas(&self) -> u64 {
        self.gas_limit.saturating_sub(self.gas_used)
    }
    
    pub fn add_log(&mut self, log: Log) {
        self.logs.push(log);
    }
    
    pub fn add_state_change(&mut self, change: StateChange) {
        self.state_changes.push(change);
    }
    
    pub fn emit_event(&mut self, event: Event) {
        self.events.push(event);
    }
}

// ===================================================================
// LOG
// ===================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<Hash>,
    pub data: Vec<u8>,
    pub block_height: BlockHeight,
    pub tx_index: usize,
    pub log_index: usize,
}

// ===================================================================
// STATE CHANGE
// ===================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StateChange {
    pub address: Address,
    pub key: Vec<u8>,
    pub old_value: Vec<u8>,
    pub new_value: Vec<u8>,
}

// ===================================================================
// EVENT
// ===================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub contract: Address,
    pub name: String,
    pub data: Vec<u8>,
    pub topics: Vec<Hash>,
}

// ===================================================================
// EXECUTION RESULT
// ===================================================================

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub gas_used: u64,
    pub gas_refund: u64,
    pub return_data: Vec<u8>,
    pub logs: Vec<Log>,
    pub state_changes: Vec<StateChange>,
    pub events: Vec<Event>,
    pub error: Option<ExecutionError>,
}

impl ExecutionResult {
    pub fn success(gas_used: u64, return_data: Vec<u8>) -> Self {
        Self {
            success: true,
            gas_used,
            gas_refund: 0,
            return_data,
            logs: Vec::new(),
            state_changes: Vec::new(),
            events: Vec::new(),
            error: None,
        }
    }
    
    pub fn failure(error: ExecutionError, gas_used: u64) -> Self {
        Self {
            success: false,
            gas_used,
            gas_refund: 0,
            return_data: Vec::new(),
            logs: Vec::new(),
            state_changes: Vec::new(),
            events: Vec::new(),
            error: Some(error),
        }
    }
    
    pub fn with_logs(mut self, logs: Vec<Log>) -> Self {
        self.logs = logs;
        self
    }
    
    pub fn with_events(mut self, events: Vec<Event>) -> Self {
        self.events = events;
        self
    }
    
    pub fn with_state_changes(mut self, changes: Vec<StateChange>) -> Self {
        self.state_changes = changes;
        self
    }
}

// ===================================================================
// EXECUTION ERROR
// ===================================================================

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecutionError {
    #[error("Out of gas: used {used}, limit {limit}")]
    OutOfGas { used: u64, limit: u64 },
    
    #[error("Contract reverted: {0}")]
    Revert(String),
    
    #[error("Invalid opcode: {0}")]
    InvalidOpcode(u8),
    
    #[error("Stack overflow")]
    StackOverflow,
    
    #[error("Stack underflow")]
    StackUnderflow,
    
    #[error("Invalid jump destination")]
    InvalidJump,
    
    #[error("Contract call depth exceeded")]
    CallDepthExceeded,
    
    #[error("Contract not found: {0:?}")]
    ContractNotFound(Address),
    
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Nonce too low: expected {expected}, got {actual}")]
    NonceTooLow { expected: u64, actual: u64 },
    
    #[error("Insufficient balance: required {required}, available {available}")]
    InsufficientBalance { required: u128, available: u128 },
    
    #[error("Execution error: {0}")]
    Other(String),
}

// ===================================================================
// EXECUTOR TRAIT
// ===================================================================

#[async_trait::async_trait]
pub trait Executor: Send + Sync {
    /// İşlemi çalıştır
    async fn execute_transaction(
        &self,
        tx: &Transaction,
        context: &mut ExecutionContext,
    ) -> PacyteResult<ExecutionResult>;
    
    /// Contract çağrısı yap (read-only)
    async fn call_contract(
        &self,
        contract: &Address,
        input: &[u8],
        caller: &Address,
        gas_limit: u64,
    ) -> PacyteResult<ExecutionResult>;
    
    /// Contract deploy et
    async fn deploy_contract(
        &self,
        code: &[u8],
        deployer: &Address,
        gas_limit: u64,
        context: &mut ExecutionContext,
    ) -> PacyteResult<(Address, ExecutionResult)>;
    
    /// Gas fiyatını hesapla
    fn estimate_gas(&self, tx: &Transaction) -> u64;
}

// ===================================================================
// CONTRACT STORAGE
// ===================================================================

pub trait ContractStorage: Send + Sync {
    fn get(&self, address: &Address, key: &[u8]) -> Option<Vec<u8>>;
    fn set(&self, address: &Address, key: &[u8], value: &[u8]);
    fn delete(&self, address: &Address, key: &[u8]);
    fn has(&self, address: &Address, key: &[u8]) -> bool;
}

// ===================================================================
// CONTRACT CODE STORAGE
// ===================================================================

pub trait CodeStorage: Send + Sync {
    fn get_code(&self, address: &Address) -> Option<Vec<u8>>;
    fn set_code(&self, address: &Address, code: &[u8]);
    fn get_code_hash(&self, address: &Address) -> Option<Hash>;
    fn code_exists(&self, address: &Address) -> bool;
}