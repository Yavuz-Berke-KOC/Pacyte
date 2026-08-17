// ===================================================================
// PACYTE NEXUS - KONSENSÜS GÜVENLİK KURALLARI
// ===================================================================

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use parking_lot::RwLock;

use crate::types::{PacyteError, PacyteResult, Hash, BlockHeight};
use crate::types::block::Block;
use super::{Proposal, Vote, VoteType, ValidatorManager};

// ===================================================================
// SAFETY RULES (HotStuff)
// ===================================================================

pub struct SafetyRules {
    validator_manager: Arc<ValidatorManager>,
    
    // Locked proposal (en son precommit edilen)
    locked_proposal: Arc<RwLock<Option<LockedProposal>>>,
    
    // Commit edilmiş block'lar
    committed_blocks: Arc<RwLock<HashSet<Hash>>>,
    
    // Görülen proposal'lar (double proposal tespiti için)
    seen_proposals: Arc<RwLock<HashSet<(BlockHeight, u64, u64)>>>,
}

#[derive(Debug, Clone)]
struct LockedProposal {
    height: BlockHeight,
    round: u64,
    block_hash: Hash,
    locked_at: u64,
}

impl SafetyRules {
    pub fn new(validator_manager: Arc<ValidatorManager>) -> Self {
        Self {
            validator_manager,
            locked_proposal: Arc::new(RwLock::new(None)),
            committed_blocks: Arc::new(RwLock::new(HashSet::new())),
            seen_proposals: Arc::new(RwLock::new(HashSet::new())),
        }
    }
    
    /// Proposal güvenlik kontrolü
    pub fn check_proposal_safety(&self, proposal: &Proposal) -> PacyteResult<()> {
        // 1. Proposer kontrolü
        let expected_proposer = self.validator_manager
            .get_proposer(proposal.height, proposal.round);
        
        if expected_proposer != Some(proposal.proposer) {
            return Err(PacyteError::InvalidProposer {
                expected: expected_proposer,
                got: proposal.proposer,
            });
        }
        
        // 2. Double proposal kontrolü
        let key = (proposal.height, proposal.round, proposal.proposer);
        {
            let mut seen = self.seen_proposals.write();
            if seen.contains(&key) {
                // Double proposal tespiti - slash
                let _ = self.validator_manager.slash(
                    proposal.proposer,
                    self.validator_manager
                        .get_validator(proposal.proposer)
                        .map(|v| v.stake / 20)
                        .unwrap_or(0),
                    "Double proposal",
                );
                return Err(PacyteError::DoubleProposal(proposal.proposer));
            }
            seen.insert(key);
        }
        
        // 3. Locked proposal kontrolü (HotStuff safety rule)
        if let Some(locked) = self.locked_proposal.read().as_ref() {
            // Aynı height için locked varsa
            if locked.height == proposal.height {
                // Sadece aynı block'u propose edebilir
                if locked.block_hash != proposal.block.hash() {
                    return Err(PacyteError::SafetyViolation {
                        reason: format!(
                            "Cannot propose different block at height {}: locked={:?}",
                            proposal.height, locked.block_hash
                        ),
                    });
                }
            }
            // Daha yüksek height için locked varsa, propose edilemez
            else if locked.height > proposal.height {
                return Err(PacyteError::SafetyViolation {
                    reason: format!(
                        "Locked at height {} > proposal height {}",
                        locked.height, proposal.height
                    ),
                });
            }
        }
        
        Ok(())
    }
    
    /// Prevote güvenlik kontrolü
    pub fn check_prevote_safety(&self, vote: &Vote, locked: Option<&LockedProposal>) -> PacyteResult<()> {
        // Eğer locked proposal varsa, sadece onu prevote edebilir
        if let Some(locked) = locked {
            if locked.height == vote.height && locked.block_hash != vote.block_hash {
                return Err(PacyteError::SafetyViolation {
                    reason: format!(
                        "Cannot prevote for {:?}: locked on {:?}",
                        vote.block_hash, locked.block_hash
                    ),
                });
            }
        }
        
        Ok(())
    }
    
    /// Precommit güvenlik kontrolü
    pub fn check_precommit_safety(
        &self,
        vote: &Vote,
        prevote_quorum_hash: Option<Hash>,
    ) -> PacyteResult<()> {
        // Sadece prevote quorum'u sağlanmış block'u precommit edebilir
        if let Some(quorum_hash) = prevote_quorum_hash {
            if quorum_hash != vote.block_hash {
                return Err(PacyteError::SafetyViolation {
                    reason: format!(
                        "Can only precommit block with prevote quorum: {:?} != {:?}",
                        quorum_hash, vote.block_hash
                    ),
                });
            }
        }
        
        Ok(())
    }
    
    /// Proposal'ı lockla (precommit quorum'u sağlandığında)
    pub fn lock_proposal(&self, height: BlockHeight, round: u64, block_hash: Hash) {
        let mut locked = self.locked_proposal.write();
        
        *locked = Some(LockedProposal {
            height,
            round,
            block_hash,
            locked_at: crate::types::current_timestamp(),
        });
        
        tracing::info!("Locked proposal at height={}, round={}, hash={:?}", height, round, block_hash);
    }
    
    /// Proposal'ı unlock et (commit edildiğinde)
    pub fn unlock(&self) {
        *self.locked_proposal.write() = None;
        tracing::debug!("Unlocked proposal");
    }
    
