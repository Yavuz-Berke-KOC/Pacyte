// ===================================================================
// PACYTE NEXUS - VALIDATOR YÖNETİMİ (GERÇEK AVX-512 KONTROLÜ)
// ===================================================================

use std::arch::x86_64::__cpuid_count;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::types::{
    PacyteError, PacyteResult, Address, BlockHeight, Timestamp, current_timestamp,
};
use crate::crypto::{HybridSigner, HybridPublicKey};
use super::{ValidatorSet, ValidatorInfo};

// ===================================================================
// VALIDATOR DURUMU
// ===================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidatorStatus {
    Active,
    Inactive,
    Jailed,
    Slashed,
    Unbonding,
}

impl std::fmt::Display for ValidatorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ===================================================================
// VALIDATOR KAYDI
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorRecord {
    pub id: u64,
    pub address: Address,
    pub public_key: Vec<u8>,
    pub stake: u128,
    pub voting_power: u64,
    pub status: ValidatorStatus,
    pub joined_at: Timestamp,
    pub last_active: Timestamp,
    pub total_blocks_proposed: u64,
    pub total_votes_cast: u64,
    pub missed_blocks: u64,
    pub slashed_amount: u128,
    pub commission_rate: u8, // 0-100
    pub delegators: HashMap<Address, u128>,
    pub hardware_verified: bool,  // AVX-512 kontrolü yapıldı mı?
    pub avx512_supported: bool,   // Gerçek sonuç
}

impl ValidatorRecord {
    pub fn new(id: u64, address: Address, public_key: Vec<u8>, stake: u128) -> Self {
        let now = current_timestamp();
        let (verified, supported) = check_hardware_features();
        
        Self {
            id,
            address,
            public_key,
            stake,
            voting_power: Self::calculate_voting_power(stake),
            status: ValidatorStatus::Active,
            joined_at: now,
            last_active: now,
            total_blocks_proposed: 0,
            total_votes_cast: 0,
            missed_blocks: 0,
            slashed_amount: 0,
            commission_rate: 10,
            delegators: HashMap::new(),
            hardware_verified: verified,
            avx512_supported: supported,
        }
    }
    
    pub fn calculate_voting_power(stake: u128) -> u64 {
        (stake / 1_000_000) as u64
    }
    
    pub fn update_voting_power(&mut self) {
        self.voting_power = Self::calculate_voting_power(self.stake);
    }
    
    pub fn add_delegation(&mut self, delegator: Address, amount: u128) -> PacyteResult<()> {
        let entry = self.delegators.entry(delegator).or_insert(0);
        *entry = entry.saturating_add(amount);
        self.stake = self.stake.saturating_add(amount);
        self.update_voting_power();
        Ok(())
    }
    
    pub fn remove_delegation(&mut self, delegator: Address, amount: u128) -> PacyteResult<u128> {
        let entry = self.delegators.get_mut(&delegator)
            .ok_or_else(|| PacyteError::AccountNotFound(format!("{:?}", delegator)))?;
        
        if *entry < amount {
            return Err(PacyteError::InsufficientBalance {
                required: amount,
                available: *entry,
            });
        }
        
        *entry -= amount;
        self.stake = self.stake.saturating_sub(amount);
        self.update_voting_power();
        
        Ok(amount)
    }
    
    pub fn record_block_proposed(&mut self) {
        self.total_blocks_proposed += 1;
        self.last_active = current_timestamp();
    }
    
    pub fn record_vote_cast(&mut self) {
        self.total_votes_cast += 1;
        self.last_active = current_timestamp();
    }
    
    pub fn record_missed_block(&mut self) {
        self.missed_blocks += 1;
        
        if self.missed_blocks >= 50 {
            self.status = ValidatorStatus::Jailed;
        }
    }
    
    pub fn slash(&mut self, amount: u128, reason: &str) -> PacyteResult<()> {
        if amount > self.stake {
            return Err(PacyteError::InsufficientBalance {
                required: amount,
                available: self.stake,
            });
        }
        
        self.stake -= amount;
        self.slashed_amount += amount;
        self.update_voting_power();
        
        tracing::warn!("Validator {} slashed {} PAC: {}", self.id, amount, reason);
        
        if self.stake < MIN_VALIDATOR_STAKE {
            self.status = ValidatorStatus::Inactive;
        }
        
        Ok(())
    }
    
    pub fn is_active(&self) -> bool {
        self.status == ValidatorStatus::Active
    }
}

