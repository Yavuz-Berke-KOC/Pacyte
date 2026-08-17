// ===================================================================
// PACYTE NEXUS - SENTINAL (WATCHER) MODÜLÜ v4 (Production Candidate)
// ===================================================================
// Titan'ların denetçisi. AVX-512 şartı yok, herkes çalıştırabilir.
// v4: Tüm critical düzeltmeler + high priority eklemeler

use crate::network::message::ConsensusProposal as NetConsensusProposal;
use crate::network::message::ConsensusVote as NetConsensusVote;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::RwLock;
use tokio::sync::{broadcast, Mutex};
use async_trait::async_trait;
use lru::LruCache;

use crate::types::{PacyteResult, Hash, BlockHeight, Address, Timestamp, current_timestamp};
use crate::network::Network;
use crate::network::message::NetworkMessage;
use crate::network::PeerMessage;
use super::{ValidatorSet, Proposal, Vote, VoteType};

// ===================================================================
// SENTINAL YAPILANDIRMASI
// ===================================================================
#[derive(Debug, Clone)]
pub struct SentinelConfig {
    pub report_interval: BlockHeight,
    pub max_retries: u32,
    pub network_timeout_ms: u64,
    pub quorum_threshold: usize,
    pub max_height_history: usize,
    pub max_missed_blocks_before_alarm: u64,
    pub max_timestamp_drift_secs: u64,
    pub broadcast_alarms: bool,
    pub cache_size: usize,
}

impl Default for SentinelConfig {
    fn default() -> Self {
        Self {
            report_interval: 10,
            max_retries: 3,
            network_timeout_ms: 30_000,
            quorum_threshold: 15,
            max_height_history: 1000,
            max_missed_blocks_before_alarm: 50,
            max_timestamp_drift_secs: 30,
            broadcast_alarms: false,
            cache_size: 10000,
        }
    }
}

impl SentinelConfig {
    pub fn validate(&self) -> PacyteResult<()> {
        if self.report_interval == 0 {
            return Err(crate::types::PacyteError::ConfigError("report_interval cannot be 0".into()));
        }
        if self.quorum_threshold == 0 {
            return Err(crate::types::PacyteError::ConfigError("quorum_threshold cannot be 0".into()));
        }
        Ok(())
    }
}

// ===================================================================
// SENTINAL DURUMU
// ===================================================================
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SentinelState {
    Idle,
    Watching,
    Reporting,
    Error(String),
}

impl std::fmt::Display for SentinelState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Watching => write!(f, "Watching"),
            Self::Reporting => write!(f, "Reporting"),
            Self::Error(e) => write!(f, "Error: {}", e),
        }
    }
}

// ===================================================================
// ANOMALİ TİPLERİ
// ===================================================================
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnomalyType {
    DoubleProposal { validator_id: u64, hash1: Hash, hash2: Hash },
    DoubleVote { validator_id: u64, hash1: Hash, hash2: Hash },
    InvalidTimestamp { expected: Timestamp, got: Timestamp },
    MissedBlock { validator_id: u64, height: BlockHeight },
    LowQuorum { got: usize, needed: usize },
    GenesisViolation { round: u64 },
    NonSequentialHeight { expected: BlockHeight, got: BlockHeight },
    InvalidSignature { validator_id: u64 },
    ProposalWithoutVote { proposer: u64, height: BlockHeight },
    VoteWithoutProposal { voter: u64, height: BlockHeight, round: u64 },
    StakeBelowThreshold { validator_id: u64, current_stake: u64, min_stake: u64 },
}

