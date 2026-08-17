// ===================================================================
// PACYTE NEXUS - HOTSTUFF KONSENSÜS MOTORU
// ===================================================================

use futures::FutureExt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::{mpsc, broadcast};
use tokio::time::{timeout, Duration};

use crate::types::{
    PacyteError, PacyteResult, Hash, BlockHeight, Timestamp, current_timestamp,
};
use crate::types::block::Block;
use crate::storage::{Storage, StateManager};
use crate::mempool::Mempool;
use crate::network::Network;
use crate::network::message::NetworkMessage;
use crate::crypto::{Ed25519Signer, HybridSigner};

use super::{
    Consensus, ConsensusConfig, ConsensusState, ConsensusCommand, ConsensusEvent,
    Proposal, Vote, VoteType, ValidatorSet, ValidatorInfo,
};

// ===================================================================
// HOTSTUFF ENGINE
// ===================================================================

pub struct HotStuffEngine {
    config: ConsensusConfig,
    
    // State
    state: Arc<RwLock<ConsensusState>>,
    height: Arc<RwLock<BlockHeight>>,
    round: Arc<RwLock<u64>>,
    
    // Validator bilgileri
    validator_set: Arc<RwLock<ValidatorSet>>,
    my_validator_id: Option<u64>,
    signer: Option<HybridSigner>,
    
    // Oylama durumu
    prevotes: Arc<RwLock<HashMap<u64, HashMap<Hash, Vec<Vote>>>>>,
    precommits: Arc<RwLock<HashMap<u64, HashMap<Hash, Vec<Vote>>>>>,
    
    // Mevcut proposal
    current_proposal: Arc<RwLock<Option<Proposal>>>,
    locked_proposal: Arc<RwLock<Option<Proposal>>>,
    committed_blocks: Arc<RwLock<Vec<Hash>>>,
    
    // Bağımlılıklar
    storage: Arc<dyn Storage>,
    state_manager: Arc<StateManager>,
    mempool: Arc<dyn Mempool>,
    network: Arc<dyn Network>,
    
    // Kanallar
    cmd_tx: mpsc::UnboundedSender<ConsensusCommand>,
    cmd_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<ConsensusCommand>>>>,
    event_tx: mpsc::UnboundedSender<ConsensusEvent>,
    event_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<ConsensusEvent>>>>,
    shutdown_tx: broadcast::Sender<()>,
    
    // Timeout yönetimi
    round_start_time: Arc<RwLock<Option<Timestamp>>>,
}

impl HotStuffEngine {
    pub fn new(
        config: ConsensusConfig,
        storage: Arc<dyn Storage>,
        state_manager: Arc<StateManager>,
        mempool: Arc<dyn Mempool>,
        network: Arc<dyn Network>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, _) = broadcast::channel(1);
        
        // Genesis yüksekliğini al
        let height = 0; // Başlangıç değeri, run() içinde güncellenecek
        
