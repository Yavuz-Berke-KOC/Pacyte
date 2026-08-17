// ===================================================================
// PACYTE NEXUS - DORMANCY YÖNETİMİ (6 YIL KURALI)
// ===================================================================

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::types::{
    PacyteError, PacyteResult, Address, BlockHeight, Timestamp, current_timestamp,
};
//use crate::types::account::{Account, AccountActivity};

// ===================================================================
// DORMANCY SABİTLERİ
// ===================================================================

const DORMANCY_THRESHOLD_YEARS: u32 = 6;
const DORMANCY_THRESHOLD_SECONDS: u64 = DORMANCY_THRESHOLD_YEARS as u64 * 365 * 24 * 60 * 60;
const GRACE_PERIOD_SECONDS: u64 = 30 * 24 * 60 * 60; // 30 gün

// ===================================================================
// DORMANCY MANAGER
// ===================================================================

pub struct DormancyManager {
    // Adres -> son aktivite zamanı
    last_activity: Arc<DashMap<Address, Timestamp>>,
    
    // Adres -> uyarı gönderilme zamanı
    warnings_sent: Arc<RwLock<HashMap<Address, Timestamp>>>,
    
    // Dormant olarak işaretlenen hesaplar
    dormant_accounts: Arc<RwLock<HashMap<Address, DormantRecord>>>,
    
    // İstatistikler
    total_dormant_accounts: Arc<RwLock<usize>>,
    total_dormant_value: Arc<RwLock<u128>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DormantRecord {
    pub address: Address,
    pub balance: u128,
    pub last_activity: Timestamp,
    pub dormant_since: Timestamp,
    pub burned: bool,
    pub burned_at: Option<Timestamp>,
}

impl DormancyManager {
    pub fn new() -> Self {
        Self {
            last_activity: Arc::new(DashMap::new()),
            warnings_sent: Arc::new(RwLock::new(HashMap::new())),
            dormant_accounts: Arc::new(RwLock::new(HashMap::new())),
            total_dormant_accounts: Arc::new(RwLock::new(0)),
            total_dormant_value: Arc::new(RwLock::new(0)),
        }
    }
    
    /// Aktivite kaydet
    pub fn record_activity(&self, address: &Address) {
        let now = current_timestamp();
        self.last_activity.insert(*address, now);
        
        // Eğer dormant olarak işaretlenmişse, yeniden aktif et
        let mut dormant = self.dormant_accounts.write();
        if dormant.remove(address).is_some() {
            *self.total_dormant_accounts.write() -= 1;
            tracing::info!("Account {} reactivated", crate::types::address_short(address));
        }
    }
    
    /// Hesap ekle (yeni hesap oluşturulduğunda)
    pub fn add_account(&self, address: &Address, balance: u128) {
        self.last_activity.insert(*address, current_timestamp());
    }
    
    /// Hesap sil (yakıldığında)
    pub fn remove_account(&self, address: &Address) {
        self.last_activity.remove(address);
        
        let mut dormant = self.dormant_accounts.write();
        if let Some(record) = dormant.remove(address) {
            *self.total_dormant_accounts.write() -= 1;
            *self.total_dormant_value.write() -= record.balance;
        }
    }
    
    /// Dormant hesapları kontrol et
    pub fn check_dormancy(&self, current_time: Timestamp) -> Vec<Address> {
        let mut newly_dormant = Vec::new();
        let threshold = current_time.saturating_sub(DORMANCY_THRESHOLD_SECONDS);
        
        for entry in self.last_activity.iter() {
            let address = *entry.key();
            let last_active = *entry.value();
            
            // Zaten dormant olarak işaretlenmiş mi?
            if self.dormant_accounts.read().contains_key(&address) {
                continue;
            }
            
            // Aktivite eşiğini geçmiş mi?
            if last_active < threshold {
                newly_dormant.push(address);
            }
        }
        
        newly_dormant
    }
    
    /// Hesabı dormant olarak işaretle
    pub fn mark_dormant(&self, address: &Address, balance: u128) {
        let now = current_timestamp();
        let last_active = self.last_activity.get(address)
            .map(|v| *v)
            .unwrap_or(now);
        
        let record = DormantRecord {
            address: *address,
            balance,
            last_activity: last_active,
            dormant_since: now,
            burned: false,
            burned_at: None,
        };
        
        let mut dormant = self.dormant_accounts.write();
        if dormant.insert(*address, record).is_none() {
            *self.total_dormant_accounts.write() += 1;
            *self.total_dormant_value.write() += balance;
        }
        
        tracing::warn!(
            "💤 Account {} marked dormant (inactive for {} years)",
            crate::types::address_short(address),
            DORMANCY_THRESHOLD_YEARS
        );
    }
    
    /// Dormant hesabı yak (6 yıl + grace period)
    pub fn burn_dormant(&self, address: &Address) -> Option<u128> {
        let mut dormant = self.dormant_accounts.write();
        
        if let Some(record) = dormant.get_mut(address) {
            if !record.burned {
                let now = current_timestamp();
                let grace_end = record.dormant_since + DORMANCY_THRESHOLD_SECONDS + GRACE_PERIOD_SECONDS;
                
                if now >= grace_end {
                    record.burned = true;
                    record.burned_at = Some(now);
                    
                    *self.total_dormant_value.write() -= record.balance;
                    
                    tracing::info!(
                        "🔥 Dormant account {} burned: {} PAC",
                        crate::types::address_short(address),
                        record.balance
                    );
                    
                    return Some(record.balance);
                }
            }
        }
        
        None
    }
    