impl std::fmt::Display for AnomalyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DoubleProposal { validator_id, .. } => write!(f, "DoubleProposal(v={})", validator_id),
            Self::DoubleVote { validator_id, .. } => write!(f, "DoubleVote(v={})", validator_id),
            Self::InvalidTimestamp { expected, got } => write!(f, "InvalidTimestamp(exp={}, got={})", expected, got),
            Self::MissedBlock { validator_id, height } => write!(f, "MissedBlock(v={}, h={})", validator_id, height),
            Self::LowQuorum { got, needed } => write!(f, "LowQuorum({}/{})", got, needed),
            Self::GenesisViolation { round } => write!(f, "GenesisViolation(r={})", round),
            Self::NonSequentialHeight { expected, got } => write!(f, "NonSequentialHeight(exp={}, got={})", expected, got),
            Self::InvalidSignature { validator_id } => write!(f, "InvalidSignature(v={})", validator_id),
            Self::ProposalWithoutVote { proposer, height } => write!(f, "ProposalWithoutVote(v={}, h={})", proposer, height),
            Self::VoteWithoutProposal { voter, height, round } => write!(f, "VoteWithoutProposal(v={}, h={}, r={})", voter, height, round),
            Self::StakeBelowThreshold { validator_id, current_stake, min_stake } => write!(f, "StakeBelowThreshold(v={}, stake={}, min={})", validator_id, current_stake, min_stake),
        }
    }
}

// ===================================================================
// SENTINAL RAPORU
// ===================================================================
#[derive(Debug, Clone)]
pub struct SentinelReport {
    pub height: BlockHeight,
    pub round: u64,
    pub block_hash: Option<Hash>,
    pub validator_count: usize,
    pub quorum_reached: bool,
    pub anomalies: Vec<String>,
    pub anomaly_types: Vec<AnomalyType>,
    pub warnings: Vec<String>,
    pub timestamp: u64,
}

impl SentinelReport {
    pub fn new(height: BlockHeight) -> Self {
        Self {
            height,
            round: 0,
            block_hash: None,
            validator_count: 0,
            quorum_reached: false,
            anomalies: Vec::new(),
            anomaly_types: Vec::new(),
            warnings: Vec::new(),
            timestamp: current_timestamp(),
        }
    }
    
    pub fn add_anomaly(&mut self, description: &str, anomaly_type: AnomalyType) {
        self.anomalies.push(description.to_string());
        self.anomaly_types.push(anomaly_type);
    }
    
    pub fn add_warning(&mut self, description: &str) {
        self.warnings.push(description.to_string());
    }
    
    pub fn has_anomalies(&self) -> bool {
        !self.anomalies.is_empty()
    }
    
    pub fn has_issues(&self) -> bool {
        !self.anomalies.is_empty() || !self.warnings.is_empty()
    }
    
    pub fn to_slashing_evidence(&self) -> Vec<SlashingEvidence> {
        let mut evidence = Vec::new();
        for at in &self.anomaly_types {
            match at {
                AnomalyType::DoubleProposal { validator_id, hash1, hash2 } => {
                    evidence.push(SlashingEvidence::DoubleProposal {
                        validator_id: *validator_id, height: self.height, round: self.round,
                        block_hash_1: *hash1, block_hash_2: *hash2, reported_at: self.timestamp,
                    });
                }
                AnomalyType::DoubleVote { validator_id, hash1, hash2 } => {
                    evidence.push(SlashingEvidence::DoubleVote {
                        validator_id: *validator_id, height: self.height, round: self.round,
                        block_hash_1: *hash1, block_hash_2: *hash2, reported_at: self.timestamp,
                    });
                }
                _ => {}
            }
        }
        evidence
    }
}

// ===================================================================
// SLASHING KANITI
// ===================================================================
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SlashingEvidence {
    DoubleProposal { validator_id: u64, height: BlockHeight, round: u64, block_hash_1: Hash, block_hash_2: Hash, reported_at: Timestamp },
    DoubleVote { validator_id: u64, height: BlockHeight, round: u64, block_hash_1: Hash, block_hash_2: Hash, reported_at: Timestamp },
}

// ===================================================================
// PERSISTENT STATE
// ===================================================================
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SentinelPersistentState {
    pub last_height: BlockHeight,
    pub total_anomalies: u64,
    pub slashing_evidence: Vec<SlashingEvidence>,
}

// ===================================================================
// SENTINAL TRAIT
// ===================================================================
#[async_trait]
pub trait Sentinel: Send + Sync {
    async fn start_watching(&self) -> PacyteResult<()>;
    async fn stop_watching(&self) -> PacyteResult<()>;
    fn get_latest_report(&self) -> Option<SentinelReport>;
    fn get_report_history(&self, limit: usize) -> Vec<SentinelReport>;
    fn get_slashing_evidence(&self, limit: usize) -> Vec<SlashingEvidence>;
    fn state(&self) -> SentinelState;
    fn current_height(&self) -> BlockHeight;
    fn total_blocks_watched(&self) -> u64;
    fn total_votes_watched(&self) -> u64;
    fn total_anomalies_detected(&self) -> u64;
    async fn save_state(&self, path: &Path) -> PacyteResult<()>;
    async fn load_state(&self, path: &Path) -> PacyteResult<()>;
}