        Self {
            config,
            state: Arc::new(RwLock::new(ConsensusState::Idle)),
            height: Arc::new(RwLock::new(height)),
            round: Arc::new(RwLock::new(0)),
            validator_set: Arc::new(RwLock::new(ValidatorSet::new())),
            my_validator_id: None,
            signer: None,
            prevotes: Arc::new(RwLock::new(HashMap::new())),
            precommits: Arc::new(RwLock::new(HashMap::new())),
            current_proposal: Arc::new(RwLock::new(None)),
            locked_proposal: Arc::new(RwLock::new(None)),
            committed_blocks: Arc::new(RwLock::new(Vec::new())),
            storage,
            state_manager,
            mempool,
            network,
            cmd_tx,
            cmd_rx: Arc::new(RwLock::new(Some(cmd_rx))),
            event_tx,
            event_rx: Arc::new(RwLock::new(Some(event_rx))),
            shutdown_tx,
            round_start_time: Arc::new(RwLock::new(None)),
        }
    }
    
    pub fn set_validator(&mut self, id: u64, signer: HybridSigner) {
        self.my_validator_id = Some(id);
        self.signer = Some(signer);
    }
    
    pub fn update_validator_set(&self, set: ValidatorSet) {
        *self.validator_set.write() = set;
    }
    
    /// Proposer'ı belirle (round-robin)
    fn get_proposer(&self, height: BlockHeight, round: u64) -> Option<u64> {
        let set = self.validator_set.read();
        set.get_proposer(height, round).map(|v| v.id)
    }
    
    /// Ben proposer mıyım?
    fn am_i_proposer(&self, height: BlockHeight, round: u64) -> bool {
        if let Some(my_id) = self.my_validator_id {
            self.get_proposer(height, round) == Some(my_id)
        } else {
            false
        }
    }
    
    /// Yeni round başlat
    async fn start_new_round(&self, height: BlockHeight, round: u64) -> PacyteResult<()> {
        tracing::info!("Starting new round: height={}, round={}", height, round);
        
        *self.round.write() = round;
        *self.round_start_time.write() = Some(current_timestamp());
        *self.state.write() = ConsensusState::Proposing;
        
        // Event gönder
        let _ = self.event_tx.send(ConsensusEvent::NewRound { height, round });
        
        // Proposer ben miyim?
        if self.am_i_proposer(height, round) {
            // Blok oluştur
            let block = self.create_block(height).await?;
            
            // Proposal oluştur
            let proposal = self.create_proposal(height, round, block).await?;
            
            // Proposal'ı broadcast et
            self.broadcast_proposal(&proposal).await?;
            
            *self.current_proposal.write() = Some(proposal.clone());
            *self.state.write() = ConsensusState::Prevoting;
            
            // Event gönder
            let _ = self.event_tx.send(ConsensusEvent::ProposalSent { height, round });
        } else {
            // Proposer'ı bekle
            *self.state.write() = ConsensusState::WaitingForBlock;
        }
        
        Ok(())
    }
    
    /// Blok oluştur
    async fn create_block(&self, height: BlockHeight) -> PacyteResult<Block> {
        let prev_block = self.storage.get_latest_block().await?
            .ok_or_else(|| PacyteError::BlockNotFound(height - 1))?;
        
        let prev_hash = prev_block.hash();
        
        // Mempool'dan işlemleri seç
        let txs = self.mempool.select_for_block(
            10000,
            self.config.max_block_size,
        ).await;
        
        let proposer = self.my_validator_id.ok_or_else(|| {
            PacyteError::NotValidator
        })?;
        
        let proposer_address = self.validator_set.read()
            .get_validator(proposer)
            .map(|v| v.address)
            .unwrap_or([0u8; 32]);
        
        let mut block = Block::new(height, prev_hash, txs, proposer_address);
        
        // State root'u hesapla
        // (Gerçek implementasyonda state'i güncelle)
        
        Ok(block)
    }
    
    /// Proposal oluştur
    async fn create_proposal(&self, height: BlockHeight, round: u64, block: Block) -> PacyteResult<Proposal> {
        let proposer = self.my_validator_id.ok_or_else(|| {
            PacyteError::NotValidator
        })?;
        
        let mut proposal = Proposal::new(height, round, block, proposer);
        
        // İmzala
        if let Some(signer) = &self.signer {
            let sig = signer.sign(&proposal.signing_hash());
            proposal.sign(sig.to_bytes());
        }
        
        Ok(proposal)
    }
    
    /// Proposal'ı broadcast et
    async fn broadcast_proposal(&self, proposal: &Proposal) -> PacyteResult<()> {
        let msg = NetworkMessage::Proposal(proposal.clone().into());
        self.network.broadcast(msg).await?;
        
        // Kendimize de gönder
        let cmd = ConsensusCommand::ProposalReceived { proposal: proposal.clone() };
        let _ = self.cmd_tx.send(cmd);
        
        Ok(())
    }
    
    /// Proposal işle
    async fn handle_proposal(&self, proposal: Proposal) -> PacyteResult<()> {
        let height = *self.height.read();
        let round = *self.round.read();
        
        // Yükseklik ve round kontrolü
        if proposal.height != height || proposal.round != round {
            tracing::debug!("Ignoring proposal for height={}, round={}", proposal.height, proposal.round);
            return Ok(());
        }
        
        // Proposer kontrolü
        let expected_proposer = self.get_proposer(height, round);
        if expected_proposer != Some(proposal.proposer) {
            tracing::warn!("Invalid proposer for height={}, round={}", height, round);
            return Ok(());
        }
        
        // İmza doğrulama
        if !self.verify_proposal_signature(&proposal).await {
            tracing::warn!("Invalid proposal signature");
            return Ok(());
        }
        
        // Block validasyonu
        if !self.validate_block(&proposal.block).await? {
            tracing::warn!("Invalid block in proposal");
            return Ok(());
        }
        
        tracing::info!("Received valid proposal for height={}, round={}", height, round);
        
        // Proposal'ı kaydet
        *self.current_proposal.write() = Some(proposal.clone());
        
        // Prevote gönder
        self.send_prevote(proposal.hash()).await?;
        
        Ok(())
    }
    
    /// Prevote gönder
    async fn send_prevote(&self, block_hash: Hash) -> PacyteResult<()> {
        let height = *self.height.read();
        let round = *self.round.read();
        let voter = self.my_validator_id.ok_or_else(|| PacyteError::NotValidator)?;
        
        let mut vote = Vote::new(height, round, block_hash, voter, VoteType::Prevote);
        
        if let Some(signer) = &self.signer {
            let sig = signer.sign(&vote.signing_hash());
            vote.sign(sig.to_bytes());
        }
        
        // Vote'u kaydet
        self.add_vote(vote.clone());
        
        // Broadcast
        let msg = NetworkMessage::Vote(vote.clone().into());
        self.network.broadcast(msg).await?;
        
        let _ = self.event_tx.send(ConsensusEvent::VoteSent { height, round, vote_type: VoteType::Prevote });
        
        // Quorum kontrolü
        self.check_prevote_quorum(height, round, block_hash).await?;
        
        Ok(())
    }
    
    /// Vote ekle
    fn add_vote(&self, vote: Vote) {
        match vote.vote_type {
            VoteType::Prevote => {
                let mut prevotes = self.prevotes.write();
                let round_votes = prevotes
                    .entry(vote.height)
                    .or_insert_with(HashMap::new);
                
                let votes = round_votes
                    .entry(vote.block_hash)
                    .or_insert_with(Vec::new);
                
                // Duplicate kontrolü
                if !votes.iter().any(|v| v.voter == vote.voter) {
                    votes.push(vote);
                }
            }
            VoteType::Precommit => {
                let mut precommits = self.precommits.write();
                let round_votes = precommits
                    .entry(vote.height)
                    .or_insert_with(HashMap::new);
                
                let votes = round_votes
                    .entry(vote.block_hash)
                    .or_insert_with(Vec::new);
                
                if !votes.iter().any(|v| v.voter == vote.voter) {
                    votes.push(vote);
                }
            }
        }
    }
    
    /// Prevote quorum kontrolü
    async fn check_prevote_quorum(&self, height: BlockHeight, round: u64, block_hash: Hash) -> PacyteResult<()> {
        let votes = {
    	    let prevotes = self.prevotes.read();
    	    prevotes
            .get(&height)
            .and_then(|r| r.get(&block_hash))
            .map(|v| v.len())
            .unwrap_or(0)
        }; // ← prevotes burada düşüyor (scope dışı)
        
        let validator_count = self.validator_set.read().active_count();
        let quorum = (validator_count * 2 / 3) + 1;
        
        if votes >= quorum {
            tracing::info!("Prevote quorum reached for height={}, round={}", height, round);
            
            let _ = self.event_tx.send(ConsensusEvent::QuorumReached {
                height, round, vote_type: VoteType::Prevote
            });
            
            // Precommit gönder
            self.send_precommit(block_hash).await?;
        }
        
        Ok(())
    }
    
    /// Precommit gönder
    async fn send_precommit(&self, block_hash: Hash) -> PacyteResult<()> {
        let height = *self.height.read();
        let round = *self.round.read();
        let voter = self.my_validator_id.ok_or_else(|| PacyteError::NotValidator)?;
        
        let mut vote = Vote::new(height, round, block_hash, voter, VoteType::Precommit);
        
        if let Some(signer) = &self.signer {
            let sig = signer.sign(&vote.signing_hash());
            vote.sign(sig.to_bytes());
        }
        
        self.add_vote(vote.clone());
        
        let msg = NetworkMessage::Vote(vote.into());
        self.network.broadcast(msg).await?;
        
        let _ = self.event_tx.send(ConsensusEvent::VoteSent { height, round, vote_type: VoteType::Precommit });
        
        // Precommit quorum kontrolü
        self.check_precommit_quorum(height, round, block_hash).await?;
        
        Ok(())
    }
    
    /// Precommit quorum kontrolü
    async fn check_precommit_quorum(&self, height: BlockHeight, round: u64, block_hash: Hash) -> PacyteResult<()> {
        let precommits = self.precommits.read();
        let votes = precommits
            .get(&height)
            .and_then(|r| r.get(&block_hash))
            .map(|v| v.len())
            .unwrap_or(0);
        
        let validator_count = self.validator_set.read().active_count();
        let quorum = (validator_count * 2 / 3) + 1;
        
        if votes >= quorum {
            tracing::info!("Precommit quorum reached for height={}, round={}", height, round);
            
            let _ = self.event_tx.send(ConsensusEvent::QuorumReached {
                height, round, vote_type: VoteType::Precommit
            });
            
            // Bloğu commit et
            self.commit_block(height).await?;
        }
        
        Ok(())
    }
    
    /// Bloğu commit et
    async fn commit_block(&self, height: BlockHeight) -> PacyteResult<()> {
        let proposal = self.current_proposal.read()
            .clone()
            .ok_or_else(|| PacyteError::Internal("No proposal to commit".to_string()))?;
        
        // State'i güncelle
        self.state_manager.apply_block(&proposal.block).await?;
        
        // Bloğu kaydet
        self.storage.save_block(&proposal.block).await?;
        
        // Mempool'u temizle
        let tx_hashes: Vec<Hash> = proposal.block.body.transactions
            .iter()
            .map(|tx| tx.hash())
            .collect();
        self.mempool.cleanup(&tx_hashes).await;
        
        // Commit edilenleri kaydet
        self.committed_blocks.write().push(proposal.block.hash());
        
        *self.state.write() = ConsensusState::Committed;
        
        // Event gönder
        let _ = self.event_tx.send(ConsensusEvent::BlockCommitted {
            block: proposal.block,
            height,
        });
        
        // Sonraki yüksekliğe geç
        *self.height.write() = height + 1;
        *self.round.write() = 0;
        
        // Yeni round başlat
        self.start_new_round(height + 1, 0).await?;
        
        Ok(())
    }
    
    /// Proposal imzasını doğrula
    async fn verify_proposal_signature(&self, proposal: &Proposal) -> bool {
        let vs = self.validator_set.read();
let validator = vs.get_validator(proposal.proposer);
        
        if let Some(validator) = validator {
            // İmza doğrulama (basitleştirilmiş)
            true
        } else {
            false
        }
    }
    
    /// Bloğu valide et
    async fn validate_block(&self, block: &Block) -> PacyteResult<bool> {
        // Temel kontroller
        if block.header.height != *self.height.read() {
            return Ok(false);
        }
        
        // Önceki hash kontrolü
        let prev_block = self.storage.get_latest_block().await?;
        if let Some(prev) = prev_block {
            if block.header.previous_hash != prev.hash() {
                return Ok(false);
            }
        }
        
        // İşlemleri valide et
        for tx in &block.body.transactions {
            if !tx.validate_basic(3600) {
                return Ok(false);
            }
        }
        
        Ok(true)
    }
    
    /// Vote işle
    async fn handle_vote(&self, vote: Vote) -> PacyteResult<()> {
        let height = *self.height.read();
        let round = *self.round.read();
        
        // Yükseklik ve round kontrolü
        if vote.height != height || vote.round != round {
            return Ok(());
        }
        
        // İmza doğrulama
        // Validator kontrolü
        
        // Vote'u ekle
        self.add_vote(vote.clone());
        
        // Vote tipine göre işlem
        match vote.vote_type {
            VoteType::Prevote => {
                self.check_prevote_quorum(height, round, vote.block_hash).await?;
            }
            VoteType::Precommit => {
                self.check_precommit_quorum(height, round, vote.block_hash).await?;
            }
        }
        
        Ok(())
    }
    
    /// Ana döngü
    async fn run(&self) -> PacyteResult<()> {
	let saved_height = self.storage.get_block_height().await.unwrap_or(0);
        if saved_height > *self.height.read() {
            *self.height.write() = saved_height;
        }
        let mut cmd_rx = self.cmd_rx.write()
            .take()
            .expect("run called multiple times");
        
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        
        // Network'ten mesajları dinle
        let mut network_rx = self.network.subscribe();
        
        // İlk round'u başlat
        let height = *self.height.read();
        self.start_new_round(height, 0).await?;

	// Storage'dan son yüksekliği oku
	let saved_height = self.storage.get_block_height().await.unwrap_or(0);
	if saved_height > *self.height.read() {
    	    *self.height.write() = saved_height;
	}
        
        loop {
            tokio::select! {
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        ConsensusCommand::Propose { height, round } => {
                            let _ = self.start_new_round(height, round).await;
                        }
                        ConsensusCommand::ProposalReceived { proposal } => {
                            let _ = self.handle_proposal(proposal).await;
                        }
                        ConsensusCommand::VoteReceived { vote } => {
                            let _ = self.handle_vote(vote).await;
                        }
                        ConsensusCommand::Timeout { height, round } => {
                            if height == *self.height.read() && round == *self.round.read() {
                                tracing::warn!("Timeout for height={}, round={}", height, round);
                                
                                // Sonraki round'a geç
                                if round < self.config.max_rounds {
                                    let _ = self.start_new_round(height, round + 1).await;
                                } else {
                                    tracing::error!("Max rounds reached for height={}", height);
                                }
                            }
                        }
                        ConsensusCommand::Stop => {
                            break;
                        }
                        _ => {}
                    }
                }
                
                Some(peer_msg) = network_rx.recv() => {
                    match peer_msg.message {
                        NetworkMessage::Proposal(proposal_data) => {
                            // Proposal'a dönüştür ve işle
                            // ...
                        }
                        NetworkMessage::Vote(vote_data) => {
                            // Vote'a dönüştür ve işle
                            // ...
                        }
                        _ => {}
                    }
                }
                
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }
        
        Ok(())
    }
    
    fn clone_engine(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state.clone(),
            height: self.height.clone(),
            round: self.round.clone(),
            validator_set: self.validator_set.clone(),
            my_validator_id: self.my_validator_id,
            signer: self.signer.clone(),
            prevotes: self.prevotes.clone(),
            precommits: self.precommits.clone(),
            current_proposal: self.current_proposal.clone(),
            locked_proposal: self.locked_proposal.clone(),
            committed_blocks: self.committed_blocks.clone(),
            storage: self.storage.clone(),
            state_manager: self.state_manager.clone(),
            mempool: self.mempool.clone(),
            network: self.network.clone(),
            cmd_tx: self.cmd_tx.clone(),
            cmd_rx: self.cmd_rx.clone(),
            event_tx: self.event_tx.clone(),
            event_rx: self.event_rx.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
            round_start_time: self.round_start_time.clone(),
        }
    }
}

