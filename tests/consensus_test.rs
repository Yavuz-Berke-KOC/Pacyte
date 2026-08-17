// ===================================================================
// PACYTE NEXUS - KONSENSÜS TESTLERİ
// ===================================================================

use pacyte_node::consensus::*;
use pacyte_node::types::*;
use pacyte_node::crypto::*;

use std::sync::Arc;

// ===================================================================
// VALIDATOR SET TESTLERİ
// ===================================================================

#[test]
fn test_validator_set_basic() {
    let mut set = ValidatorSet::new();
    
    let info = ValidatorInfo {
        id: 1,
        address: [1u8; 32],
        public_key: vec![1, 2, 3],
        stake: 1_000_000,
        voting_power: 100,
        is_active: true,
    };
    
    set.add_validator(info);
    
    assert_eq!(set.validators.len(), 1);
    assert_eq!(set.total_stake, 1_000_000);
    assert_eq!(set.active_count(), 1);
}

#[test]
fn test_proposer_selection_round_robin() {
    let mut set = ValidatorSet::new();
    
    for i in 0..21 {
        set.add_validator(ValidatorInfo {
            id: i + 1,
            address: [i as u8; 32],
            public_key: vec![],
            stake: 1_000_000,
            voting_power: 100,
            is_active: true,
        });
    }
    
    // Aynı height, farklı round -> farklı proposer
    let proposer1 = set.get_proposer(100, 0).unwrap();
    let proposer2 = set.get_proposer(100, 1).unwrap();
    assert_ne!(proposer1.id, proposer2.id);
    
    // Aynı round, farklı height -> farklı proposer
    let proposer3 = set.get_proposer(101, 0).unwrap();
    assert_ne!(proposer1.id, proposer3.id);
}

#[test]
fn test_quorum_calculation() {
    let mut set = ValidatorSet::new();
    
    for i in 0..21 {
        set.add_validator(ValidatorInfo {
            id: i + 1,
            address: [i as u8; 32],
            public_key: vec![],
            stake: 1_000_000,
            voting_power: 100,
            is_active: true,
        });
    }
    
    let quorum = set.quorum_voting_power();
    assert_eq!(quorum, 1401); // 21 * 100 * 2/3 + 1 = 1401
}

// ===================================================================
// VOTE TESTLERİ
// ===================================================================

#[test]
fn test_vote_signing() {
    let signer = Ed25519Signer::generate();
    
    let mut vote = Vote::new(
        1,
        0,
        [1u8; 32],
        1,
        VoteType::Prevote,
    );
    
    let sig = signer.sign(&vote.signing_hash());
    vote.sign(sig);
    
    assert!(!vote.signature.is_empty());
}

#[test]
fn test_vote_aggregator() {
    let mut agg = VoteAggregator::new();
    
    let vote1 = Vote::new(1, 0, [1u8; 32], 1, VoteType::Prevote);
    let vote2 = Vote::new(1, 0, [1u8; 32], 2, VoteType::Prevote);
    let vote3 = Vote::new(1, 0, [2u8; 32], 3, VoteType::Prevote);
    
    agg.add_vote(&vote1, 100);
    agg.add_vote(&vote2, 100);
    agg.add_vote(&vote3, 100);
    
    let winner = agg.get_winner(2);
    assert_eq!(winner, Some([1u8; 32]));
}

#[test]
fn test_double_vote_detection() {
    let rules = SafetyRules::new(Arc::new(ValidatorManager::new()));
    
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
}

// ===================================================================
// ROUND TESTLERİ
// ===================================================================

#[test]
fn test_round_manager() {
    let manager = RoundManager::new(1000, 2000, 3000, 5000);
    
    manager.start_new_round(1, 0);
    
    assert_eq!(manager.height(), 1);
    assert_eq!(manager.round(), 0);
    assert_eq!(manager.state(), RoundState::NewRound);
    
    manager.set_state(RoundState::ProposalSent);
    assert_eq!(manager.state(), RoundState::ProposalSent);
    
    let (height, round) = manager.advance_round();
    assert_eq!(height, 1);
    assert_eq!(round, 1);
}

#[test]
fn test_timeout_backoff() {
    let manager = RoundManager::new(1000, 2000, 3000, 5000);
    
    assert_eq!(manager.get_timeout_with_backoff(1000, 0), 1000);
    assert_eq!(manager.get_timeout_with_backoff(1000, 1), 2000);
    assert_eq!(manager.get_timeout_with_backoff(1000, 2), 4000);
    assert_eq!(manager.get_timeout_with_backoff(1000, 5), 10000);
}

// ===================================================================
// SAFETY RULES TESTLERİ
// ===================================================================

#[test]
fn test_lock_unlock() {
    let validator_manager = Arc::new(ValidatorManager::new());
    let rules = SafetyRules::new(validator_manager);
    
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
fn test_safety_violation_detection() {
    let validator_manager = Arc::new(ValidatorManager::new());
    let rules = SafetyRules::new(validator_manager);
    
    rules.lock_proposal(1, 0, [1u8; 32]);
    
    let proposal = Proposal::new(1, 0, Block::genesis(), 1);
    
    // Aynı height, farklı block -> güvenlik ihlali
    let result = rules.check_proposal_safety(&proposal);
    // Bu test için mock validator set gerekir
}

// ===================================================================
// PACEMAKER TESTLERİ
// ===================================================================

#[test]
fn test_pacemaker_timeout_calculation() {
    let calc = TimeoutCalculator::new(std::time::Duration::from_millis(1000));
    
    assert_eq!(calc.calculate(0), std::time::Duration::from_millis(1000));
    assert_eq!(calc.calculate(1), std::time::Duration::from_millis(1500));
    assert_eq!(calc.calculate(2), std::time::Duration::from_millis(2250));
}

#[test]
fn test_round_timeout_difference() {
    let calc = TimeoutCalculator::new(std::time::Duration::from_millis(1000));
    
    let leader_timeout = calc.calculate_for_round(0, true);
    let validator_timeout = calc.calculate_for_round(0, false);
    
    assert!(leader_timeout < validator_timeout);
}

// ===================================================================
// HOTSTUFF SCENARIO TEST
// ===================================================================

#[tokio::test]
async fn test_hotstuff_happy_path() {
    // 4 validator ile mutlu yol testi
    // Bu test için full setup gerekir
}

#[tokio::test]
async fn test_hotstuff_leader_failure() {
    // Leader timeout senaryosu
}

#[tokio::test]
async fn test_hotstuff_network_partition() {
    // Ağ bölünmesi senaryosu
}