// ===================================================================
// DONANIM KONTROLÜ (GERÇEK CPUID - SİMÜLASYON DEĞİL!)
// ===================================================================

/// CPU'nun AVX-512 ve diğer özelliklerini kontrol eder
/// Bu fonksiyon GERÇEK CPUID komutunu kullanır, simülasyon değildir!
pub fn check_hardware_features() -> (bool, bool) {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        
        unsafe {
            // CPUID Leaf 1: Genel özellikler
            let cpuid1 = __cpuid(1);
            let has_avx = (cpuid1.ecx & (1 << 28)) != 0;  // AVX
            let has_fma = (cpuid1.ecx & (1 << 12)) != 0;  // FMA
            
            // CPUID Leaf 7, Subleaf 0: Genişletilmiş özellikler
            let cpuid7 = __cpuid(7);
            
            // AVX-512 Foundation (EBX bit 16)
            let has_avx512f = (cpuid7.ebx & (1 << 16)) != 0;
            
            // AVX-512 Byte and Word (EBX bit 30)
            let has_avx512bw = (cpuid7.ebx & (1 << 30)) != 0;
            
            // AVX-512 Doubleword and Quadword (EBX bit 17)
            let has_avx512dq = (cpuid7.ebx & (1 << 17)) != 0;
            
            // AVX-512 Vector Length Extensions (EBX bit 26)
            let has_avx512vl = (cpuid7.ebx & (1 << 26)) != 0;
            
            // AVX-512 Integer Fused Multiply-Add (EBX bit 21)
            let has_avx512ifma = (cpuid7.ebx & (1 << 21)) != 0;
            
            // AVX-512 Vector Byte Manipulation Instructions (ECX bit 1)
            let has_avx512vbmi = (cpuid7.ecx & (1 << 1)) != 0;
            
            // AVX-512 Vector Neural Network Instructions (ECX bit 11)
            let has_avx512vnni = (cpuid7.ecx & (1 << 11)) != 0;
            
            // AVX-512 Bit Algorithms (ECX bit 12)
            let has_avx512bitalg = (cpuid7.ecx & (1 << 12)) != 0;
            
            // AVX-512 Vector Popcount (ECX bit 14)
            let has_avx512vpopcntdq = (cpuid7.ecx & (1 << 14)) != 0;
            
            // CPUID Leaf 7, Subleaf 1: Daha fazla AVX-512 özelliği
            let cpuid7_1 = __cpuid_count(7, 1);
            
            // AVX-512 Vector AES (EAX bit 0)
            let has_avx512vaes = (cpuid7_1.eax & (1 << 0)) != 0;
            
            // AVX-512 Vector Carry-less Multiplication (EAX bit 1)
            let has_avx512vpclmulqdq = (cpuid7_1.eax & (1 << 1)) != 0;
            
            // CPUID Leaf 0x80000001: AMD özellikleri
            let cpuid_ext = __cpuid(0x80000001);
            let has_3dnow = (cpuid_ext.edx & (1 << 31)) != 0;
            
            // Titan Node için minimum gereksinimler:
            // AVX-512F + AVX-512BW + AVX-512DQ + AVX-512VL
            let titan_ready = has_avx512f && has_avx512bw && has_avx512dq && has_avx512vl;
            
            // Loglama (opsiyonel)
            if titan_ready {
                tracing::info!(
                    "✅ Titan Hardware Verified: AVX-512F={}, AVX-512BW={}, AVX-512DQ={}, AVX-512VL={}",
                    has_avx512f, has_avx512bw, has_avx512dq, has_avx512vl
                );
                tracing::debug!(
                    "Additional features: AVX={}, FMA={}, VNNI={}, VAES={}",
                    has_avx, has_fma, has_avx512vnni, has_avx512vaes
                );
            } else {
                tracing::warn!(
                    "⚠️ Titan Hardware Incomplete: AVX-512F={}, AVX-512BW={}, AVX-512DQ={}, AVX-512VL={}",
                    has_avx512f, has_avx512bw, has_avx512dq, has_avx512vl
                );
            }
            
            (true, titan_ready)
        }
    }
    
    #[cfg(target_arch = "aarch64")]
    {
        // ARM64 (Apple Silicon, AWS Graviton) - AVX-512 yok, NEON var
        tracing::info!("ARM64 architecture detected - AVX-512 not available, using NEON");
        (true, false)
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        tracing::warn!("Unknown architecture - hardware features not verified");
        (false, false)
    }
}