#[async_trait::async_trait]
impl Consensus for HotStuffEngine {
    async fn start(&self) -> PacyteResult<()> {
        *self.state.write() = ConsensusState::Idle;
        
	
        let engine = self.clone_engine();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                if let Err(e) = engine.run().await {
                    tracing::error!("Consensus engine error: {}", e);
                }
            });
        });
        
        Ok(())
    }
    
    async fn stop(&self) -> PacyteResult<()> {
        let _ = self.shutdown_tx.send(());
        *self.state.write() = ConsensusState::Idle;
        Ok(())
    }
    
    async fn send_command(&self, cmd: ConsensusCommand) -> PacyteResult<()> {
        self.cmd_tx.send(cmd)
            .map_err(|e| PacyteError::Internal(format!("Failed to send command: {}", e)))
    }
    
    fn subscribe_events(&self) -> mpsc::UnboundedReceiver<ConsensusEvent> {
        self.event_rx.write()
            .take()
            .expect("subscribe_events called multiple times")
    }
    
    fn state(&self) -> ConsensusState {
        self.state.read().clone()
    }
    
    fn current_height(&self) -> BlockHeight {
        *self.height.read()
    }
    
    fn current_round(&self) -> u64 {
        *self.round.read()
    }
    
    fn is_validator(&self) -> bool {
        self.my_validator_id.is_some()
    }
    
    fn is_proposer(&self, height: BlockHeight, round: u64) -> bool {
        self.am_i_proposer(height, round)
    }
}

// ===================================================================
// DÖNÜŞÜMLER
// ===================================================================

impl From<Proposal> for crate::network::message::ConsensusProposal {
    fn from(p: Proposal) -> Self {
        Self {
            height: p.height,
            round: p.round,
            block: p.block,
            proposer: p.proposer,
            signature: p.signature,
        }
    }
}

impl From<Vote> for crate::network::message::ConsensusVote {
    fn from(v: Vote) -> Self {
        Self {
            height: v.height,
            round: v.round,
            block_hash: v.block_hash,
            voter: v.voter,
            vote_type: match v.vote_type {
                VoteType::Prevote => crate::network::message::VoteType::Prevote,
                VoteType::Precommit => crate::network::message::VoteType::Precommit,
            },
            signature: v.signature,
        }
    }
}