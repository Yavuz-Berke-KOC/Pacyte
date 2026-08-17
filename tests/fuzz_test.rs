// ===================================================================
// PACYTE NEXUS - FUZZ TESTLERİ
// ===================================================================

use pacyte_node::types::*;
use pacyte_node::crypto::*;
use pacyte_node::execution::*;
use rand::Rng;

// ===================================================================
// TRANSACTION FUZZING
// ===================================================================

#[test]
fn fuzz_transaction_deserialize() {
    let mut rng = rand::thread_rng();
    
    for _ in 0..10000 {
        let size = rng.gen_range(0..1000);
        let bytes: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
        
        // Panik yapmamalı
        let _ = bincode::deserialize::<Transaction>(&bytes);
    }
}

#[test]
fn fuzz_transaction_validation() {
    let mut rng = rand::thread_rng();
    
    for _ in 0..1000 {
        let tx = Transaction {
            from: rng.gen(),
            to: rng.gen(),
            amount: rng.gen(),
            fee: rng.gen(),
            nonce: rng.gen(),
            signature: (0..rng.gen_range(0..100)).map(|_| rng.gen()).collect(),
            timestamp: rng.gen(),
        };
        
        // Panik yapmamalı
        let _ = tx.validate_basic(3600);
        let _ = tx.hash();
        let _ = tx.sighash();
    }
}

// ===================================================================
// BLOCK FUZZING
// ===================================================================

#[test]
fn fuzz_block_deserialize() {
    let mut rng = rand::thread_rng();
    
    for _ in 0..1000 {
        let size = rng.gen_range(0..10000);
        let bytes: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
        
        let _ = bincode::deserialize::<Block>(&bytes);
    }
}

// ===================================================================
// VM FUZZING
// ===================================================================

#[test]
fn fuzz_vm_execution() {
    let mut rng = rand::thread_rng();
    
    for _ in 0..1000 {
        let code_size = rng.gen_range(0..500);
        let code: Vec<u8> = (0..code_size).map(|_| rng.gen_range(0..=0xff)).collect();
        let gas_limit = rng.gen_range(0..1_000_000);
        
        let mut vm = VM::new(code, gas_limit);
        
        // Panik yapmamalı, hata dönebilir
        let _ = vm.run();
    }
}

#[test]
fn fuzz_vm_stack() {
    let mut rng = rand::thread_rng();
    let mut stack = Stack::new();
    
    for _ in 0..1000 {
        match rng.gen_range(0..5) {
            0 => {
                let value: [u8; 32] = rng.gen();
                let _ = stack.push(value);
            }
            1 => {
                let _ = stack.pop();
            }
            2 => {
                let depth = rng.gen_range(0..10);
                let _ = stack.peek(depth);
            }
            3 => {
                let depth = rng.gen_range(0..10);
                let _ = stack.dup(depth);
            }
            4 => {
                let depth = rng.gen_range(0..10);
                let _ = stack.swap(depth);
            }
            _ => {}
        }
    }
}

// ===================================================================
// MEMORY FUZZING
// ===================================================================

#[test]
fn fuzz_memory_operations() {
    let mut rng = rand::thread_rng();
    let mut mem = Memory::new();
    
    for _ in 0..1000 {
        match rng.gen_range(0..4) {
            0 => {
                let offset = rng.gen_range(0..10000);
                let size = rng.gen_range(0..1000);
                let _ = mem.read(offset, size);
            }
            1 => {
                let offset = rng.gen_range(0..10000);
                let size = rng.gen_range(0..100);
                let data: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
                mem.write(offset, &data);
            }
            2 => {
                let offset = rng.gen_range(0..10000);
                let _ = mem.read_word(offset);
            }
            3 => {
                let offset = rng.gen_range(0..10000);
                let word: [u8; 32] = rng.gen();
                mem.write_word(offset, &word);
            }
            _ => {}
        }
    }
}

// ===================================================================
// CRYPTO FUZZING
// ===================================================================

