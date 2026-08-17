use thiserror::Error;
use crate::types::{Address, BlockHeight};
use crate::execution::ExecutionError;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum PacyteError {
    #[error("State lock acquisition failed after {0} retries")]
    StateLocked(u32),
    #[error("Insufficient balance: required {required}, available {available}")]
    InsufficientBalance { required: u128, available: u128 },
    #[error("Insufficient total supply for burn operation")]
    InsufficientSupply,
    #[error("Account not found: {0}")]
    AccountNotFound(String),
    #[error("Invalid nonce: expected {expected}, got {actual}")]
    InvalidNonce { expected: u64, actual: u64 },
    #[error("RocksDB error: {0}")]
    RocksDBError(String),
    #[error("WAL corruption detected at LSN {0}")]
    WalCorruption(u64),
    #[error("Disk I/O failure: {0}")]
    DiskIoFailure(String),
    #[error("State root mismatch: expected {expected}, got {actual}")]
    StateRootMismatch { expected: String, actual: String },
    #[error("Network I/O error: {0}")]
    NetworkError(String),
    #[error("Connection closed by peer")]
    ConnectionClosed,
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),
    #[error("Peer not found: {0}")]
    PeerNotFound(String),
    #[error("Maximum peers reached: {0}")]
    MaxPeersReached(usize),
    #[error("Network timeout after {0}ms")]
    NetworkTimeout(u64),
    #[error("Consensus timeout: round {round}, height {height}")]
    ConsensusTimeout { round: u64, height: u64 },
    #[error("Invalid block proposal: {0}")]
    InvalidProposal(String),
    #[error("Double voting detected from validator {0}")]
    DoubleVoting(u64),
    #[error("Insufficient votes: have {have}, need {need}")]
    InsufficientVotes { have: usize, need: usize },
    #[error("Validator not in active set")]
    NotValidator,
    #[error("Invalid transaction signature")]
    InvalidSignature,
    #[error("Transaction expired at height {0}")]
    TransactionExpired(u64),
    #[error("Transaction too large: {0} bytes (max: {1})")]
    TransactionTooLarge(usize, usize),
    #[error("Gas limit exceeded: used {used}, limit {limit}")]
    GasLimitExceeded { used: u64, limit: u64 },
    #[error("Invalid recipient address")]
    InvalidRecipient,
    #[error("Cryptographic error: {0}")]
    CryptoError(String),
    #[error("Invalid public key format")]
    InvalidPublicKey,
    #[error("Key derivation failed")]
    KeyDerivationFailed,
    #[error("Bridge transaction expired at {0}")]
    BridgeExpired(u64),
    #[error("Bridge already finalized")]
    BridgeAlreadyFinalized,
    #[error("Shard not found: {0}")]
    ShardNotFound(u64),
    #[error("Cross-shard verification failed")]
    CrossShardVerificationFailed,
    #[error("Hardware insufficient: missing AVX-512 support")]
    HardwareInsufficient,
    #[error("ZK proof latency too high: {0}ms > {1}ms")]
    ZkLatencyTooHigh(u64, u64),
    #[error("Validator slashed: reason = {0}")]
    ValidatorSlashed(String),
    #[error("Minimum stake not met: have {have}, need {need}")]
    InsufficientStake { have: u128, need: u128 },
    #[error("Account dormant for {0} years")]
    DormantAccount(u32),
    #[error("Cannot reactivate dormant account without sovereign approval")]
    DormantReactivationDenied,
    #[error("Invalid JSON-RPC request: {0}")]
    InvalidRpcRequest(String),
    #[error("Method not found: {0}")]
    MethodNotFound(String),
    #[error("Rate limit exceeded for {0}")]
    RateLimitExceeded(String),
    #[error("Unauthorized operation: {0}")]
    Unauthorized(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Block not found at height {0}")]
    BlockNotFound(BlockHeight),
    #[error("Peer banned: {0}")]
    PeerBanned(String),
    #[error("Peer already connected: {0}")]
    PeerAlreadyConnected(String),
    #[error("Validator already exists")]
    ValidatorAlreadyExists,
    #[error("Validator set is full")]
    ValidatorSetFull,
    #[error("Validator not found: {0}")]
    ValidatorNotFound(u64),
    #[error("Validator is inactive: {0}")]
    ValidatorInactive(u64),
    #[error("Invalid timestamp")]
    InvalidTimestamp,
    #[error("Invalid proposer: expected {expected:?}, got {got}")]
    InvalidProposer { expected: Option<u64>, got: u64 },
    #[error("Double proposal from validator {0}")]
    DoubleProposal(u64),
    #[error("Safety rule violation: {reason}")]
    SafetyViolation { reason: String },
    #[error("Block already committed at height {0}")]
    BlockAlreadyCommitted(BlockHeight),
    #[error("Contract already exists at {0:?}")]
    ContractAlreadyExists(Address),
    #[error("Contract not found: {0:?}")]
    ContractNotFound(Address),
    #[error("Contract not upgradeable: {0:?}")]
    ContractNotUpgradeable(Address),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Precompile not found: {0:?}")]
    PrecompileNotFound(Address),
    #[error("Invalid WASM module")]
    InvalidWasmModule,
    #[error("WASM module not found")]
    WasmModuleNotFound,
    #[error("WASM function not found: {0}")]
    WasmFunctionNotFound(String),
    #[error("Execution error: {0}")]
    ExecutionError(String),
    #[error("Out of gas")]
    OutOfGas,
    #[error("Amount too small: got {0}, minimum {1}")]
    AmountTooSmall(u128, u128),
    #[error("Self transfer not allowed")]
    SelfTransfer,
    #[error("Invalid amount")]
    InvalidAmount,
    #[error("Same shard transfer not allowed")]
    SameShardTransfer,
    #[error("Bridge not found: {0}")]
    BridgeNotFound(u64),
    #[error("Bridge amount too large: got {0}, max {1}")]
    BridgeAmountTooLarge(u128, u128),
    #[error("Invalid bridge status: expected {expected}, got {actual}")]
    InvalidBridgeStatus { expected: String, actual: String },
    #[error("Handshake timeout")]
    HandshakeTimeout,
}

pub type PacyteResult<T> = Result<T, PacyteError>;

impl From<std::io::Error> for PacyteError {
    fn from(err: std::io::Error) -> Self { PacyteError::DiskIoFailure(err.to_string()) }
}
impl From<serde_json::Error> for PacyteError {
    fn from(err: serde_json::Error) -> Self { PacyteError::Internal(format!("JSON error: {}", err)) }
}
impl From<hex::FromHexError> for PacyteError {
    fn from(err: hex::FromHexError) -> Self { PacyteError::CryptoError(format!("Hex decode error: {}", err)) }
}
impl From<ExecutionError> for PacyteError {
    fn from(e: ExecutionError) -> Self {
        PacyteError::ExecutionError(e.to_string())
    }
}