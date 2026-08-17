pub mod block;
pub mod transaction;
pub mod account;
pub mod error;
pub mod config;

pub use block::*;
pub use account::*;
pub use error::*;
pub use config::*;

use crate::types::transaction::Transaction;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

pub type Address = [u8; 32];
pub type ShardId = u64;
pub type BlockHeight = u64;
pub type Timestamp = u64;
pub type Nonce = u64;
pub type Hash = [u8; 32];
pub type Signature = Vec<u8>;
pub type PublicKeyBytes = Vec<u8>;

pub type SharedState = Arc<RwLock<PacyteGlobalState>>;

pub struct PacyteGlobalState {
    pub config: NodeConfig,
    pub metrics: Metrics,
}

#[derive(Debug, Clone, Default)]
pub struct Metrics {
    pub blocks_produced: u64,
    pub transactions_processed: u64,
    pub peers_connected: usize,
    pub current_tps: f64,
    pub avg_block_time_ms: u64,
    pub disk_usage_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    Handshake(HandshakeData),
    HandshakeAck(HandshakeAck),
    GetBlocks { from: BlockHeight, to: BlockHeight },
    Blocks(Vec<Block>),
    NewBlock(Block),
    NewTransaction(Transaction),
    GetTransactions(Vec<Hash>),
    Transactions(Vec<Transaction>),
    Proposal(ConsensusProposal),
    Vote(ConsensusVote),
    NewView(u64),
    Ping(u64),
    Pong(u64),
    GetPeers,
    Peers(Vec<PeerInfo>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeData {
    pub version: String,
    pub node_id: u64,
    pub port: u16,
    pub genesis_hash: Hash,
    pub best_height: BlockHeight,
    pub best_hash: Hash,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeAck {
    pub accepted: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: u64,
    pub address: String,
    pub port: u16,
    pub best_height: BlockHeight,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusProposal {
    pub height: BlockHeight,
    pub round: u64,
    pub block: Block,
    pub proposer: u64,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusVote {
    pub height: BlockHeight,
    pub round: u64,
    pub block_hash: Hash,
    pub voter: u64,
    pub vote_type: VoteType,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VoteType {
    Prevote,
    Precommit,
}

pub const GENESIS_VAULT_ADDRESS: Address = [0u8; 32];
pub const FOUNDER_VESTING_ADDRESS: Address = [1u8; 32];
pub const FOUNDER_ALLOCATION: u128 = 55_000_000_000_000;
pub const TOTAL_SUPPLY: u128 = 550_000_000_000_000;
pub const GENESIS_BALANCE: u128 = 122_500_000_000_000;
pub const MIN_STAKE: u128 = 1_000_000_000_000;
pub const TARGET_BLOCK_TIME_MS: u64 = 1000;
pub const MAX_BLOCK_SIZE: usize = 4 * 1024 * 1024;
pub const MAX_TX_SIZE: usize = 64 * 1024;

pub fn current_timestamp() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    hex::decode(hex.trim_start_matches("0x")).ok()
}

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

pub fn address_short(addr: &Address) -> String {
    format!("0x{}...{}", hex::encode(&addr[..4]), hex::encode(&addr[28..]))
}