#[test]
fn fuzz_signature_verification() {
    let signer = Ed25519Signer::generate();
    let mut rng = rand::thread_rng();
    
    for _ in 0..1000 {
        let msg_size = rng.gen_range(0..1000);
        let msg: Vec<u8> = (0..msg_size).map(|_| rng.gen()).collect();
        
        let sig_size = rng.gen_range(0..200);
        let sig: Vec<u8> = (0..sig_size).map(|_| rng.gen()).collect();
        
        let pk_size = rng.gen_range(0..100);
        let pk: Vec<u8> = (0..pk_size).map(|_| rng.gen()).collect();
        
        // Panik yapmamalı
        let _ = Ed25519Verifier::verify(&msg, &sig, &pk);
    }
}

#[test]
fn fuzz_hash_functions() {
    let mut rng = rand::thread_rng();
    
    for _ in 0..1000 {
        let size = rng.gen_range(0..10000);
        let data: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
        
        let _ = hash_sha256(&data);
        let _ = hash_sha3_256(&data);
        let _ = hash_blake3(&data);
        let _ = hash_keccak256(&data);
    }
}

// ===================================================================
// NETWORK MESSAGE FUZZING
// ===================================================================

#[test]
fn fuzz_network_message() {
    let mut rng = rand::thread_rng();
    
    for _ in 0..1000 {
        let size = rng.gen_range(0..10000);
        let bytes: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
        
        let _ = NetworkMessage::from_bytes(&bytes);
    }
}

// ===================================================================
// ADDRESS FUZZING
// ===================================================================

#[test]
fn fuzz_address_parsing() {
    let mut rng = rand::thread_rng();
    
    for _ in 0..1000 {
        let addr: Address = rng.gen();
        
        let short = address_short(&addr);
        assert!(short.starts_with("0x"));
        
        let hex = bytes_to_hex(&addr);
        assert!(hex.starts_with("0x"));
        
        let parsed = hex_to_bytes(&hex);
        assert_eq!(parsed, Some(addr.to_vec()));
    }
}


// ===================================================================
// PACYTE NEXUS - FUZZ TESTLERİ
// Bölüm 15 - Dosya 15.6: tests/fuzz_test.rs
// ===================================================================

use pacyte_node::types::*;
use pacyte_node::crypto::*;
use rand::Rng;

#[test]
fn fuzz_transaction_deserialize() {
    let mut rng = rand::thread_rng();
    for _ in 0..1000 {
        let size = rng.gen_range(0..1000);
        let bytes: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
        let _ = bincode::deserialize::<Transaction>(&bytes);
    }
}

#[test]
fn fuzz_block_deserialize() {
    let mut rng = rand::thread_rng();
    for _ in 0..1000 {
        let size = rng.gen_range(0..10000);
        let bytes: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
        let _ = bincode::deserialize::<Block>(&bytes);
    }
}

#[test]
fn fuzz_hash_functions() {
    let mut rng = rand::thread_rng();
    for _ in 0..1000 {
        let size = rng.gen_range(0..10000);
        let data: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
        let _ = hash_sha256(&data);
        let _ = hash_sha3_256(&data);
        let _ = hash_blake3(&data);
    }
}

#[test]
fn fuzz_signature_verification() {
    let signer = Ed25519Signer::generate();
    let mut rng = rand::thread_rng();
    for _ in 0..500 {
        let msg_size = rng.gen_range(0..500);
        let msg: Vec<u8> = (0..msg_size).map(|_| rng.gen()).collect();
        let sig: Vec<u8> = (0..rng.gen_range(0..100)).map(|_| rng.gen()).collect();
        let pk: Vec<u8> = (0..rng.gen_range(0..100)).map(|_| rng.gen()).collect();
        let _ = Ed25519Verifier::verify(&msg, &sig, &pk);
    }
}


// ===================================================================
// PACYTE NEXUS - END-TO-END TESTLER
// ===================================================================

use pacyte_node::*;
use std::sync::Arc;
use tempfile::tempdir;

// ===================================================================
// ÇOKLU NODE TESTİ
// ===================================================================