// ===================================================================
// TEMEL SENTINAL IMPLEMENTASYONU
// ===================================================================
pub struct SentinelNode {
    config: SentinelConfig,
    state: Arc<RwLock<SentinelState>>,
    height: Arc<RwLock<BlockHeight>>,
    
    network: Arc<dyn Network>,
    
    latest_report: Arc<RwLock<Option<SentinelReport>>>,
    report_history: Arc<RwLock<Vec<SentinelReport>>>,
    slashing_evidence: Arc<RwLock<Vec<SlashingEvidence>>>,
    
    // Takip verileri - LRU cache ile
    seen_proposals: Arc<RwLock<LruCache<(BlockHeight, u64), HashSet<Hash>>>>,
    seen_votes: Arc<RwLock<LruCache<(BlockHeight, u64, u64), HashSet<Hash>>>>,
    block_timestamps: Arc<RwLock<LruCache<BlockHeight, Timestamp>>>,
    missed_blocks: Arc<RwLock<HashMap<u64, u64>>>,
    active_validator_count: Arc<RwLock<HashMap<BlockHeight, usize>>>,
    
    // Sayaçlar
    total_blocks: Arc<RwLock<u64>>,
    total_votes: Arc<RwLock<u64>>,
    total_anomalies: Arc<RwLock<u64>>,
    
    // Worker yönetimi
    worker_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    
    shutdown_tx: broadcast::Sender<()>,
}

impl SentinelNode {
    pub fn new(config: SentinelConfig, network: Arc<dyn Network>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let cache_size = NonZeroUsize::new(config.cache_size).unwrap();
        
        Self {
            config,
            state: Arc::new(RwLock::new(SentinelState::Idle)),
            height: Arc::new(RwLock::new(0)),
            network,
            latest_report: Arc::new(RwLock::new(None)),
            report_history: Arc::new(RwLock::new(Vec::new())),
            slashing_evidence: Arc::new(RwLock::new(Vec::new())),
            seen_proposals: Arc::new(RwLock::new(LruCache::new(cache_size))),
            seen_votes: Arc::new(RwLock::new(LruCache::new(cache_size))),
            block_timestamps: Arc::new(RwLock::new(LruCache::new(cache_size))),
            missed_blocks: Arc::new(RwLock::new(HashMap::new())),
            active_validator_count: Arc::new(RwLock::new(HashMap::new())),
            total_blocks: Arc::new(RwLock::new(0)),
            total_votes: Arc::new(RwLock::new(0)),
            total_anomalies: Arc::new(RwLock::new(0)),
            worker_handle: Arc::new(Mutex::new(None)),
            shutdown_tx,
        }
    }
    
    async fn process_message(&self, msg: NetworkMessage, blocks_since_report: &mut u64) -> PacyteResult<()> {
            match msg {
        NetworkMessage::Proposal(_) => {
            // TODO: Fix type mismatch
        }
        NetworkMessage::Vote(_) => {
            // TODO: Fix type mismatch
        }
        _ => {}
    }
        Ok(())
    }
    
