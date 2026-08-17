// ===================================================================
// PACYTE NEXUS - KONSENSÜS MODÜLÜ
// ===================================================================

pub mod sentinel;
pub mod engine;
pub mod validator;
pub mod proposal;
pub mod vote;
pub mod round;
pub mod safety;
pub mod pacemaker;

// Re-export'lar
pub use engine::*;
pub use validator::*;
pub use proposal::*;
pub use vote::*;
pub use round::*;
pub use safety::*;
pub use pacemaker::*;

use crate::types::{
    PacyteError, PacyteResult, Hash, BlockHeight, Address, Signature, Timestamp,
};
use crate::types::block::{Block, BlockHeader};
use crate::network::message::NetworkMessage;
use std::sync::Arc;
use tokio::sync::mpsc;

// ===================================================================
// KONSENSÜS KONFİGÜRASYONU
// ===================================================================

#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    pub max_block_size: usize,
    pub validator_count: usize,           // 21 Titan
    pub quorum_size: usize,               // 2/3 + 1 = 15
    pub block_time_target_ms: u64,        // 1000 ms
    pub proposal_timeout_ms: u64,         // 3000 ms
    pub vote_timeout_ms: u64,             // 2000 ms
    pub round_timeout_ms: u64,            // 4000 ms
    pub max_rounds: u64,                  // 10
    pub sync_timeout_ms: u64,             // 10000 ms
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
	    max_block_size: 1_000_000,
            validator_count: 21,
            quorum_size: 15, // 2/3 * 21 + 1
            block_time_target_ms: 1000,
            proposal_timeout_ms: 3000,
            vote_timeout_ms: 2000,
            round_timeout_ms: 4000,
            max_rounds: 10,
            sync_timeout_ms: 10000,
        }
    }
}

impl ConsensusConfig {
    pub fn quorum_size(&self) -> usize {
        (self.validator_count * 2 / 3) + 1
    }
    
    pub fn is_quorum(&self, votes: usize) -> bool {
        votes >= self.quorum_size()
    }
}

// ===================================================================
// KONSENSÜS DURUMU
// ===================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsensusState {
    Idle,
    Proposing,
    Prevoting,
    Precommitting,
    Committed,
    WaitingForBlock,
    Syncing,
}

impl std::fmt::Display for ConsensusState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ===================================================================
// KONSENSÜS MESAJLARI
// ===================================================================

#[derive(Debug, Clone)]
pub enum ConsensusCommand {
    /// Yeni blok öner
    Propose { height: BlockHeight, round: u64 },
    
    /// Öneri alındı
    ProposalReceived { proposal: Proposal },
    
    /// Oy alındı
    VoteReceived { vote: Vote },
    
    /// Timeout oluştu
    Timeout { height: BlockHeight, round: u64 },
    
    /// Yeni blok alındı (ağdan)
    NewBlock { block: Block },
    
    /// Senkronizasyon tamamlandı
    Synced { height: BlockHeight },
    
    /// Durdur
    Stop,
}

#[derive(Debug, Clone)]
pub enum ConsensusEvent {
    /// Blok commit edildi
    BlockCommitted { block: Block, height: BlockHeight },
    
    /// Yeni round başladı
    NewRound { height: BlockHeight, round: u64 },
    
    /// Proposal gönderildi
    ProposalSent { height: BlockHeight, round: u64 },
    
    /// Vote gönderildi
    VoteSent { height: BlockHeight, round: u64, vote_type: VoteType },
    
    /// Quorum sağlandı
    QuorumReached { height: BlockHeight, round: u64, vote_type: VoteType },
    
    /// Timeout
    Timeout { height: BlockHeight, round: u64 },
    
    /// Hata
    Error { error: PacyteError },
}

// ===================================================================
// KONSENSÜS TRAIT
// ===================================================================

#[async_trait::async_trait]
pub trait Consensus: Send + Sync {
    /// Konsensüsü başlat
    async fn start(&self) -> PacyteResult<()>;
    
    /// Konsensüsü durdur
    async fn stop(&self) -> PacyteResult<()>;
    
    /// Komut gönder
    async fn send_command(&self, cmd: ConsensusCommand) -> PacyteResult<()>;
    
    /// Event kanalını al
    fn subscribe_events(&self) -> mpsc::UnboundedReceiver<ConsensusEvent>;
    
    /// Mevcut durumu getir
    fn state(&self) -> ConsensusState;
    
    /// Mevcut yüksekliği getir
    fn current_height(&self) -> BlockHeight;
    
    /// Mevcut round'u getir
    fn current_round(&self) -> u64;
    