/// Sadece AVX-512 kontrolü (basit versiyon)
pub fn has_avx512() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        unsafe {
            let cpuid7 = __cpuid(7);
            (cpuid7.ebx & (1 << 16)) != 0 &&  // AVX512F
            (cpuid7.ebx & (1 << 30)) != 0     // AVX512BW
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// CPU marka/model bilgisini al
pub fn get_cpu_info() -> String {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        unsafe {
            let mut brand = [0u8; 48];
            
            let cpuid0 = __cpuid(0x80000000);
            if cpuid0.eax >= 0x80000004 {
                let cpuid1 = __cpuid(0x80000002);
                let cpuid2 = __cpuid(0x80000003);
                let cpuid3 = __cpuid(0x80000004);
                
                brand[0..4].copy_from_slice(&cpuid1.eax.to_le_bytes());
                brand[4..8].copy_from_slice(&cpuid1.ebx.to_le_bytes());
                brand[8..12].copy_from_slice(&cpuid1.ecx.to_le_bytes());
                brand[12..16].copy_from_slice(&cpuid1.edx.to_le_bytes());
                
                brand[16..20].copy_from_slice(&cpuid2.eax.to_le_bytes());
                brand[20..24].copy_from_slice(&cpuid2.ebx.to_le_bytes());
                brand[24..28].copy_from_slice(&cpuid2.ecx.to_le_bytes());
                brand[28..32].copy_from_slice(&cpuid2.edx.to_le_bytes());
                
                brand[32..36].copy_from_slice(&cpuid3.eax.to_le_bytes());
                brand[36..40].copy_from_slice(&cpuid3.ebx.to_le_bytes());
                brand[40..44].copy_from_slice(&cpuid3.ecx.to_le_bytes());
                brand[44..48].copy_from_slice(&cpuid3.edx.to_le_bytes());
                
                String::from_utf8_lossy(&brand)
                    .trim_end_matches('\0')
                    .trim()
                    .to_string()
            } else {
                "Unknown x86_64 CPU".to_string()
            }
        }
    }
    
    #[cfg(target_arch = "aarch64")]
    {
        "ARM64 Processor".to_string()
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        "Unknown CPU Architecture".to_string()
    }
}

// ===================================================================
// VALIDATOR SABİTLERİ
// ===================================================================

pub const MIN_VALIDATOR_STAKE: u128 = 1_000_000_000_000; // 1M PAC
pub const MAX_VALIDATORS: usize = 21;
pub const UNBONDING_PERIOD_SECS: u64 = 14 * 24 * 60 * 60; // 14 gün

// ===================================================================
// VALIDATOR MANAGER
// ===================================================================

pub struct ValidatorManager {
    validators: Arc<RwLock<HashMap<u64, ValidatorRecord>>>,
    address_to_id: Arc<RwLock<HashMap<Address, u64>>>,
    next_validator_id: Arc<RwLock<u64>>,
    current_set: Arc<RwLock<ValidatorSet>>,
    epoch: Arc<RwLock<u64>>,
    epoch_start_height: Arc<RwLock<BlockHeight>>,
}

impl ValidatorManager {
    pub fn new() -> Self {
        Self {
            validators: Arc::new(RwLock::new(HashMap::new())),
            address_to_id: Arc::new(RwLock::new(HashMap::new())),
            next_validator_id: Arc::new(RwLock::new(1)),
            current_set: Arc::new(RwLock::new(ValidatorSet::new())),
            epoch: Arc::new(RwLock::new(0)),
            epoch_start_height: Arc::new(RwLock::new(0)),
        }
    }
    
    pub fn register_validator(
        &self,
        address: Address,
        public_key: Vec<u8>,
        stake: u128,
    ) -> PacyteResult<u64> {
        // Donanım kontrolü (GERÇEK!)
        let (hardware_verified, avx512_supported) = check_hardware_features();
        
        if !avx512_supported {
            return Err(PacyteError::HardwareInsufficient);
        }
        
        if stake < MIN_VALIDATOR_STAKE {
            return Err(PacyteError::InsufficientStake {
                have: stake,
                need: MIN_VALIDATOR_STAKE,
            });
        }
        
        {
            let addr_map = self.address_to_id.read();
            if addr_map.contains_key(&address) {
                return Err(PacyteError::ValidatorAlreadyExists);
            }
        }
        
        {
            let validators = self.validators.read();
            let active_count = validators
                .values()
                .filter(|v| v.status == ValidatorStatus::Active)
                .count();
            
            if active_count >= MAX_VALIDATORS {
                return self.replace_validator(address, public_key, stake);
            }
        }
        
        let id = {
            let mut next_id = self.next_validator_id.write();
            let id = *next_id;
            *next_id += 1;
            id
        };
        
        let mut record = ValidatorRecord::new(id, address, public_key, stake);
        record.hardware_verified = hardware_verified;
        record.avx512_supported = avx512_supported;
        
        {
            let mut validators = self.validators.write();
            validators.insert(id, record);
        }
        
        {
            let mut addr_map = self.address_to_id.write();
            addr_map.insert(address, id);
        }
        
        self.update_validator_set()?;
        
        tracing::info!("Validator {} registered (AVX-512: {})", id, avx512_supported);
        
        Ok(id)
    }
    