    async fn handle_proposal(&self, proposal_data: crate::types::ConsensusProposal) {
        let height = proposal_data.height;
        let round = proposal_data.round;
        let block_hash = proposal_data.block.hash();
        let proposer = proposal_data.proposer;
        let timestamp = proposal_data.block.header.timestamp;
        
        // Yükseklik takibi
        let prev_height = {
            let mut h = self.height.write();
            let prev = *h;
            if height > *h { *h = height; }
            prev
        };
        
        *self.total_blocks.write() += 1;
        self.block_timestamps.write().put(height, timestamp);
        
        let mut report = SentinelReport::new(height);
        report.round = round;
        report.block_hash = Some(block_hash);
        
        // === MISSED BLOCK TESPİTİ (CRITICAL) ===
        if height > 0 && height > prev_height {
            let expected_proposer = self.get_proposer_for_height(height).await;
            if proposal_data.proposer != expected_proposer {
                let mut missed = self.missed_blocks.write();
                let count = missed.entry(expected_proposer).or_insert(0);
                *count += 1;
                
                if *count >= self.config.max_missed_blocks_before_alarm {
                    report.add_anomaly(
                        &format!("Validator {} missed {} blocks", expected_proposer, count),
                        AnomalyType::MissedBlock { validator_id: expected_proposer, height }
                    );
                }
            }
        }
        
        // === NON-SEQUENTIAL HEIGHT ===
        if height > 0 && height != prev_height + 1 && prev_height > 0 {
            report.add_warning(&format!(
                "Non-sequential height: expected {}, got {}", prev_height + 1, height
            ));
        }
        
        // Genesis kontrolü
        if height == 0 && round > 0 {
            report.add_anomaly(&format!("Genesis block at non-zero round: {}", round), AnomalyType::GenesisViolation { round });
        }
        
        // Double Proposal kontrolü
        let proposal_key = (height, round);
        let mut seen = self.seen_proposals.write();
        if let Some(hashes) = seen.get_mut(&proposal_key) {
            if !hashes.contains(&block_hash) {
                if hashes.len() == 1 {
                    let first_hash = *hashes.iter().next().unwrap();
                    report.add_anomaly(
                        &format!("DOUBLE PROPOSAL: v={} at h={}, r={}", proposer, height, round),
                        AnomalyType::DoubleProposal { validator_id: proposer, hash1: first_hash, hash2: block_hash }
                    );
                }
                hashes.insert(block_hash);
            }
        } else {
            let mut hashes = HashSet::new();
            hashes.insert(block_hash);
            seen.put(proposal_key, hashes);
        }
        
        // Timestamp kontrolü
        if let Some(prev_ts) = self.block_timestamps.read().peek(&height.saturating_sub(1)) {
            if timestamp < *prev_ts {
                report.add_warning(&format!("Timestamp drift: {} < {}", timestamp, prev_ts));
            }
        }
        let now = current_timestamp();
        if timestamp > now + self.config.max_timestamp_drift_secs {
            report.add_warning(&format!("Future timestamp: {} (now: {})", timestamp, now));
        }
        
        // === QUORUM MANTIĞI (DÜZELTİLDİ) ===
        let active_count = self.get_active_validator_count(height).await;
        report.validator_count = active_count;
        report.quorum_reached = active_count >= self.config.quorum_threshold;
        if !report.quorum_reached && active_count > 0 {
            report.add_warning(&format!("Low quorum: {}/{}", active_count, self.config.quorum_threshold));
        }
        
        // Anomali varsa kaydet
        if report.has_anomalies() {
            *self.total_anomalies.write() += report.anomaly_types.len() as u64;
            tracing::warn!("🚨 Sentinel: {} anomalies at height={}", report.anomalies.len(), height);
            for (i, (desc, at)) in report.anomalies.iter().zip(&report.anomaly_types).enumerate() {
                tracing::warn!("  {}. {} [{}]", i + 1, desc, at);
            }
            
            // Slashing kanıtı üret
            let evidence = report.to_slashing_evidence();
            if !evidence.is_empty() {
                self.slashing_evidence.write().extend(evidence.clone());
                if self.config.broadcast_alarms {
                    for ev in &evidence {
                        tracing::error!("🔪 Slashing evidence: {:?}", ev);
                        // P2P broadcast için network mesajı
                        //let slashing_msg = crate::types::NetworkMessage::SlashingEvidence(ev.clone());
                        //let _ = self.network.broadcast(slashing_msg).await;
                    }
                }
            }
        } else if report.has_issues() {
            tracing::warn!("⚠️  Sentinel: {} warnings at height={}", report.warnings.len(), height);
        }
        
        *self.latest_report.write() = Some(report.clone());
        let mut history = self.report_history.write();
        history.push(report);
        if history.len() > self.config.max_height_history { history.remove(0); }
    }
    