    /// Validator mı?
    fn is_validator(&self) -> bool;
    
    /// Proposer mı?
    fn is_proposer(&self, height: BlockHeight, round: u64) -> bool;
}

// ===================================================================
// PROPOSAL
// ===================================================================

#[derive(Debug, Clone)]
pub struct Proposal {
    pub height: BlockHeight,
    pub round: u64,
    pub block: Block,
    pub proposer: u64,
    pub signature: Signature,
    pub timestamp: Timestamp,
}

impl Proposal {
    pub fn new(height: BlockHeight, round: u64, block: Block, proposer: u64) -> Self {
        Self {
            height,
            round,
            block,
            proposer,
            signature: Vec::new(),
            timestamp: crate::types::current_timestamp(),
        }
    }
    
    pub fn hash(&self) -> Hash {
        self.block.hash()
    }
    
    pub fn sign(&mut self, signature: Signature) {
        self.signature = signature;
    }
    
    pub fn signing_hash(&self) -> Hash {
        use sha3::{Digest, Sha3_256};
        
        let mut hasher = Sha3_256::new();
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.round.to_le_bytes());
        hasher.update(&self.block.hash());
        hasher.update(&self.proposer.to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.finalize().into()
    }
}

// ===================================================================
// VOTE
// ===================================================================

#[derive(Debug, Clone)]
pub struct Vote {
    pub height: BlockHeight,
    pub round: u64,
    pub block_hash: Hash,
    pub voter: u64,
    pub vote_type: VoteType,
    pub signature: Signature,
    pub timestamp: Timestamp,
}

impl Vote {
    pub fn new(height: BlockHeight, round: u64, block_hash: Hash, voter: u64, vote_type: VoteType) -> Self {
        Self {
            height,
            round,
            block_hash,
            voter,
            vote_type,
            signature: Vec::new(),
            timestamp: crate::types::current_timestamp(),
        }
    }
    
    pub fn sign(&mut self, signature: Signature) {
        self.signature = signature;
    }
    
    pub fn signing_hash(&self) -> Hash {
        use sha3::{Digest, Sha3_256};
        
        let mut hasher = Sha3_256::new();
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&self.round.to_le_bytes());
        hasher.update(&self.block_hash);
        hasher.update(&self.voter.to_le_bytes());
        hasher.update(&(self.vote_type as u8).to_le_bytes());
        hasher.update(&self.timestamp.to_le_bytes());
        hasher.finalize().into()
    }
}

// ===================================================================
// VOTE TYPE
// ===================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoteType {
    Prevote = 1,
    Precommit = 2,
}

// ===================================================================
// VALIDATOR SET
// ===================================================================

#[derive(Debug, Clone)]
pub struct ValidatorSet {
    pub validators: Vec<ValidatorInfo>,
    pub total_stake: u128,
    pub epoch: u64,
}

#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    pub id: u64,
    pub address: Address,
    pub public_key: Vec<u8>,
    pub stake: u128,
    pub voting_power: u64,
    pub is_active: bool,
}

impl ValidatorSet {
    pub fn new() -> Self {
        Self {
            validators: Vec::new(),
            total_stake: 0,
            epoch: 0,
        }
    }
    
    pub fn add_validator(&mut self, info: ValidatorInfo) {
        self.total_stake += info.stake;
        self.validators.push(info);
    }
    
    pub fn get_validator(&self, id: u64) -> Option<&ValidatorInfo> {
        self.validators.iter().find(|v| v.id == id)
    }
    
    pub fn get_proposer(&self, height: BlockHeight, round: u64) -> Option<&ValidatorInfo> {
        if self.validators.is_empty() {
            return None;
        }
        
        let index = (height as usize + round as usize) % self.validators.len();
        self.validators.get(index)
    }
    
    pub fn is_proposer(&self, validator_id: u64, height: BlockHeight, round: u64) -> bool {
        self.get_proposer(height, round)
            .map(|v| v.id == validator_id)
            .unwrap_or(false)
    }
    
    pub fn total_voting_power(&self) -> u64 {
        self.validators.iter().map(|v| v.voting_power).sum()
    }
    
    pub fn quorum_voting_power(&self) -> u64 {
        (self.total_voting_power() * 2 / 3) + 1
    }
    
    pub fn active_validators(&self) -> Vec<&ValidatorInfo> {
        self.validators.iter().filter(|v| v.is_active).collect()
    }
    
    pub fn active_count(&self) -> usize {
        self.validators.iter().filter(|v| v.is_active).count()
    }
}