    fn replace_validator(
        &self,
        address: Address,
        public_key: Vec<u8>,
        stake: u128,
    ) -> PacyteResult<u64> {
        let mut validators = self.validators.write();
        
        let min_validator = validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Active)
            .min_by_key(|v| v.stake);
        
        if let Some(min_v) = min_validator {
            let min_stake = min_v.stake;
            
            if stake > min_stake {
                let old_id = min_v.id;
                
                if let Some(old) = validators.get_mut(&old_id) {
                    old.status = ValidatorStatus::Inactive;
                }
                
                let id = {
                    let mut next_id = self.next_validator_id.write();
                    let id = *next_id;
                    *next_id += 1;
                    id
                };
                
                let (verified, supported) = check_hardware_features();
                let mut record = ValidatorRecord::new(id, address, public_key, stake);
                record.hardware_verified = verified;
                record.avx512_supported = supported;
                
                validators.insert(id, record);
                
                {
                    let mut addr_map = self.address_to_id.write();
                    addr_map.insert(address, id);
                }
                
                self.update_validator_set()?;
                
                tracing::info!("Validator {} replaced {}", id, old_id);
                
                return Ok(id);
            }
        }
        
        Err(PacyteError::ValidatorSetFull)
    }
    
    pub fn update_validator_set(&self) -> PacyteResult<()> {
        let validators = self.validators.read();
        
        let mut set = ValidatorSet::new();
        set.epoch = *self.epoch.read();
        
        let mut active: Vec<_> = validators
            .values()
            .filter(|v| v.status == ValidatorStatus::Active && v.avx512_supported)
            .collect();
        
        active.sort_by_key(|v| std::cmp::Reverse(v.stake));
        
        for v in active.iter().take(MAX_VALIDATORS) {
            set.add_validator(ValidatorInfo {
                id: v.id,
                address: v.address,
                public_key: v.public_key.clone(),
                stake: v.stake,
                voting_power: v.voting_power,
                is_active: true,
            });
        }
        
        *self.current_set.write() = set;
        
        Ok(())
    }
    
    pub fn get_validator(&self, id: u64) -> Option<ValidatorRecord> {
        self.validators.read().get(&id).cloned()
    }
    
    pub fn get_validator_by_address(&self, address: &Address) -> Option<ValidatorRecord> {
        let addr_map = self.address_to_id.read();
        let id = addr_map.get(address)?;
        self.get_validator(*id)
    }
    
    pub fn get_active_set(&self) -> ValidatorSet {
        self.current_set.read().clone()
    }
    
    pub fn get_proposer(&self, height: BlockHeight, round: u64) -> Option<u64> {
        let set = self.current_set.read();
        set.get_proposer(height, round).map(|v| v.id)
    }
    
    pub fn active_count(&self) -> usize {
        self.current_set.read().active_count()
    }
    
    pub fn slash(&self, validator_id: u64, amount: u128, reason: &str) -> PacyteResult<()> {
        let mut validators = self.validators.write();
        
        let validator = validators.get_mut(&validator_id)
            .ok_or_else(|| PacyteError::ValidatorNotFound(validator_id))?;
        
        validator.slash(amount, reason)?;
        
        if validator.status == ValidatorStatus::Inactive {
            self.update_validator_set()?;
        }
        
        Ok(())
    }
    
    pub fn slash_double_sign(&self, validator_id: u64) -> PacyteResult<()> {
        let mut validators = self.validators.write();
        
        let validator = validators.get_mut(&validator_id)
            .ok_or_else(|| PacyteError::ValidatorNotFound(validator_id))?;
        
        let slash_amount = validator.stake / 20;
        validator.slash(slash_amount, "Double signing")?;
        validator.status = ValidatorStatus::Slashed;
        
        self.update_validator_set()?;
        
        tracing::error!("Validator {} slashed for double signing!", validator_id);
        
        Ok(())
    }
}