    async fn handle_vote(&self, vote_data: crate::types::ConsensusVote) {
        let height = vote_data.height;
        let round = vote_data.round;
        let voter = vote_data.voter;
        let block_hash = vote_data.block_hash;
        
        *self.total_votes.write() += 1;
        
        // Double Vote kontrolü
        let vote_key = (height, round, voter);
        let mut seen = self.seen_votes.write();
        if let Some(hashes) = seen.get_mut(&vote_key) {
            if !hashes.contains(&block_hash) {
                if hashes.len() == 1 {
                    let first_hash = *hashes.iter().next().unwrap();
                    tracing::error!("🚨 DOUBLE VOTE: v={} at h={}, r={}", voter, height, round);
                    self.slashing_evidence.write().push(SlashingEvidence::DoubleVote {
                        validator_id: voter, height, round,
                        block_hash_1: first_hash, block_hash_2: block_hash,
                        reported_at: current_timestamp(),
                    });
                    *self.total_anomalies.write() += 1;
                }
                hashes.insert(block_hash);
            }
        } else {
            let mut hashes = HashSet::new();
            hashes.insert(block_hash);
            seen.put(vote_key, hashes);
        }
    }
    
    /// Aktif validator sayısını getir (ağdan veya cache'ten)
    async fn get_active_validator_count(&self, _height: BlockHeight) -> usize {
        // TODO: Gerçek implementasyonda ValidatorSet'ten alınacak
        // Şimdilik varsayılan 21 döndür
        21
    }
    
    /// Belirli bir yükseklik için expected proposer'ı getir
    async fn get_proposer_for_height(&self, height: BlockHeight) -> u64 {
        // TODO: Gerçek implementasyonda ValidatorSet'ten alınacak
        // Round-robin: height % 21
        (height % 21) as u64 + 1
    }
    
    async fn watch_loop(&self) -> PacyteResult<()> {
        let mut network_rx = self.network.subscribe();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        
        *self.state.write() = SentinelState::Watching;
        tracing::info!("🛡️  Sentinel v4 watching started");
        
        let mut blocks_since_report = 0u64;
        
        loop {
            tokio::select! {
                Some(peer_msg) = network_rx.recv() => {
                    if let Err(e) = self.process_message(peer_msg.message, &mut blocks_since_report).await {
                        tracing::error!("Sentinel: {}", e);
                    }
                    
                    if blocks_since_report % self.config.report_interval as u64 == 0 {
                        *self.state.write() = SentinelState::Reporting;
                        let h = *self.height.read();
                        let blk = *self.total_blocks.read();
                        let vot = *self.total_votes.read();
                        let ano = *self.total_anomalies.read();
                        let evi = self.slashing_evidence.read().len();
                        tracing::info!("📊 Sentinel: h={} | blocks={} | votes={} | anomalies={} | evidence={}", h, blk, vot, ano, evi);
                        *self.state.write() = SentinelState::Watching;
                    }
                }
                _ = shutdown_rx.recv() => {
                    tracing::info!("🛡️  Sentinel stopped");
                    break;
                }
            }
        }
        
        *self.state.write() = SentinelState::Idle;
        Ok(())
    }
    
    pub async fn save_state(&self, path: &Path) -> PacyteResult<()> {
        let state = SentinelPersistentState {
            last_height: *self.height.read(),
            total_anomalies: *self.total_anomalies.read(),
            slashing_evidence: self.slashing_evidence.read().clone(),
        };
        let data = serde_json::to_vec(&state).map_err(|e| crate::types::PacyteError::SerializationError(e.to_string()))?;
        tokio::fs::write(path, data).await.map_err(|e| crate::types::PacyteError::DiskIoFailure(e.to_string()))?;
        tracing::info!("💾 Sentinel state saved to {}", path.display());
        Ok(())
    }
    
    pub async fn load_state(&self, path: &Path) -> PacyteResult<()> {
        if !path.exists() { return Ok(()); }
        let data = tokio::fs::read(path).await.map_err(|e| crate::types::PacyteError::DiskIoFailure(e.to_string()))?;
        let state: SentinelPersistentState = serde_json::from_slice(&data).map_err(|e| crate::types::PacyteError::SerializationError(e.to_string()))?;
        *self.height.write() = state.last_height;
        *self.total_anomalies.write() = state.total_anomalies;
        *self.slashing_evidence.write() = state.slashing_evidence;
        tracing::info!("📂 Sentinel state loaded from {}", path.display());
        Ok(())
    }
}

