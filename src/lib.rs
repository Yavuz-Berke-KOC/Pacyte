// ===================================================================
// PACYTE NEXUS v25.0.2 - CORE LIBRARY
// ===================================================================

pub mod types;
pub mod crypto;
pub mod storage;
pub mod network;
pub mod mempool;
pub mod consensus;
pub mod execution;
pub mod vault;
pub mod api;
pub mod utils;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL_VERSION: &str = "25.0.2";
pub const GENESIS_HASH: [u8; 32] = [0u8; 32];