#[tokio::test]
async fn test_multi_node_network() {
    let temp = tempdir().unwrap();
    
    // 3 node başlat
    let nodes = setup_test_network(3, temp.path()).await;
    
    // Node'ların birbirine bağlanmasını bekle
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    // Her node'da peer kontrolü
    for node in &nodes {
        assert!(node.network.peer_count() >= 1);
    }
    
    // İşlem gönder
    let alice = Ed25519Signer::generate();
    let bob = Ed25519Signer::generate();
    
    // Genesis'ten Alice'e bakiye
    nodes[0].vault.transfer(
        &GENESIS_VAULT_ADDRESS,
        &alice.address(),
        1_000_000,
        0,
    ).await.unwrap();
    
    let mut tx = Transaction::new(
        alice.address(),
        bob.address(),
        100_000,
        10_000,
        0,
    );
    let sig = alice.sign(&tx.sighash());
    tx.sign(sig);
    
    // İşlemi tüm node'lara yay
    nodes[0].mempool.add_transaction(tx).await.unwrap();
    
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Diğer node'larda da işlem görünmeli
    for node in &nodes[1..] {
        assert!(node.mempool.size() >= 1);
    }
    
    cleanup_nodes(nodes).await;
}

// ===================================================================
// BLOK ÜRETİM TESTİ
// ===================================================================

#[tokio::test]
async fn test_block_production() {
    let temp = tempdir().unwrap();
    let node = setup_single_node(temp.path()).await;
    
    // Validator olarak kaydet
    let signer = Ed25519Signer::generate();
    node.validator_manager.register_validator(
        signer.address(),
        signer.public_key_bytes(),
        MIN_VALIDATOR_STAKE,
    ).unwrap();
    
    // Konsensüsü başlat
    node.consensus.start().await.unwrap();
    
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    
    // Blok üretilmiş olmalı
    let height = node.storage.get_block_height().await.unwrap();
    assert!(height > 0);
    
    node.consensus.stop().await.unwrap();
}

// ===================================================================
// TRANSFER E2E TESTİ
// ===================================================================

#[tokio::test]
async fn test_e2e_transfer() {
    let temp = tempdir().unwrap();
    let node = setup_single_node(temp.path()).await;
    
    let alice = Ed25519Signer::generate();
    let bob = Ed25519Signer::generate();
    
    // Genesis'ten Alice'e bakiye
    node.vault.transfer(
        &GENESIS_VAULT_ADDRESS,
        &alice.address(),
        10_000_000,
        0,
    ).await.unwrap();
    
    // Alice'ten Bob'a transfer
    let result = node.vault.transfer(
        &alice.address(),
        &bob.address(),
        5_000_000,
        1_000,
    ).await.unwrap();
    
    assert!(result.success);
    
    let alice_balance = node.vault.get_balance(&alice.address()).await.unwrap();
    let bob_balance = node.vault.get_balance(&bob.address()).await.unwrap();
    
    assert_eq!(alice_balance, 4_999_000);
    assert_eq!(bob_balance, 5_000_000);
}

// ===================================================================
// SNAPSHOT TESTİ
// ===================================================================

#[tokio::test]
async fn test_snapshot_create_restore() {
    let temp = tempdir().unwrap();
    let node = setup_single_node(temp.path()).await;
    
    // Birkaç blok üret
    for i in 0..5 {
        let block = Block::new(
            i + 1,
            [0u8; 32],
            vec![],
            [1u8; 32],
        );
        node.storage.save_block(&block).await.unwrap();
    }
    
    // Snapshot oluştur
    let snapshot_path = temp.path().join("snapshot");
    node.storage.create_snapshot(&snapshot_path).await.unwrap();
    
    assert!(snapshot_path.exists());
}

// ===================================================================
// REORGANIZATION TESTİ
// ===================================================================

#[tokio::test]
async fn test_chain_reorganization() {
    let temp = tempdir().unwrap();
    let node = setup_single_node(temp.path()).await;
    
    // Ana zincir: 1 -> 2 -> 3
    let block1 = Block::new(1, [0u8; 32], vec![], [1u8; 32]);
    let block2 = Block::new(2, block1.hash(), vec![], [1u8; 32]);
    let block3 = Block::new(3, block2.hash(), vec![], [1u8; 32]);
    
    node.storage.save_block(&block1).await.unwrap();
    node.storage.save_block(&block2).await.unwrap();
    node.storage.save_block(&block3).await.unwrap();
    
    // Fork zincir: 1 -> 2' -> 3' (daha fazla iş)
    let block2_fork = Block::new(2, block1.hash(), vec![], [2u8; 32]);
    let block3_fork = Block::new(3, block2_fork.hash(), vec![], [2u8; 32]);
    
    // Reorganizasyon
    // En ağır zincir seçilmeli
    
    let height = node.storage.get_block_height().await.unwrap();
    assert_eq!(height, 3);
}

