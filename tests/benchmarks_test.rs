// ===================================================================
// PACYTE NEXUS - BENCHMARKS
// ===================================================================

#![feature(test)]
extern crate test;

use test::Bencher;
use pacyte_node::crypto::*;
use pacyte_node::storage::*;
use pacyte_node::execution::*;
use pacyte_node::types::*;

// ===================================================================
// CRYPTO BENCHMARKS
// ===================================================================

#[bench]
fn bench_ed25519_sign(b: &mut Bencher) {
    let signer = Ed25519Signer::generate();
    let msg = b"Pacyte Nexus Benchmark Message";
    
    b.iter(|| {
        signer.sign(msg)
    });
}

#[bench]
fn bench_ed25519_verify(b: &mut Bencher) {
    let signer = Ed25519Signer::generate();
    let msg = b"Pacyte Nexus Benchmark Message";
    let sig = signer.sign(msg);
    let pk = signer.public_key_bytes();
    
    b.iter(|| {
        Ed25519Verifier::verify(msg, &sig, &pk)
    });
}

#[bench]
fn bench_dilithium5_sign(b: &mut Bencher) {
    let signer = Dilithium5Signer::generate();
    let msg = b"Pacyte Nexus Benchmark Message";
    
    b.iter(|| {
        signer.sign(msg)
    });
}

#[bench]
fn bench_dilithium5_verify(b: &mut Bencher) {
    let signer = Dilithium5Signer::generate();
    let msg = b"Pacyte Nexus Benchmark Message";
    let sig = signer.sign(msg);
    let pk = signer.public_key_bytes();
    
    b.iter(|| {
        Dilithium5Verifier::verify(msg, &sig, &pk)
    });
}

#[bench]
fn bench_sha3_256(b: &mut Bencher) {
    let data = vec![0x42; 1024];
    
    b.iter(|| {
        hash_sha3_256(&data)
    });
}

#[bench]
fn bench_blake3(b: &mut Bencher) {
    let data = vec![0x42; 1024];
    
    b.iter(|| {
        hash_blake3(&data)
    });
}

// ===================================================================
// MERKLE TREE BENCHMARKS
// ===================================================================

#[bench]
fn bench_merkle_tree_1000(b: &mut Bencher) {
    let leaves: Vec<Hash> = (0..1000).map(|i| {
        let mut hash = [0u8; 32];
        hash[0..8].copy_from_slice(&i.to_le_bytes());
        hash
    }).collect();
    
    b.iter(|| {
        MerkleTree::new(&leaves)
    });
}

#[bench]
fn bench_merkle_proof_verify(b: &mut Bencher) {
    let leaves: Vec<Hash> = (0..1000).map(|i| {
        let mut hash = [0u8; 32];
        hash[0..8].copy_from_slice(&i.to_le_bytes());
        hash
    }).collect();
    
    let tree = MerkleTree::new(&leaves);
    let proof = tree.generate_proof(500).unwrap();
    
    b.iter(|| {
        proof.verify()
    });
}

// ===================================================================
// STORAGE BENCHMARKS
// ===================================================================

#[bench]
fn bench_block_serialization(b: &mut Bencher) {
    let block = Block::genesis();
    
    b.iter(|| {
        bincode::serialize(&block).unwrap()
    });
}

#[bench]
fn bench_block_deserialization(b: &mut Bencher) {
    let block = Block::genesis();
    let bytes = bincode::serialize(&block).unwrap();
    
    b.iter(|| {
        bincode::deserialize::<Block>(&bytes).unwrap()
    });
}

#[bench]
fn bench_transaction_serialization(b: &mut Bencher) {
    let tx = Transaction::new([1u8; 32], [2u8; 32], 1000, 10, 0);
    
    b.iter(|| {
        bincode::serialize(&tx).unwrap()
    });
}

// ===================================================================
// VM BENCHMARKS
// ===================================================================

