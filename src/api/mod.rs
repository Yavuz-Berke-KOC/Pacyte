// ===================================================================
// PACYTE NEXUS - API MODÜLÜ
// ===================================================================

pub mod rest;
pub mod rpc;
pub mod websocket;
pub mod types;
pub mod middleware;

// Re-export'lar
pub use rest::*;
pub use rpc::*;
pub use websocket::*;
pub use types::*;
pub use middleware::*;

use crate::types::account::Account;
use crate::types::{PacyteError, PacyteResult, Address, Hash, BlockHeight};
use crate::types::block::Block;
use crate::types::transaction::Transaction;
use crate::storage::Storage;
use crate::mempool::Mempool;
use crate::consensus::Consensus;
use crate::network::Network;
use std::sync::Arc;
use std::net::SocketAddr;

// ===================================================================
// API KONFİGÜRASYONU
// ===================================================================

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub enabled: bool,
    pub rest_enabled: bool,
    pub rpc_enabled: bool,
    pub ws_enabled: bool,
    
    pub rest_addr: SocketAddr,
    pub rpc_addr: SocketAddr,
    pub ws_addr: SocketAddr,
    
    pub cors_allowed_origins: Vec<String>,
    pub max_request_body_size: usize,
    pub rate_limit_per_second: u32,
    pub enable_metrics: bool,
    pub enable_docs: bool,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rest_enabled: true,
            rpc_enabled: true,
            ws_enabled: true,
            rest_addr: "127.0.0.1:8080".parse().unwrap(),
            rpc_addr: "127.0.0.1:9332".parse().unwrap(),
            ws_addr: "127.0.0.1:9334".parse().unwrap(),
            cors_allowed_origins: vec!["*".to_string()],
            max_request_body_size: 10 * 1024 * 1024, // 10 MB
            rate_limit_per_second: 100,
            enable_metrics: true,
            enable_docs: true,
        }
    }
}

// ===================================================================
// API SERVER
// ===================================================================

pub struct ApiServer {
    config: ApiConfig,
    storage: Arc<dyn Storage>,
    mempool: Arc<dyn Mempool>,
    consensus: Arc<dyn Consensus>,
    network: Arc<dyn Network>,
    
    rest_server: Option<RestServer>,
    rpc_server: Option<RpcServer>,
    ws_server: Option<WebSocketServer>,
}

impl ApiServer {
    pub fn new(
        config: ApiConfig,
        storage: Arc<dyn Storage>,
        mempool: Arc<dyn Mempool>,
        consensus: Arc<dyn Consensus>,
        network: Arc<dyn Network>,
    ) -> Self {
        Self {
            config,
            storage,
            mempool,
            consensus,
            network,
            rest_server: None,
            rpc_server: None,
            ws_server: None,
        }
    }
    
    pub async fn start(&mut self) -> PacyteResult<()> {
        if !self.config.enabled {
            return Ok(());
        }
        
        tracing::info!("Starting API servers...");
        
        if self.config.rest_enabled {
            self.rest_server = Some(RestServer::new(
                self.config.clone(),
                self.storage.clone(),
                self.mempool.clone(),
                self.consensus.clone(),
                self.network.clone(),
            ));
            //self.rest_server.as_ref().unwrap().start().await?;
        }
        
        if self.config.rpc_enabled {
            self.rpc_server = Some(RpcServer::new(
                self.config.clone(),
                self.storage.clone(),
                self.mempool.clone(),
                self.consensus.clone(),
                self.network.clone(),
            ));
            //self.rpc_server.as_ref().unwrap().start().await?;
        }
        
        if self.config.ws_enabled {
            self.ws_server = Some(WebSocketServer::new(
                self.config.clone(),
                self.storage.clone(),
                self.mempool.clone(),
                self.consensus.clone(),
                self.network.clone(),
            ));
            //self.ws_server.as_ref().unwrap().start().await?;
        }
        
        tracing::info!("API servers started");
        
        Ok(())
    }
    