    /// Uyarı gönderilmesi gereken hesapları bul
    pub fn get_accounts_needing_warning(&self, current_time: Timestamp) -> Vec<Address> {
        let mut needs_warning = Vec::new();
        let warning_threshold = current_time.saturating_sub(DORMANCY_THRESHOLD_SECONDS - GRACE_PERIOD_SECONDS);
        
        for entry in self.last_activity.iter() {
            let address = *entry.key();
            let last_active = *entry.value();
            
            // Zaten uyarı gönderilmiş mi?
            if let Some(last_warning) = self.warnings_sent.read().get(&address) {
                if current_time - last_warning < GRACE_PERIOD_SECONDS {
                    continue;
                }
            }
            
            // Zaten dormant mı?
            if self.dormant_accounts.read().contains_key(&address) {
                continue;
            }
            
            // Uyarı eşiğini geçmiş mi?
            if last_active < warning_threshold {
                needs_warning.push(address);
            }
        }
        
        needs_warning
    }
    
    /// Uyarı gönderildi olarak işaretle
    pub fn mark_warning_sent(&self, address: &Address) {
        self.warnings_sent.write().insert(*address, current_timestamp());
    }
    
    /// Hesabın dormant olup olmadığını kontrol et
    pub fn is_dormant(&self, address: &Address) -> bool {
        self.dormant_accounts.read().contains_key(address)
    }
    
    /// Hesabın ne kadar süredir inaktif olduğunu getir
    pub fn inactive_duration(&self, address: &Address) -> Option<u64> {
        self.last_activity.get(address)
            .map(|v| current_timestamp().saturating_sub(*v))
    }
    
    /// Dormant hesapların listesini getir
    pub fn list_dormant_accounts(&self) -> Vec<DormantRecord> {
        self.dormant_accounts.read().values().cloned().collect()
    }
    
    /// İstatistikleri getir
    pub fn stats(&self) -> DormancyStats {
        let dormant = self.dormant_accounts.read();
        
        let mut total_balance = 0u128;
        let mut burned_count = 0;
        let mut burned_value = 0u128;
        let mut oldest_dormant = 0u64;
        
        for record in dormant.values() {
            total_balance += record.balance;
            
            if record.burned {
                burned_count += 1;
                burned_value += record.balance;
            }
            
            let age = current_timestamp().saturating_sub(record.dormant_since);
            oldest_dormant = oldest_dormant.max(age);
        }
        
        DormancyStats {
            total_dormant_accounts: dormant.len(),
            total_dormant_balance: total_balance,
            burned_accounts: burned_count,
            burned_value,
            oldest_dormant_days: oldest_dormant / 86400,
            threshold_years: DORMANCY_THRESHOLD_YEARS,
        }
    }
}

impl Default for DormancyManager {
    fn default() -> Self {
        Self::new()
    }
}

use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct DormancyStats {
    pub total_dormant_accounts: usize,
    pub total_dormant_balance: u128,
    pub burned_accounts: usize,
    pub burned_value: u128,
    pub oldest_dormant_days: u64,
    pub threshold_years: u32,
}

impl std::fmt::Display for DormancyStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Dormancy: {} accounts, {} PAC dormant, {} burned ({} PAC), oldest {} days",
            self.total_dormant_accounts,
            self.total_dormant_balance,
            self.burned_accounts,
            self.burned_value,
            self.oldest_dormant_days
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
    fn test_record_activity() {
        let manager = DormancyManager::new();
        let addr = [1u8; 32];
        
        manager.record_activity(&addr);
        
        assert!(manager.last_activity.contains_key(&addr));
        assert!(!manager.is_dormant(&addr));
    }
    
    #[test]
    fn test_dormancy_check() {
        let manager = DormancyManager::new();
        let addr = [1u8; 32];
        
        // Eski aktivite kaydet
        let old_time = current_timestamp() - DORMANCY_THRESHOLD_SECONDS - 1000;
        manager.last_activity.insert(addr, old_time);
        
        let dormant = manager.check_dormancy(current_timestamp());
        assert!(dormant.contains(&addr));
    }
    
    #[test]
    fn test_mark_dormant() {
        let manager = DormancyManager::new();
        let addr = [1u8; 32];
        
        manager.mark_dormant(&addr, 1000000);
        
        assert!(manager.is_dormant(&addr));
        assert_eq!(manager.stats().total_dormant_accounts, 1);
        assert_eq!(manager.stats().total_dormant_balance, 1000000);
    }
    
    #[test]
    fn test_burn_dormant() {
        let manager = DormancyManager::new();
        let addr = [1u8; 32];
        
        manager.mark_dormant(&addr, 1000000);
        
        // Hemen yakılamaz (grace period)
        let burned = manager.burn_dormant(&addr);
        assert!(burned.is_none());
    }
}