    /// Bloğu commit et
    pub fn commit_block(&self, block: &Block) -> PacyteResult<()> {
        let hash = block.hash();
        
        // Commit edilmiş mi kontrolü
        {
            let committed = self.committed_blocks.read();
            if committed.contains(&hash) {
                return Err(PacyteError::BlockAlreadyCommitted(block.header.height));
            }
        }
        
        // Safety: Daha önce commit edilmiş bir block'un atası mı?
        // (Gerçek implementasyonda kontrol edilmeli)
        
        // Commit et
        {
            let mut committed = self.committed_blocks.write();
            committed.insert(hash);
        }
        
        // Unlock
        self.unlock();
        
        tracing::info!("Block committed at height={}, hash={:?}", block.header.height, hash);
        
        Ok(())
    }
    
    /// Double voting kontrolü
    pub fn check_double_vote(&self, vote1: &Vote, vote2: &Vote) -> bool {
        vote1.height == vote2.height &&
        vote1.round == vote2.round &&
        vote1.voter == vote2.voter &&
        vote1.vote_type == vote2.vote_type &&
        vote1.block_hash != vote2.block_hash
    }
    
    /// Fork kontrolü
    pub fn check_fork(&self, block: &Block, parent: &Block) -> bool {
        // Aynı height'da iki farklı block varsa fork vardır
        block.header.height == parent.header.height &&
        block.hash() != parent.hash()
    }
    
    /// Nihai commit kontrolü
    pub fn is_finalized(&self, height: BlockHeight) -> bool {
        // 2/3+ precommit varsa finalized
        // (Basitleştirilmiş - gerçek implementasyonda daha karmaşık)
        false
    }
    
    /// Güvenlik ihlali durumunda slash
    pub fn report_equivocation(&self, validator_id: u64, evidence: EquivocationEvidence) -> PacyteResult<()> {
        match evidence {
            EquivocationEvidence::DoubleProposal { height, round, block1, block2 } => {
                tracing::error!(
                    "Double proposal detected: validator={}, height={}, round={}",
                    validator_id, height, round
                );
                self.validator_manager.slash_double_sign(validator_id)
            }
            EquivocationEvidence::DoubleVote { vote1, vote2 } => {
                tracing::error!(
                    "Double vote detected: validator={}, height={}, round={}, type={:?}",
                    validator_id, vote1.height, vote1.round, vote1.vote_type
                );
                self.validator_manager.slash_double_sign(validator_id)
            }
        }
    }
    
    /// Mevcut locked proposal'ı getir
    pub fn locked(&self) -> Option<LockedProposal> {
        self.locked_proposal.read().clone()
    }
    
    /// Commit edilmiş block'ları getir
    pub fn committed_heights(&self) -> Vec<BlockHeight> {
        // Basitleştirilmiş
        Vec::new()
    }
}

// ===================================================================
// EQUIVOCATION EVIDENCE
// ===================================================================

#[derive(Debug, Clone)]
pub enum EquivocationEvidence {
    DoubleProposal {
        height: BlockHeight,
        round: u64,
        block1: Hash,
        block2: Hash,
    },
    DoubleVote {
        vote1: Vote,
        vote2: Vote,
    },
}

// ===================================================================
// SAFETY VIOLATION
// ===================================================================

#[derive(Debug, Clone, thiserror::Error)]
pub enum SafetyViolation {
    #[error("Invalid proposer: expected {expected:?}, got {got}")]
    InvalidProposer { expected: Option<u64>, got: u64 },
    
    #[error("Double proposal from validator {0}")]
    DoubleProposal(u64),
    
    #[error("Double vote from validator {0}")]
    DoubleVote(u64),
    
    #[error("Safety rule violation: {reason}")]
    RuleViolation { reason: String },
    
    #[error("Block already committed at height {0}")]
    AlreadyCommitted(BlockHeight),
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Ed25519Signer;
    use crate::consensus::validator::{ValidatorManager, MIN_VALIDATOR_STAKE};

    fn setup_safety_rules() -> SafetyRules {
        let validator_manager = Arc::new(ValidatorManager::new());
        SafetyRules::new(validator_manager)
    }

    #[test]
    fn test_lock_unlock() {
        let rules = setup_safety_rules();
        
        assert!(rules.locked().is_none());
        
        rules.lock_proposal(1, 0, [1u8; 32]);
        assert!(rules.locked().is_some());
        
        let locked = rules.locked().unwrap();
        assert_eq!(locked.height, 1);
        assert_eq!(locked.round, 0);
        
        rules.unlock();
        assert!(rules.locked().is_none());
    }
    
    #[test]
    fn test_double_vote_detection() {
        let rules = setup_safety_rules();
        
        let vote1 = Vote {
            height: 1,
            round: 0,
            block_hash: [1u8; 32],
            voter: 1,
            vote_type: VoteType::Prevote,
            signature: vec![],
            timestamp: 0,
        };
        
        let vote2 = Vote {
            height: 1,
            round: 0,
            block_hash: [2u8; 32],
            voter: 1,
            vote_type: VoteType::Prevote,
            signature: vec![],
            timestamp: 0,
        };
        
        assert!(rules.check_double_vote(&vote1, &vote2));
        
        let vote3 = Vote {
            height: 2,
            round: 0,
            block_hash: [1u8; 32],
            voter: 1,
            vote_type: VoteType::Prevote,
            signature: vec![],
            timestamp: 0,
        };
        
        assert!(!rules.check_double_vote(&vote1, &vote3));
    }
}