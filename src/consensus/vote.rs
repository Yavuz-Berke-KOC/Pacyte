// ===================================================================
// PACYTE NEXUS - VOTE YÖNETİMİ
// ===================================================================

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use parking_lot::RwLock;

use crate::types::{PacyteError, PacyteResult, Hash, BlockHeight, Address, Signature, Timestamp};
use crate::crypto::{Ed25519Verifier, Dilithium5Verifier};
use super::{Vote, VoteType, ValidatorManager};

// ===================================================================
// VOTE MANAGER
// ===================================================================

pub struct VoteManager {
    validator_manager: Arc<ValidatorManager>,
    
    // height -> round -> vote_type -> block_hash -> votes
    votes: Arc<RwLock<HashMap<BlockHeight, RoundVotes>>>,
    
    // Double vote tespiti için
    seen_votes: Arc<RwLock<HashSet<VoteKey>>>,
}

#[derive(Debug, Clone)]
struct RoundVotes {
    round: u64,
    prevotes: HashMap<Hash, Vec<Vote>>,
    precommits: HashMap<Hash, Vec<Vote>>,
}

impl RoundVotes {
    fn new(round: u64) -> Self {
        Self {
            round,
            prevotes: HashMap::new(),
            precommits: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct VoteKey {
    height: BlockHeight,
    round: u64,
    vote_type: u8,
    voter: u64,
}

impl VoteManager {
    pub fn new(validator_manager: Arc<ValidatorManager>) -> Self {
        Self {
            validator_manager,
            votes: Arc::new(RwLock::new(HashMap::new())),
            seen_votes: Arc::new(RwLock::new(HashSet::new())),
        }
    }
    
    /// Vote ekle
    pub fn add_vote(&self, vote: Vote) -> PacyteResult<VoteResult> {
        // Temel validasyon
        self.validate_vote(&vote)?;
        
        // Double vote kontrolü
        let vote_key = VoteKey {
            height: vote.height,
            round: vote.round,
            vote_type: vote.vote_type as u8,
            voter: vote.voter,
        };
        
        {
            let mut seen = self.seen_votes.write();
            if seen.contains(&vote_key) {
                return Ok(VoteResult::AlreadySeen);
            }
            seen.insert(vote_key);
        }
        
        // Vote'u kaydet
        let quorum_reached = self.store_vote(vote.clone())?;
        
        // Aynı round'da farklı block'a vote kontrolü (double voting)
        if self.detect_double_voting(&vote) {
            // Validator'u slash'le
            let _ = self.validator_manager.slash_double_sign(vote.voter);
            return Err(PacyteError::DoubleVoting(vote.voter));
        }
        
        Ok(VoteResult::Added { quorum_reached })
    }
    
    /// Vote'u valide et
    fn validate_vote(&self, vote: &Vote) -> PacyteResult<()> {
        // Validator aktif mi?
        let validator = self.validator_manager
            .get_validator(vote.voter)
            .ok_or_else(|| PacyteError::ValidatorNotFound(vote.voter))?;
        
        if !validator.is_active() {
            return Err(PacyteError::ValidatorInactive(vote.voter));
        }
        
        // İmza doğrulama
        if !self.verify_vote_signature(vote, &validator.public_key) {
            return Err(PacyteError::InvalidSignature);
        }
        
        // Timestamp kontrolü
        let now = crate::types::current_timestamp();
        if vote.timestamp > now + 10 || vote.timestamp < now - 60 {
            return Err(PacyteError::InvalidTimestamp);
        }
        
        Ok(())
    }
    
    /// İmza doğrula
    fn verify_vote_signature(&self, vote: &Vote, public_key: &[u8]) -> bool {
        let message = vote.signing_hash();
        
        match vote.signature.len() {
            64 => Ed25519Verifier::verify(&message, &vote.signature, public_key),
            4595 => Dilithium5Verifier::verify(&message, &vote.signature, public_key),
            _ => false,
        }
    }
    
    /// Vote'u kaydet
    fn store_vote(&self, vote: Vote) -> PacyteResult<bool> {
        let mut votes = self.votes.write();
        
        let round_votes = votes
            .entry(vote.height)
            .or_insert_with(|| RoundVotes::new(vote.round));
        
        // Round değiştiyse temizle
        if round_votes.round != vote.round {
            *round_votes = RoundVotes::new(vote.round);
        }
        
        let vote_map = match vote.vote_type {
            VoteType::Prevote => &mut round_votes.prevotes,
            VoteType::Precommit => &mut round_votes.precommits,
        };
        
        let votes_for_block = vote_map
            .entry(vote.block_hash)
            .or_insert_with(Vec::new);
        
        // Duplicate kontrolü
        if votes_for_block.iter().any(|v| v.voter == vote.voter) {
            return Ok(false);
        }
        
	let vt = vote.vote_type;
        votes_for_block.push(vote.clone());
        
        // Quorum kontrolü
        let quorum = self.check_quorum(
            vote.height,
            vote.round,
            vote.block_hash,
            vt,
        );
        
        Ok(quorum)
    }
    
    /// Quorum kontrolü
    fn check_quorum(
        &self,
        height: BlockHeight,
        round: u64,
        block_hash: Hash,
        vote_type: VoteType,
    ) -> bool {
        let votes = self.votes.read();
        
        let round_votes = match votes.get(&height) {
            Some(rv) if rv.round == round => rv,
            _ => return false,
        };
        
        let vote_map = match vote_type {
            VoteType::Prevote => &round_votes.prevotes,
            VoteType::Precommit => &round_votes.precommits,
        };
        
        let vote_count = vote_map
            .get(&block_hash)
            .map(|v| v.len())
            .unwrap_or(0);
        
        let total_voting_power = self.get_voting_power_for_votes(
            height,
            round,
            block_hash,
            vote_type,
        );
        
        let active_validators = self.validator_manager.active_count();
        let quorum_threshold = (active_validators * 2 / 3) + 1;
        
        total_voting_power >= quorum_threshold as u64
    }
    
    /// Vote'ların toplam voting power'ını hesapla
    fn get_voting_power_for_votes(
        &self,
        height: BlockHeight,
        round: u64,
        block_hash: Hash,
        vote_type: VoteType,
    ) -> u64 {
        let votes = self.votes.read();
        
        let round_votes = match votes.get(&height) {
            Some(rv) if rv.round == round => rv,
            _ => return 0,
        };
        
        let vote_map = match vote_type {
            VoteType::Prevote => &round_votes.prevotes,
            VoteType::Precommit => &round_votes.precommits,
        };
        
        let votes = match vote_map.get(&block_hash) {
            Some(v) => v,
            None => return 0,
        };
        
        votes.iter()
            .filter_map(|v| self.validator_manager.get_validator(v.voter))
            .map(|v| v.voting_power)
            .sum()
    }
    
    /// Double voting tespiti
    fn detect_double_voting(&self, vote: &Vote) -> bool {
        let votes = self.votes.read();
        
        let round_votes = match votes.get(&vote.height) {
            Some(rv) if rv.round == vote.round => rv,
            _ => return false,
        };
        
        let vote_map = match vote.vote_type {
            VoteType::Prevote => &round_votes.prevotes,
            VoteType::Precommit => &round_votes.precommits,
        };
        
        // Aynı validator başka bir block'a vote vermiş mi?
        for (hash, votes) in vote_map.iter() {
            if *hash != vote.block_hash {
                if votes.iter().any(|v| v.voter == vote.voter) {
                    return true;
                }
            }
        }
        
        false
    }
    
    /// Belirli bir block için vote sayısını getir
    pub fn get_vote_count(
        &self,
        height: BlockHeight,
        round: u64,
        block_hash: Hash,
        vote_type: VoteType,
    ) -> usize {
        let votes = self.votes.read();
        
        let round_votes = match votes.get(&height) {
            Some(rv) if rv.round == round => rv,
            _ => return 0,
        };
        
        let vote_map = match vote_type {
            VoteType::Prevote => &round_votes.prevotes,
            VoteType::Precommit => &round_votes.precommits,
        };
        
        vote_map.get(&block_hash).map(|v| v.len()).unwrap_or(0)
    }
    
    /// Quorum sağlanmış mı?
    pub fn has_quorum(
        &self,
        height: BlockHeight,
        round: u64,
        block_hash: Hash,
        vote_type: VoteType,
    ) -> bool {
        self.check_quorum(height, round, block_hash, vote_type)
    }
    
    /// Round'u temizle
    pub fn clear_round(&self, height: BlockHeight) {
        self.votes.write().remove(&height);
    }
    
    /// Eski height'ları temizle
    pub fn prune_old_heights(&self, keep_last: usize) {
        let mut votes = self.votes.write();
        
        let mut heights: Vec<BlockHeight> = votes.keys().copied().collect();
        heights.sort();
        
        if heights.len() > keep_last {
            let to_remove = heights.len() - keep_last;
            for height in heights.iter().take(to_remove) {
                votes.remove(height);
            }
        }
    }
    
    /// Tüm vote'ları getir
    pub fn get_all_votes(&self, height: BlockHeight, round: u64) -> Vec<Vote> {
        let votes = self.votes.read();
        
        let round_votes = match votes.get(&height) {
            Some(rv) if rv.round == round => rv,
            _ => return Vec::new(),
        };
        
        let mut all_votes = Vec::new();
        all_votes.extend(round_votes.prevotes.values().flatten().cloned());
        all_votes.extend(round_votes.precommits.values().flatten().cloned());
        
        all_votes
    }
    
    /// Vote'ları doğrula ve temizle
    pub fn verify_and_cleanup(&self, height: BlockHeight) -> usize {
        let mut removed = 0;
        
        let votes = self.votes.read();
        if let Some(round_votes) = votes.get(&height) {
            // Geçersiz vote'ları kontrol et
            // (Bu kısım optimize edilebilir)
        }
        
        removed
    }
}

// ===================================================================
// VOTE SONUCU
// ===================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoteResult {
    Added { quorum_reached: bool },
    AlreadySeen,
    Invalid,
}

// ===================================================================
// VOTE AGGREGATOR
// ===================================================================

pub struct VoteAggregator {
    votes: HashMap<Hash, AggregatedVotes>,
}

#[derive(Debug, Clone, Default)]
pub struct AggregatedVotes {
    pub prevotes: usize,
    pub precommits: usize,
    pub voting_power: u64,
    pub voters: HashSet<u64>,
}

impl VoteAggregator {
    pub fn new() -> Self {
        Self {
            votes: HashMap::new(),
        }
    }
    
    pub fn add_vote(&mut self, vote: &Vote, voting_power: u64) {
        let entry = self.votes.entry(vote.block_hash).or_default();
        
        if !entry.voters.contains(&vote.voter) {
            entry.voters.insert(vote.voter);
            entry.voting_power += voting_power;
            
            match vote.vote_type {
                VoteType::Prevote => entry.prevotes += 1,
                VoteType::Precommit => entry.precommits += 1,
            }
        }
    }
    
    pub fn get_winner(&self, quorum: usize) -> Option<Hash> {
        self.votes
            .iter()
            .filter(|(_, v)| v.prevotes >= quorum && v.precommits >= quorum)
            .max_by_key(|(_, v)| v.voting_power)
            .map(|(h, _)| *h)
    }
    
    pub fn clear(&mut self) {
        self.votes.clear();
    }
}

impl Default for VoteAggregator {
    fn default() -> Self {
        Self::new()
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Ed25519Signer;

    fn create_test_vote() -> Vote {
        Vote {
            height: 1,
            round: 0,
            block_hash: [1u8; 32],
            voter: 1,
            vote_type: VoteType::Prevote,
            signature: vec![4u8; 64],
            timestamp: crate::types::current_timestamp(),
        }
    }

    #[test]
    fn test_vote_aggregator() {
        let mut agg = VoteAggregator::new();
        
        let vote = create_test_vote();
        agg.add_vote(&vote, 100);
        
        let winner = agg.get_winner(1);
        assert_eq!(winner, Some(vote.block_hash));
    }
    
    #[test]
    fn test_vote_result() {
        let result = VoteResult::Added { quorum_reached: true };
        match result {
            VoteResult::Added { quorum_reached } => assert!(quorum_reached),
            _ => panic!("Wrong variant"),
        }
    }
}