    pub async fn stop(&mut self) -> PacyteResult<()> {
        if let Some(server) = &self.rest_server {
            server.stop().await?;
        }
        if let Some(server) = &self.rpc_server {
            server.stop().await?;
        }
        if let Some(server) = &self.ws_server {
            server.stop().await?;
        }
        Ok(())
    }
}

// ===================================================================
// ORTAK API TRAIT'LERİ
// ===================================================================

#[async_trait::async_trait]
pub trait BlockchainApi: Send + Sync {
    // Blok sorguları
    async fn get_block_by_height(&self, height: BlockHeight) -> PacyteResult<Option<Block>>;
    async fn get_block_by_hash(&self, hash: &Hash) -> PacyteResult<Option<Block>>;
    async fn get_latest_block(&self) -> PacyteResult<Option<Block>>;
    async fn get_block_height(&self) -> PacyteResult<BlockHeight>;
    
    // İşlem sorguları
    async fn get_transaction(&self, hash: &Hash) -> PacyteResult<Option<Transaction>>;
    async fn send_transaction(&self, tx: Transaction) -> PacyteResult<Hash>;
    async fn get_pending_transactions(&self) -> PacyteResult<Vec<Transaction>>;
    
    // Hesap sorguları
    async fn get_balance(&self, address: &Address) -> PacyteResult<u128>;
    async fn get_account(&self, address: &Address) -> PacyteResult<Option<Account>>;
    async fn get_nonce(&self, address: &Address) -> PacyteResult<u64>;
    
    // Ağ sorguları
    async fn get_network_info(&self) -> PacyteResult<NetworkInfo>;
    async fn get_peers(&self) -> PacyteResult<Vec<PeerInfo>>;
    
    // Konsensüs sorguları
    async fn get_consensus_state(&self) -> PacyteResult<ConsensusStateInfo>;
    async fn get_validators(&self) -> PacyteResult<Vec<ValidatorInfo>>;
    
    // Mempool sorguları
    async fn get_mempool_stats(&self) -> PacyteResult<MempoolStats>;
}

// ===================================================================
// API RESPONSE TİPLERİ
// ===================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkInfo {
    pub node_id: u64,
    pub network_id: u64,
    pub peer_count: usize,
    pub is_listening: bool,
    pub protocol_version: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConsensusStateInfo {
    pub height: BlockHeight,
    pub round: u64,
    pub state: String,
    pub is_validator: bool,
    pub is_proposer: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValidatorInfo {
    pub id: u64,
    pub address: Address,
    pub stake: u128,
    pub voting_power: u64,
    pub is_active: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MempoolStats {
    pub size: usize,
    pub total_size_bytes: usize,
    pub avg_fee_per_byte: f64,
    pub oldest_tx_age_secs: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerInfo {
    pub id: u64,
    pub address: String,
    pub best_height: BlockHeight,
    pub latency_ms: u64,
}

// ===================================================================
// API HATA YANITI
// ===================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

impl ApiError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
    
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }
}

// Standart JSON-RPC hata kodları
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

// Pacyte özel hata kodları (-32000 ile -32099 arası)
pub const BLOCK_NOT_FOUND: i32 = -32000;
pub const TRANSACTION_NOT_FOUND: i32 = -32001;
pub const ACCOUNT_NOT_FOUND: i32 = -32002;
pub const INVALID_TRANSACTION: i32 = -32003;
pub const MEMPOOL_FULL: i32 = -32004;
pub const RATE_LIMITED: i32 = -32005;

// ===================================================================
// API BAŞARI YANITI
// ===================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiResponse<T: serde::Serialize> {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: Option<T>,
    pub error: Option<ApiError>,
}

impl<T: serde::Serialize> ApiResponse<T> {
    pub fn success(id: serde_json::Value, result: T) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }
    
    pub fn error(id: serde_json::Value, error: ApiError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

// ===================================================================
// PAGINATION
// ===================================================================

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Pagination {
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            page: Some(1),
            limit: Some(20),
        }
    }
}

impl Pagination {
    pub fn offset(&self) -> usize {
        (self.page.unwrap_or(1).saturating_sub(1)) * self.limit.unwrap_or(20)
    }
    
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(20).min(100)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub page: usize,
    pub limit: usize,
    pub has_next: bool,
}