#[bench]
fn bench_vm_simple_add(b: &mut Bencher) {
    let code = vec![
        0x60, 0x05, // PUSH1 5
        0x60, 0x03, // PUSH1 3
        0x01,       // ADD
        0x00,       // STOP
    ];
    
    b.iter(|| {
        let mut vm = VM::new(code.clone(), 100_000);
        vm.run().unwrap();
    });
}

#[bench]
fn bench_vm_loop_100(b: &mut Bencher) {
    // 100 kere dönen loop
    let code = vec![
        0x60, 0x64, // PUSH1 100
        0x60, 0x00, // PUSH1 0
        0x5b,       // JUMPDEST
        0x60, 0x01, // PUSH1 1
        0x01,       // ADD
        0x80,       // DUP1
        0x60, 0x64, // PUSH1 100
        0x11,       // GT
        0x60, 0x04, // PUSH1 4
        0x57,       // JUMPI
        0x00,       // STOP
    ];
    
    b.iter(|| {
        let mut vm = VM::new(code.clone(), 1_000_000);
        vm.run().unwrap();
    });
}

#[bench]
fn bench_vm_sha3(b: &mut Bencher) {
    let code = vec![
        0x60, 0x20, // PUSH1 32 (size)
        0x60, 0x00, // PUSH1 0 (offset)
        0x20,       // SHA3
        0x00,       // STOP
    ];
    
    b.iter(|| {
        let mut vm = VM::new(code.clone(), 100_000);
        vm.run().unwrap();
    });
}

// ===================================================================
// MEMPOOL BENCHMARKS
// ===================================================================

#[bench]
fn bench_mempool_add(b: &mut Bencher) {
    // Setup için tokio runtime gerekir
    // Bu benchmark test framework'ü ile çalışmaz, criterion kullanılmalı
}

// ===================================================================
// NETWORK BENCHMARKS
// ===================================================================

#[bench]
fn bench_message_serialization_network(b: &mut Bencher) {
    let msg = NetworkMessage::NewBlock(Block::genesis());
    
    b.iter(|| {
        msg.to_bytes()
    });
}

#[bench]
fn bench_message_deserialization_network(b: &mut Bencher) {
    let msg = NetworkMessage::NewBlock(Block::genesis());
    let bytes = msg.to_bytes();
    
    b.iter(|| {
        NetworkMessage::from_bytes(&bytes).unwrap()
    });
}


// ===================================================================
// PACYTE NEXUS - BENCHMARKS
// Bölüm 15 - Dosya 15.5: tests/benchmarks.rs
// ===================================================================

#![feature(test)]
extern crate test;

use test::Bencher;
use pacyte_node::crypto::*;
use pacyte_node::types::*;

#[bench]
fn bench_ed25519_sign(b: &mut Bencher) {
    let signer = Ed25519Signer::generate();
    let msg = b"Pacyte Nexus Benchmark Message";
    b.iter(|| signer.sign(msg));
}

#[bench]
fn bench_ed25519_verify(b: &mut Bencher) {
    let signer = Ed25519Signer::generate();
    let msg = b"Pacyte Nexus Benchmark Message";
    let sig = signer.sign(msg);
    let pk = signer.public_key_bytes();
    b.iter(|| Ed25519Verifier::verify(msg, &sig, &pk));
}

#[bench]
fn bench_dilithium5_sign(b: &mut Bencher) {
    let signer = Dilithium5Signer::generate();
    let msg = b"Pacyte Nexus Benchmark Message";
    b.iter(|| signer.sign(msg));
}

#[bench]
fn bench_dilithium5_verify(b: &mut Bencher) {
    let signer = Dilithium5Signer::generate();
    let msg = b"Pacyte Nexus Benchmark Message";
    let sig = signer.sign(msg);
    let pk = signer.public_key_bytes();
    b.iter(|| Dilithium5Verifier::verify(msg, &sig, &pk));
}

#[bench]
fn bench_sha3_256(b: &mut Bencher) {
    let data = vec![0x42; 1024];
    b.iter(|| hash_sha3_256(&data));
}

#[bench]
fn bench_blake3(b: &mut Bencher) {
    let data = vec![0x42; 1024];
    b.iter(|| hash_blake3(&data));
}