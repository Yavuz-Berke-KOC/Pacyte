// ===================================================================
// PACYTE NEXUS - ENTEGRASYON TESTLERİ
// ===================================================================

use pacyte_node::types::*;
use pacyte_node::crypto::*;
use pacyte_node::storage::*;
use pacyte_node::vault::*;
use pacyte_node::mempool::*;
use pacyte_node::consensus::*;
use pacyte_node::network::*;
use pacyte_node::execution::*;

use std::sync::Arc;
use tempfile::tempdir;

// ===================================================================
// TEST FIXTURES
// ===================================================================

async fn setup_test_node() -> TestNode {
    let temp = tempdir().unwrap();
    let storage = Arc::new(MemoryStorage::new());
    let state_manager = Arc::new(StateManager::new(storage.clone()));
    let vault = Arc::new(VaultManager::new(storage.clone(), state_manager.clone()));
    
    vault.initialize_genesis().await.unwrap();
    
    let mempool_config = MempoolConfig::default();
    let mempool = Arc::new(MempoolImpl::new(mempool_config, state_manager.clone()));
    
    let network_config = NetworkConfig::default();
    let network = Arc::new(P2PNetwork::new(network_config, [0u8; 32]));
    
    let consensus_config = ConsensusConfig::default();
    let consensus = Arc::new(HotStuffEngine::new(
        consensus_config,
        storage.clone(),
        state_manager.clone(),
        mempool.clone(),
        network.clone(),
    ));
    
    let alice = Ed25519Signer::generate();
    let bob = Ed25519Signer::generate();
    
    // Genesis'ten Alice'e bakiye aktar
    vault.transfer(
        &GENESIS_VAULT_ADDRESS,
        &alice.address(),
        1_000_000_000_000, // 1M PAC
        0,
    ).await.unwrap();
    
    TestNode {
        storage,
        state_manager,
        vault,
        mempool,
        network,
        consensus,
        alice,
        bob,
        _temp: temp,
    }
}

struct TestNode {
    storage: Arc<dyn Storage>,
    state_manager: Arc<StateManager>,
    vault: Arc<VaultManager>,
    mempool: Arc<dyn Mempool>,
    network: Arc<dyn Network>,
    consensus: Arc<dyn Consensus>,
    alice: Ed25519Signer,
    bob: Ed25519Signer,
    _temp: tempfile::TempDir,
}

// ===================================================================
// TRANSFER TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_transfer_flow() {
    let node = setup_test_node().await;
    
    let alice_balance = node.vault.get_balance(&node.alice.address()).await.unwrap();
    assert_eq!(alice_balance, 1_000_000_000_000);
    
    // Transfer işlemi
    let result = node.vault.transfer(
        &node.alice.address(),
        &node.bob.address(),
        500_000_000_000,
        1_000,
    ).await.unwrap();
    
    assert!(result.success);
    assert_eq!(result.fee_burned, result.fee_to_validator + result.fee_to_genesis + 1_000);
    
    let alice_after = node.vault.get_balance(&node.alice.address()).await.unwrap();
    let bob_after = node.vault.get_balance(&node.bob.address()).await.unwrap();
    
    assert_eq!(alice_after, 500_000_000_000 - 1_000);
    assert_eq!(bob_after, 500_000_000_000);
}

#[tokio::test]
async fn test_insufficient_balance() {
    let node = setup_test_node().await;
    
    let result = node.vault.transfer(
        &node.bob.address(), // Bob'un bakiyesi yok
        &node.alice.address(),
        100_000,
        1_000,
    ).await;
    
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        PacyteError::InsufficientBalance { .. }
    ));
}

// ===================================================================
// MEMPOOL TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_mempool_add_and_select() {
    let node = setup_test_node().await;
    
    let mut tx = Transaction::new(
        node.alice.address(),
        node.bob.address(),
        100_000,
        10_000,
        0,
    );
    
    let sig = node.alice.sign(&tx.sighash());
    tx.sign(sig);
    
    let result = node.mempool.add_transaction(tx.clone()).await.unwrap();
    assert!(result.is_added());
    assert_eq!(node.mempool.size(), 1);
    
    let selected = node.mempool.select_for_block(10, 1024 * 1024).await;
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].hash(), tx.hash());
}