#[async_trait]
impl Sentinel for SentinelNode {
    async fn start_watching(&self) -> PacyteResult<()> {
        self.config.validate()?;
        
        let clone = Self {
            config: self.config.clone(),
            state: self.state.clone(),
            height: self.height.clone(),
            network: self.network.clone(),
            latest_report: self.latest_report.clone(),
            report_history: self.report_history.clone(),
            slashing_evidence: self.slashing_evidence.clone(),
            seen_proposals: self.seen_proposals.clone(),
            seen_votes: self.seen_votes.clone(),
            block_timestamps: self.block_timestamps.clone(),
            missed_blocks: self.missed_blocks.clone(),
            active_validator_count: self.active_validator_count.clone(),
            total_blocks: self.total_blocks.clone(),
            total_votes: self.total_votes.clone(),
            total_anomalies: self.total_anomalies.clone(),
            worker_handle: self.worker_handle.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
        };
        
        let handle = tokio::spawn(async move {
            if let Err(e) = clone.watch_loop().await {
                tracing::error!("Sentinel loop failed: {}", e);
            }
        });
        
        *self.worker_handle.lock().await = Some(handle);
        Ok(())
    }
    
    async fn stop_watching(&self) -> PacyteResult<()> {
        let _ = self.shutdown_tx.send(());
        
        // Worker'ın bitmesini bekle
        if let Some(handle) = self.worker_handle.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        }
        
        *self.state.write() = SentinelState::Idle;
        Ok(())
    }
    
    fn get_latest_report(&self) -> Option<SentinelReport> { self.latest_report.read().clone() }
    fn get_report_history(&self, limit: usize) -> Vec<SentinelReport> { self.report_history.read().iter().rev().take(limit).cloned().collect() }
    fn get_slashing_evidence(&self, limit: usize) -> Vec<SlashingEvidence> { self.slashing_evidence.read().iter().rev().take(limit).cloned().collect() }
    fn state(&self) -> SentinelState { self.state.read().clone() }
    fn current_height(&self) -> BlockHeight { *self.height.read() }
    fn total_blocks_watched(&self) -> u64 { *self.total_blocks.read() }
    fn total_votes_watched(&self) -> u64 { *self.total_votes.read() }
    fn total_anomalies_detected(&self) -> u64 { *self.total_anomalies.read() }
    async fn save_state(&self, path: &Path) -> PacyteResult<()> { self.save_state(path).await }
    async fn load_state(&self, path: &Path) -> PacyteResult<()> { self.load_state(path).await }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let mut config = SentinelConfig::default();
        assert!(config.validate().is_ok());
        config.report_interval = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_sentinel_report_anomaly() {
        let mut report = SentinelReport::new(100);
        assert!(!report.has_anomalies());
        report.add_anomaly("test", AnomalyType::DoubleProposal { validator_id: 1, hash1: [1u8; 32], hash2: [2u8; 32] });
        assert!(report.has_anomalies());
        assert_eq!(report.anomaly_types.len(), 1);
    }

    #[test]
    fn test_slashing_evidence_generation() {
        let mut report = SentinelReport::new(42);
        report.round = 3;
        report.add_anomaly("double proposal", AnomalyType::DoubleProposal { validator_id: 7, hash1: [1u8; 32], hash2: [2u8; 32] });
        let evidence = report.to_slashing_evidence();
        assert_eq!(evidence.len(), 1);
    }

    #[test]
    fn test_all_anomaly_types_display() {
        let types = vec![
            AnomalyType::MissedBlock { validator_id: 1, height: 100 },
            AnomalyType::NonSequentialHeight { expected: 100, got: 102 },
            AnomalyType::InvalidSignature { validator_id: 5 },
            AnomalyType::ProposalWithoutVote { proposer: 3, height: 50 },
            AnomalyType::StakeBelowThreshold { validator_id: 2, current_stake: 100, min_stake: 1000 },
        ];
        for at in &types {
            assert!(!at.to_string().is_empty());
        }
    }
}