impl Default for ValidatorManager {
    fn default() -> Self {
        Self::new()
    }
}

// ===================================================================
// TITAN NODE (Hardware Meritocracy)
// ===================================================================

#[derive(Debug, Clone)]
pub struct TitanNode {
    pub id: u64,
    pub address: Address,
    pub public_key: Vec<u8>,
    pub merit_score: f64,
    pub last_latency_ms: u64,
    pub consecutive_fails: u32,
    pub is_active: bool,
    pub stake_amount: u128,
    pub joined_at: u64,
    pub cpu_info: String,
    pub avx512_supported: bool,
    pub core_count: usize,
}

impl TitanNode {
    pub fn new(id: u64, address: Address, stake: u128) -> Self {
        let keypair = HybridSigner::new_both();
        let cpu_info = get_cpu_info();
        let avx512_supported = has_avx512();
        let core_count = num_cpus::get();
        
        Self {
            id,
            address,
            public_key: keypair.public_keys().to_bytes(),
            merit_score: 100.0,
            last_latency_ms: 0,
            consecutive_fails: 0,
            is_active: true,
            stake_amount: stake,
            joined_at: current_timestamp(),
            cpu_info,
            avx512_supported,
            core_count,
        }
    }
    
    /// GERÇEK AVX-512 kontrolü (CPUID ile - simülasyon DEĞİL!)
    pub fn has_avx512(&self) -> bool {
        has_avx512()
    }
    
    /// Donanım yeterlilik kontrolü (Titan Grade)
    pub fn is_titan_grade(&self) -> bool {
        self.avx512_supported && self.core_count >= 32
    }
    
    /// Merit skorunu güncelle
    pub fn update_merit(&mut self, zk_latency_ms: u64) {
        self.last_latency_ms = zk_latency_ms;
        
        const ZK_LATENCY_LIMIT_MS: u64 = 800;
        
        if zk_latency_ms > ZK_LATENCY_LIMIT_MS {
            self.consecutive_fails += 1;
            self.merit_score *= 0.95;
        } else {
            self.consecutive_fails = 0;
            let reward_factor = if zk_latency_ms < 500 { 1.02 } else { 1.0 };
            self.merit_score = (self.merit_score * reward_factor).min(200.0);
        }
        
        if self.consecutive_fails >= 10 {
            self.is_active = false;
        }
    }
    
    pub fn reward_weight(&self) -> f64 {
        if !self.is_active || !self.avx512_supported {
            return 0.0;
        }
        self.merit_score * (self.stake_amount as f64).sqrt()
    }
    
    pub fn hardware_report(&self) -> String {
        format!(
            "CPU: {}, Cores: {}, AVX-512: {}, Titan Grade: {}",
            self.cpu_info,
            self.core_count,
            if self.avx512_supported { "✅" } else { "❌" },
            if self.is_titan_grade() { "✅" } else { "❌" }
        )
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Ed25519Signer;

    #[test]
    fn test_has_avx512() {
        let result = has_avx512();
        // Test ortamında true veya false olabilir, panic yapmamalı
        println!("AVX-512 supported: {}", result);
    }
    
    #[test]
    fn test_get_cpu_info() {
        let cpu = get_cpu_info();
        assert!(!cpu.is_empty());
        println!("CPU: {}", cpu);
    }
    
    #[test]
    fn test_check_hardware_features() {
        let (verified, supported) = check_hardware_features();
        println!("Hardware verified: {}, AVX-512: {}", verified, supported);
    }
    
    #[test]
    fn test_register_validator() {
        let manager = ValidatorManager::new();
        let signer = Ed25519Signer::generate();
        
        // AVX-512 yoksa bu test başarısız olabilir
        let result = manager.register_validator(
            signer.address(),
            signer.public_key_bytes(),
            MIN_VALIDATOR_STAKE,
        );
        
        match result {
            Ok(id) => {
                println!("Validator registered: {}", id);
                let v = manager.get_validator(id).unwrap();
                println!("Hardware report: AVX-512={}", v.avx512_supported);
            }
            Err(e) => {
                println!("Validator registration failed (expected if no AVX-512): {}", e);
            }
        }
    }
    
    #[test]
    fn test_titan_node() {
        let node = TitanNode::new(1, [1u8; 32], MIN_VALIDATOR_STAKE);
        println!("{}", node.hardware_report());
    }
}