#[tokio::test]
async fn test_mempool_duplicate() {
    let node = setup_test_node().await;
    
    let mut tx = Transaction::new(
        node.alice.address(),
        node.bob.address(),
        100_000,
        10_000,
        0,
    );
    
    let sig = node.alice.sign(&tx.sighash());
    tx.sign(sig);
    
    let result1 = node.mempool.add_transaction(tx.clone()).await.unwrap();
    assert!(result1.is_added());
    
    let result2 = node.mempool.add_transaction(tx).await.unwrap();
    assert_eq!(result2, AddTxResult::AlreadyExists);
}

#[tokio::test]
async fn test_mempool_capacity() {
    let node = setup_test_node().await;
    
    let config = MempoolConfig {
        max_size: 2,
        ..Default::default()
    };
    let mempool = Arc::new(MempoolImpl::new(config, node.state_manager.clone()));
    
    for i in 0..5 {
        let mut tx = Transaction::new(
            node.alice.address(),
            node.bob.address(),
            100_000,
            10_000,
            i,
        );
        let sig = node.alice.sign(&tx.sighash());
        tx.sign(sig);
        
        let _ = mempool.add_transaction(tx).await;
    }
    
    assert!(mempool.size() <= 2);
}

// ===================================================================
// CONSENSUS TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_consensus_start_stop() {
    let node = setup_test_node().await;
    
    node.consensus.start().await.unwrap();
    
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    assert!(matches!(node.consensus.state(), ConsensusState::Idle));
    
    node.consensus.stop().await.unwrap();
}

#[tokio::test]
async fn test_validator_set() {
    let manager = ValidatorManager::new();
    let signer = Ed25519Signer::generate();
    
    let id = manager.register_validator(
        signer.address(),
        signer.public_key_bytes(),
        MIN_VALIDATOR_STAKE,
    ).unwrap();
    
    assert_eq!(id, 1);
    assert_eq!(manager.active_count(), 1);
    
    let validator = manager.get_validator(id).unwrap();
    assert_eq!(validator.stake, MIN_VALIDATOR_STAKE);
}

#[tokio::test]
async fn test_proposer_selection() {
    let manager = ValidatorManager::new();
    
    for i in 0..21 {
        let signer = Ed25519Signer::generate();
        manager.register_validator(
            signer.address(),
            signer.public_key_bytes(),
            MIN_VALIDATOR_STAKE,
        ).unwrap();
    }
    
    let proposer = manager.get_proposer(100, 0);
    assert!(proposer.is_some());
}

// ===================================================================
// STORAGE TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_block_storage() {
    let storage = MemoryStorage::new();
    
    let block = Block::genesis();
    storage.save_block(&block).await.unwrap();
    
    let retrieved = storage.get_block(0).await.unwrap().unwrap();
    assert_eq!(block.hash(), retrieved.hash());
    
    let latest = storage.get_latest_block().await.unwrap().unwrap();
    assert_eq!(block.hash(), latest.hash());
    
    let height = storage.get_block_height().await.unwrap();
    assert_eq!(height, 0);
}

#[tokio::test]
async fn test_account_storage() {
    let storage = MemoryStorage::new();
    
    let addr = [1u8; 32];
    let account = Account::new(addr, 1000);
    
    storage.save_account(&addr, &account).await.unwrap();
    
    let retrieved = storage.get_account(&addr).await.unwrap().unwrap();
    assert_eq!(retrieved.balance, 1000);
    
    storage.delete_account(&addr).await.unwrap();
    assert!(storage.get_account(&addr).await.unwrap().is_none());
}

#[tokio::test]
async fn test_write_batch() {
    let storage = MemoryStorage::new();
    
    let mut batch = WriteBatch::new();
    
    let block = Block::genesis();
    batch.add_block(block.clone());
    
    let addr = [1u8; 32];
    let account = Account::new(addr, 1000);
    batch.add_account(addr, account);
    
    storage.write_batch(batch).await.unwrap();
    
    assert_eq!(storage.get_block_height().await.unwrap(), 0);
    assert!(storage.account_exists(&addr).await.unwrap());
}

// ===================================================================
// VAULT TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_supply_phase_transition() {
    let node = setup_test_node().await;
    
    assert_eq!(node.vault.current_phase(), SupplyPhase::GreatBurn);
    
    // Supply'ı değiştir
    let mut supply = node.vault.total_supply.read();
    *supply = 350_000_000_000_000;
    drop(supply);
    
    assert_eq!(node.vault.current_phase(), SupplyPhase::Transition);
}

#[tokio::test]
async fn test_burn_mechanism() {
    let node = setup_test_node().await;
    
    let initial_supply = node.vault.total_supply().await.unwrap();
    
    node.vault.burn(1_000_000, BurnReason::TransactionFee).await.unwrap();
    
    let new_supply = node.vault.total_supply().await.unwrap();
    assert_eq!(new_supply, initial_supply - 1_000_000);
}