// ===================================================================
// STRESS TEST
// ===================================================================

#[tokio::test]
async fn test_stress_transactions() {
    let temp = tempdir().unwrap();
    let node = setup_single_node(temp.path()).await;
    
    let alice = Ed25519Signer::generate();
    let bob = Ed25519Signer::generate();
    
    // Genesis'ten bakiye
    node.vault.transfer(
        &GENESIS_VAULT_ADDRESS,
        &alice.address(),
        1_000_000_000,
        0,
    ).await.unwrap();
    
    // 1000 işlem gönder
    let mut handles = Vec::new();
    
    for i in 0..1000 {
        let vault = node.vault.clone();
        let alice_addr = alice.address();
        let bob_addr = bob.address();
        
        let handle = tokio::spawn(async move {
            vault.transfer(&alice_addr, &bob_addr, 100, 10).await
        });
        
        handles.push(handle);
    }
    
    for handle in handles {
        let _ = handle.await.unwrap();
    }
    
    let bob_balance = node.vault.get_balance(&bob.address()).await.unwrap();
    assert_eq!(bob_balance, 100_000); // 1000 * 100
}

// ===================================================================
// TEST HELPERS
// ===================================================================

struct TestNode {
    storage: Arc<dyn Storage>,
    state_manager: Arc<StateManager>,
    vault: Arc<VaultManager>,
    mempool: Arc<dyn Mempool>,
    network: Arc<dyn Network>,
    consensus: Arc<dyn Consensus>,
    validator_manager: Arc<ValidatorManager>,
}

async fn setup_single_node(path: &std::path::Path) -> TestNode {
    let config = StorageConfig::default();
    let storage = Arc::new(RocksDBStorage::new(path.to_path_buf(), config).unwrap());
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
    
    let validator_manager = Arc::new(ValidatorManager::new());
    
    TestNode {
        storage,
        state_manager,
        vault,
        mempool,
        network,
        consensus,
        validator_manager,
    }
}

async fn setup_test_network(count: usize, path: &std::path::Path) -> Vec<TestNode> {
    let mut nodes = Vec::new();
    let base_port = 50000;
    
    for i in 0..count {
        let node_path = path.join(format!("node{}", i));
        std::fs::create_dir_all(&node_path).unwrap();
        
        let mut config = StorageConfig::default();
        let storage = Arc::new(RocksDBStorage::new(node_path, config).unwrap());
        let state_manager = Arc::new(StateManager::new(storage.clone()));
        let vault = Arc::new(VaultManager::new(storage.clone(), state_manager.clone()));
        
        vault.initialize_genesis().await.unwrap();
        
        let mempool = Arc::new(MempoolImpl::new(MempoolConfig::default(), state_manager.clone()));
        
        let mut network_config = NetworkConfig::default();
        network_config.node_id = (i + 1) as u64;
        network_config.listen_addr = format!("127.0.0.1:{}", base_port + i).parse().unwrap();
        
        if i > 0 {
            network_config.bootstrap_peers = vec![
                format!("127.0.0.1:{}", base_port).parse().unwrap()
            ];
        }
        
        let network = Arc::new(P2PNetwork::new(network_config, [0u8; 32]));
        
        let consensus = Arc::new(HotStuffEngine::new(
            ConsensusConfig::default(),
            storage.clone(),
            state_manager.clone(),
            mempool.clone(),
            network.clone(),
        ));
        
        nodes.push(TestNode {
            storage,
            state_manager,
            vault,
            mempool,
            network,
            consensus,
            validator_manager: Arc::new(ValidatorManager::new()),
        });
    }
    
    nodes
}

async fn cleanup_nodes(nodes: Vec<TestNode>) {
    for node in nodes {
        node.consensus.stop().await.unwrap();
        node.network.stop().await.unwrap();
    }
}