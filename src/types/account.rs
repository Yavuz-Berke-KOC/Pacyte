use serde::{Deserialize, Serialize};
use super::{Address, Hash, Timestamp, current_timestamp};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub address: Address,
    pub balance: u128,
    pub nonce: u64,
    pub staked: u128,
    pub created_at: Timestamp,
    pub last_activity: Timestamp,
    pub is_dormant: bool,
    pub is_validator: bool,
    pub validator_key: Option<Vec<u8>>,
    pub account_type: AccountType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccountType {
    User,
    Sovereign,
    System,
    Bridge,
}

impl Account {
    pub fn new(address: Address, initial_balance: u128) -> Self {
        let now = current_timestamp();
        Self {
            address,
            balance: initial_balance,
            nonce: 0,
            staked: 0,
            created_at: now,
            last_activity: now,
            is_dormant: false,
            is_validator: false,
            validator_key: None,
            account_type: AccountType::User,
        }
    }
    pub fn genesis_sovereign(balance: u128) -> Self {
        let now = current_timestamp();
        Self {
            address: [0u8; 32],
            balance,
            nonce: 0,
            staked: 0,
            created_at: now,
            last_activity: now,
            is_dormant: false,
            is_validator: true,
            validator_key: None,
            account_type: AccountType::Sovereign,
        }
    }
    pub fn check_dormancy(&mut self, current_time: Timestamp) -> bool {
        const SIX_YEARS_SECS: u64 = 6 * 365 * 24 * 60 * 60;
        if !self.is_dormant && current_time - self.last_activity > SIX_YEARS_SECS {
            self.is_dormant = true;
            true
        } else { false }
    }
    pub fn record_activity(&mut self) { self.last_activity = current_timestamp(); self.is_dormant = false; }
    pub fn increment_nonce(&mut self) -> u64 { self.nonce += 1; self.record_activity(); self.nonce }
    pub fn credit(&mut self, amount: u128) { self.balance = self.balance.saturating_add(amount); self.record_activity(); }
    pub fn debit(&mut self, amount: u128) -> bool {
        if self.balance >= amount { self.balance -= amount; self.record_activity(); true } else { false }
    }
    pub fn stake(&mut self, amount: u128) -> bool {
        if self.balance >= amount { self.balance -= amount; self.staked += amount; self.record_activity(); true } else { false }
    }
    pub fn unstake(&mut self, amount: u128) -> bool {
        if self.staked >= amount { self.staked -= amount; self.balance += amount; self.record_activity(); true } else { false }
    }
    pub fn register_validator(&mut self, validator_key: Vec<u8>) {
        self.is_validator = true;
        self.validator_key = Some(validator_key);
        self.record_activity();
    }
    pub fn unregister_validator(&mut self) {
        self.is_validator = false;
        self.validator_key = None;
        self.record_activity();
    }
    pub fn total_wealth(&self) -> u128 { self.balance.saturating_add(self.staked) }
    pub fn is_empty(&self) -> bool { self.balance == 0 && self.staked == 0 }
}