#[tokio::test]
async fn test_fee_distribution() {
    let node = setup_test_node().await;
    
    let distribution = node.vault.distribute_fees(1_000_000, 100).await.unwrap();
    
    assert_eq!(distribution.total_fee, 1_000_000);
    assert_eq!(distribution.burned + distribution.to_validators + distribution.to_genesis, 1_000_000);
}

// ===================================================================
// DORMANCY TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_dormancy_check() {
    let manager = DormancyManager::new();
    let addr = [1u8; 32];
    
    // Eski aktivite
    let old_time = current_timestamp() - DORMANCY_SECONDS - 1000;
    manager.last_activity.insert(addr, old_time);
    
    let dormant = manager.check_dormancy(current_timestamp());
    assert!(dormant.contains(&addr));
}

#[tokio::test]
async fn test_dormant_marking() {
    let manager = DormancyManager::new();
    let addr = [1u8; 32];
    
    manager.mark_dormant(&addr, 1_000_000);
    
    assert!(manager.is_dormant(&addr));
    
    let stats = manager.stats();
    assert_eq!(stats.total_dormant_accounts, 1);
    assert_eq!(stats.total_dormant_balance, 1_000_000);
}

// ===================================================================
// CRYPTO TESTLERİ
// ===================================================================

#[test]
fn test_ed25519_sign_verify() {
    let signer = Ed25519Signer::generate();
    let msg = b"Pacyte Nexus Test";
    
    let sig = signer.sign(msg);
    let pk = signer.public_key_bytes();
    
    assert!(Ed25519Verifier::verify(msg, &sig, &pk));
}

#[test]
fn test_dilithium5_sign_verify() {
    let signer = Dilithium5Signer::generate();
    let msg = b"Pacyte Nexus Post-Quantum Test";
    
    let sig = signer.sign(msg);
    let pk = signer.public_key_bytes();
    
    assert!(Dilithium5Verifier::verify(msg, &sig, &pk));
}

#[test]
fn test_hybrid_sign_verify() {
    let signer = HybridSigner::new_both();
    let msg = b"Hybrid signature test";
    
    let sig = signer.sign(msg);
    let pk = signer.public_keys();
    
    assert!(sig.verify(msg, &pk));
}

#[test]
fn test_hash_functions() {
    let data = b"Pacyte Nexus";
    
    let sha256 = hash_sha256(data);
    assert_eq!(sha256.len(), 32);
    
    let sha3 = hash_sha3_256(data);
    assert_eq!(sha3.len(), 32);
    assert_ne!(sha256, sha3);
    
    let blake3 = hash_blake3(data);
    assert_eq!(blake3.len(), 32);
}

#[test]
fn test_merkle_tree() {
    let leaves: Vec<Hash> = (0..4).map(|i| [i as u8; 32]).collect();
    let tree = MerkleTree::new(&leaves);
    
    let proof = tree.generate_proof(2).unwrap();
    assert!(proof.verify());
    
    // Bozuk kanıt
    let mut bad_proof = proof.clone();
    bad_proof.proof_nodes[0].hash = [255u8; 32];
    assert!(!bad_proof.verify());
}

// ===================================================================
// VM TESTLERİ
// ===================================================================

#[test]
fn test_vm_arithmetic() {
    let code = vec![
        0x60, 0x05, // PUSH1 5
        0x60, 0x03, // PUSH1 3
        0x01,       // ADD
        0x00,       // STOP
    ];
    
    let mut vm = VM::new(code, 100_000);
    let result = vm.run();
    
    assert!(result.is_ok());
    assert_eq!(vm.stack.len(), 1);
    
    let top = vm.stack.pop().unwrap();
    assert_eq!(top[31], 8);
}

#[test]
fn test_vm_comparison() {
    let code = vec![
        0x60, 0x05, // PUSH1 5
        0x60, 0x03, // PUSH1 3
        0x11,       // GT
        0x00,       // STOP
    ];
    
    let mut vm = VM::new(code, 100_000);
    vm.run().unwrap();
    
    let top = vm.stack.pop().unwrap();
    assert_eq!(top[31], 1); // 5 > 3 = true
}

#[test]
fn test_vm_memory() {
    let code = vec![
        0x60, 0x2a, // PUSH1 42
        0x60, 0x00, // PUSH1 0
        0x52,       // MSTORE
        0x60, 0x00, // PUSH1 0
        0x51,       // MLOAD
        0x00,       // STOP
    ];
    
    let mut vm = VM::new(code, 100_000);
    vm.run().unwrap();
    
    let top = vm.stack.pop().unwrap();
    assert_eq!(top[31], 42);
}

// ===================================================================
// NETWORK TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_peer_manager() {
    let manager = PeerManager::new(10);
    
    let addr: SocketAddr = "127.0.0.1:9333".parse().unwrap();
    let conn = PeerConnection::new();
    
    let peer_id = manager.add_peer(addr, conn).unwrap();
    assert_eq!(manager.peer_count(), 1);
    
    manager.update_peer_state(peer_id, PeerState::Connected).unwrap();
    assert_eq!(manager.connected_count(), 1);
    
    let peers = manager.get_connected_peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].id, peer_id);
    
    manager.remove_peer(peer_id);
    assert_eq!(manager.peer_count(), 0);
}

// ===================================================================
// BRIDGE TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_bridge_lifecycle() {
    let manager = BridgeManager::new();
    
    let tx = manager.initiate_bridge([1u8; 32], [2u8; 32], 1_000_000, 1, 2).unwrap();
    let id = tx.id;
    
    assert_eq!(tx.status, BridgeStatus::Pending);
    
    manager.lock_bridge(id).unwrap();
    assert_eq!(manager.get_bridge(id).unwrap().status, BridgeStatus::Locked);
    
    manager.relay_bridge(id).unwrap();
    assert_eq!(manager.get_bridge(id).unwrap().status, BridgeStatus::Relayed);
    
    manager.finalize_bridge(id).unwrap();
    assert_eq!(manager.get_bridge(id).unwrap().status, BridgeStatus::Finalized);
}

#[tokio::test]
async fn test_bridge_expiry() {
    let manager = BridgeManager::new();
    
    let tx = manager.initiate_bridge([1u8; 32], [2u8; 32], 1_000_000, 1, 2).unwrap();
    
    let future = current_timestamp() + BRIDGE_TIMEOUT_SECS + 10;
    let expired = manager.find_expired_bridges(future);
    
    assert!(expired.contains(&tx.id));
}

// ===================================================================
// API TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_rest_api_health() {
    let node = setup_test_node().await;
    
    let api = RestApiImpl::new(
        node.storage,
        node.mempool,
        node.consensus,
        node.network,
    );
    
    let latest = api.get_latest_block().await.unwrap();
    assert!(latest.is_some());
    
    let height = api.get_block_height().await.unwrap();
    assert_eq!(height, 0);
}

// ===================================================================
// END-TO-END TEST
// ===================================================================

#[tokio::test]
async fn test_e2e_transfer_flow() {
    let node = setup_test_node().await;
    
    // 1. İşlem oluştur
    let mut tx = Transaction::new(
        node.alice.address(),
        node.bob.address(),
        100_000,
        10_000,
        0,
    );
    let sig = node.alice.sign(&tx.sighash());
    tx.sign(sig);
    
    // 2. Mempool'a ekle
    let result = node.mempool.add_transaction(tx.clone()).await.unwrap();
    assert!(result.is_added());
    
    // 3. Blok oluştur
    let txs = node.mempool.select_for_block(10, 1024 * 1024).await;
    let block = Block::new(1, [0u8; 32], txs, node.alice.address());
    
    // 4. State'i güncelle
    let version = node.state_manager.apply_block(&block).await.unwrap();
    assert_eq!(version.height, 1);
    
    // 5. Bakiyeleri kontrol et
    let alice_balance = node.vault.get_balance(&node.alice.address()).await.unwrap();
    let bob_balance = node.vault.get_balance(&node.bob.address()).await.unwrap();
    
    assert!(alice_balance < 1_000_000_000_000);
    assert_eq!(bob_balance, 100_000);
}

// ===================================================================
// CONCURRENT TEST
// ===================================================================

#[tokio::test]
async fn test_concurrent_transfers() {
    let node = setup_test_node().await;
    let vault = Arc::new(node.vault);
    
    let mut handles = Vec::new();
    
    for i in 0..10 {
        let vault = vault.clone();
        let alice = node.alice.address();
        let bob = node.bob.address();
        
        let handle = tokio::spawn(async move {
            vault.transfer(&alice, &bob, 10_000, 1_000).await
        });
        
        handles.push(handle);
    }
    
    for handle in handles {
        let _ = handle.await.unwrap();
    }
    
    let bob_balance = vault.get_balance(&node.bob.address()).await.unwrap();
    assert_eq!(bob_balance, 100_000); // 10 * 10_000
}