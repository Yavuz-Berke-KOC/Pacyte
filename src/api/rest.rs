// ===================================================================
// PACYTE NEXUS - REST API
// ===================================================================

use tracing_subscriber::fmt::layer;
use super::middleware::RateLimiter;
use axum::{
    extract::{Path, Query, State, Json},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use std::sync::Arc;
use std::net::SocketAddr;
use serde::{Deserialize, Serialize};

use crate::types::{PacyteError, PacyteResult, Address, Hash, BlockHeight};
use crate::types::block::Block;
use crate::types::transaction::Transaction;
use crate::types::account::Account;
use crate::storage::Storage;
use crate::mempool::Mempool;
use crate::consensus::Consensus;
use crate::network::Network;

use super::{
    ApiConfig, BlockchainApi, Pagination, PaginatedResponse,
    NetworkInfo, ConsensusStateInfo, ValidatorInfo, MempoolStats, PeerInfo,
};

// ===================================================================
// REST SERVER
// ===================================================================

pub struct RestServer {
    config: ApiConfig,
    api: Arc<dyn BlockchainApi>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl RestServer {
    pub fn new(
        config: ApiConfig,
        storage: Arc<dyn Storage>,
        mempool: Arc<dyn Mempool>,
        consensus: Arc<dyn Consensus>,
        network: Arc<dyn Network>,
    ) -> Self {
        let api = Arc::new(RestApiImpl::new(storage, mempool, consensus, network));
        Self {
            config,
            api,
            shutdown_tx: None,
        }
    }
    
    pub async fn start(&mut self) -> PacyteResult<()> {
        let app = self.create_router();
        let addr = self.config.rest_addr;
        
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);
        
        tracing::info!("REST API listening on http://{}", addr);
        
        let listener = tokio::net::TcpListener::bind(addr).await
            .map_err(|e| PacyteError::NetworkError(e.to_string()))?;
        
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                })
                .await
                .ok();
        });
        
        Ok(())
    }
    
    pub async fn stop(&self) -> PacyteResult<()> {
        Ok(())
    }
    
    fn create_router(&self) -> Router {
	let rate_limiter = Arc::new(RateLimiter::new(100, 1)); // saniyede 100 istek
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST])
            .allow_headers(Any);
        
        let state = Arc::clone(&self.api);
        
        Router::new()
            // Blok endpoint'leri
            .route("/api/v1/blocks/latest", get(get_latest_block))
            .route("/api/v1/blocks/:height", get(get_block_by_height))
            .route("/api/v1/blocks/hash/:hash", get(get_block_by_hash))
            .route("/api/v1/blocks", get(get_blocks))
            
            // İşlem endpoint'leri
            .route("/api/v1/transactions/:hash", get(get_transaction))
            .route("/api/v1/transactions", post(send_transaction))
            .route("/api/v1/transactions/pending", get(get_pending_transactions))
            
            // Hesap endpoint'leri
            .route("/api/v1/accounts/:address", get(get_account))
            .route("/api/v1/accounts/:address/balance", get(get_balance))
            
            // Ağ endpoint'leri
            .route("/api/v1/network/info", get(get_network_info))
            .route("/api/v1/network/peers", get(get_peers))
            
            // Konsensüs endpoint'leri
            .route("/api/v1/consensus/state", get(get_consensus_state))
            .route("/api/v1/consensus/validators", get(get_validators))
            
            // Mempool endpoint'leri
            .route("/api/v1/mempool/stats", get(get_mempool_stats))
            
            // Sağlık kontrolü
            .route("/health", get(health_check))
            .route("/api/v1/health", get(health_check))
            
            // Dokümantasyon
            .route("/api/v1/docs", get(get_docs))
            
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .with_state(state)
    }
}

// ===================================================================
// REST API IMPLEMENTATION
// ===================================================================

struct RestApiImpl {
    storage: Arc<dyn Storage>,
    mempool: Arc<dyn Mempool>,
    consensus: Arc<dyn Consensus>,
    network: Arc<dyn Network>,
}

impl RestApiImpl {
    fn new(
        storage: Arc<dyn Storage>,
        mempool: Arc<dyn Mempool>,
        consensus: Arc<dyn Consensus>,
        network: Arc<dyn Network>,
    ) -> Self {
        Self {
            storage,
            mempool,
            consensus,
            network,
        }
    }
}

#[async_trait::async_trait]
impl BlockchainApi for RestApiImpl {
    async fn get_block_by_height(&self, height: BlockHeight) -> PacyteResult<Option<Block>> {
        self.storage.get_block(height).await
    }
    
    async fn get_block_by_hash(&self, hash: &Hash) -> PacyteResult<Option<Block>> {
        self.storage.get_block_by_hash(hash).await
    }
    
    async fn get_latest_block(&self) -> PacyteResult<Option<Block>> {
        self.storage.get_latest_block().await
    }
    
    async fn get_block_height(&self) -> PacyteResult<BlockHeight> {
        self.storage.get_block_height().await
    }
    
    async fn get_transaction(&self, hash: &Hash) -> PacyteResult<Option<Transaction>> {
        self.storage.get_transaction(hash).await
    }
    
    async fn send_transaction(&self, tx: Transaction) -> PacyteResult<Hash> {
        let hash = tx.hash();
        self.mempool.add_transaction(tx).await
            .map_err(|e| PacyteError::Internal(e.to_string()))?;
        Ok(hash)
    }
    
    async fn get_pending_transactions(&self) -> PacyteResult<Vec<Transaction>> {
        Ok(self.mempool.get_all_transactions())
    }
    
    async fn get_balance(&self, address: &Address) -> PacyteResult<u128> {
        self.storage.get_account(address).await
            .map(|acc| acc.map(|a| a.balance).unwrap_or(0))
    }
    
    async fn get_account(&self, address: &Address) -> PacyteResult<Option<Account>> {
        self.storage.get_account(address).await
    }
    
    async fn get_nonce(&self, address: &Address) -> PacyteResult<u64> {
        self.storage.get_account(address).await
            .map(|acc| acc.map(|a| a.nonce).unwrap_or(0))
    }
    
    async fn get_network_info(&self) -> PacyteResult<NetworkInfo> {
        Ok(NetworkInfo {
            node_id: 1,
            network_id: 1,
            peer_count: self.network.peer_count(),
            is_listening: true,
            protocol_version: crate::PROTOCOL_VERSION.to_string(),
        })
    }
    
    async fn get_peers(&self) -> PacyteResult<Vec<PeerInfo>> {
        Ok(self.network.connected_peers()
            .into_iter()
            .map(|p| PeerInfo {
                id: p.id,
                address: p.address,
                best_height: p.best_height,
                latency_ms: p.latency_ms,
            })
            .collect())
    }
    
    async fn get_consensus_state(&self) -> PacyteResult<ConsensusStateInfo> {
        Ok(ConsensusStateInfo {
            height: self.consensus.current_height(),
            round: self.consensus.current_round(),
            state: self.consensus.state().to_string(),
            is_validator: self.consensus.is_validator(),
            is_proposer: self.consensus.is_proposer(
                self.consensus.current_height(),
                self.consensus.current_round()
            ),
        })
    }
    
    async fn get_validators(&self) -> PacyteResult<Vec<ValidatorInfo>> {
        Ok(Vec::new()) // Placeholder
    }
    
    async fn get_mempool_stats(&self) -> PacyteResult<MempoolStats> {
        let stats = self.mempool.stats();
        Ok(MempoolStats {
            size: stats.total_transactions,
            total_size_bytes: stats.total_size_bytes,
            avg_fee_per_byte: stats.avg_fee_per_byte,
            oldest_tx_age_secs: stats.oldest_tx_age_secs,
        })
    }
}

// ===================================================================
// HANDLER'LAR
// ===================================================================

type ApiState = State<Arc<dyn BlockchainApi>>;

async fn get_latest_block(State(api): ApiState) -> impl IntoResponse {
    match api.get_latest_block().await {
        Ok(Some(block)) => Json(block).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "No blocks found").into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_block_by_height(
    State(api): ApiState,
    Path(height): Path<BlockHeight>,
) -> impl IntoResponse {
    match api.get_block_by_height(height).await {
        Ok(Some(block)) => Json(block).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, format!("Block {} not found", height)).into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_block_by_hash(
    State(api): ApiState,
    Path(hash_str): Path<String>,
) -> impl IntoResponse {
    let hash = match hex::decode(hash_str.trim_start_matches("0x")) {
        Ok(h) if h.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            arr
        }
        _ => return (StatusCode::BAD_REQUEST, "Invalid hash format").into_response(),
    };
    
    match api.get_block_by_hash(&hash).await {
        Ok(Some(block)) => Json(block).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Block not found").into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_blocks(
    State(api): ApiState,
    Query(pagination): Query<Pagination>,
) -> impl IntoResponse {
    let height = api.get_block_height().await.unwrap_or(0);
    let limit = pagination.limit();
    let offset = pagination.offset();
    
    let mut blocks = Vec::new();
    for h in (0..=height).rev().skip(offset).take(limit) {
        if let Ok(Some(block)) = api.get_block_by_height(h).await {
            blocks.push(block);
        }
    }
    
    let response = PaginatedResponse {
        items: blocks,
        total: height as usize + 1,
        page: pagination.page.unwrap_or(1),
        limit,
        has_next: offset + limit <= height as usize,
    };
    
    Json(response).into_response()
}

async fn get_transaction(
    State(api): ApiState,
    Path(hash_str): Path<String>,
) -> impl IntoResponse {
    let hash = match hex::decode(hash_str.trim_start_matches("0x")) {
        Ok(h) if h.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            arr
        }
        _ => return (StatusCode::BAD_REQUEST, "Invalid hash format").into_response(),
    };
    
    match api.get_transaction(&hash).await {
        Ok(Some(tx)) => Json(tx).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Transaction not found").into_response(),
        Err(e) => error_response(e),
    }
}

async fn send_transaction(
    State(api): ApiState,
    Json(tx): Json<Transaction>,
) -> impl IntoResponse {
    match api.send_transaction(tx).await {
        Ok(hash) => {
            #[derive(Serialize)]
            struct SendTxResponse {
                hash: String,
            }
            Json(SendTxResponse {
                hash: format!("0x{}", hex::encode(hash)),
            }).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn get_pending_transactions(State(api): ApiState) -> impl IntoResponse {
    match api.get_pending_transactions().await {
        Ok(txs) => Json(txs).into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_account(
    State(api): ApiState,
    Path(address_str): Path<String>,
) -> impl IntoResponse {
    let address = match parse_address(&address_str) {
        Ok(addr) => addr,
        Err(e) => return e.into_response(),
    };
    
    match api.get_account(&address).await {
        Ok(Some(account)) => Json(account).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Account not found").into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_balance(
    State(api): ApiState,
    Path(address_str): Path<String>,
) -> impl IntoResponse {
    let address = match parse_address(&address_str) {
        Ok(addr) => addr,
        Err(e) => return e.into_response(),
    };
    
    match api.get_balance(&address).await {
        Ok(balance) => {
            #[derive(Serialize)]
            struct BalanceResponse {
                address: String,
                balance: u128,
            }
            Json(BalanceResponse {
                address: address_str,
                balance,
            }).into_response()
        }
        Err(e) => error_response(e),
    }
}

async fn get_network_info(State(api): ApiState) -> impl IntoResponse {
    match api.get_network_info().await {
        Ok(info) => Json(info).into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_peers(State(api): ApiState) -> impl IntoResponse {
    match api.get_peers().await {
        Ok(peers) => Json(peers).into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_consensus_state(State(api): ApiState) -> impl IntoResponse {
    match api.get_consensus_state().await {
        Ok(state) => Json(state).into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_validators(State(api): ApiState) -> impl IntoResponse {
    match api.get_validators().await {
        Ok(validators) => Json(validators).into_response(),
        Err(e) => error_response(e),
    }
}

async fn get_mempool_stats(State(api): ApiState) -> impl IntoResponse {
    match api.get_mempool_stats().await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => error_response(e),
    }
}

async fn health_check() -> impl IntoResponse {
    #[derive(Serialize)]
    struct HealthResponse {
        status: &'static str,
        version: &'static str,
        timestamp: u64,
    }
    
    Json(HealthResponse {
        status: "ok",
        version: crate::VERSION,
        timestamp: crate::types::current_timestamp(),
    })
}

async fn get_docs() -> impl IntoResponse {
    const API_DOCS: &str = r#"
# Pacyte Nexus API Documentation

## Blocks
- `GET /api/v1/blocks/latest` - Get latest block
- `GET /api/v1/blocks/:height` - Get block by height
- `GET /api/v1/blocks/hash/:hash` - Get block by hash
- `GET /api/v1/blocks?page=1&limit=20` - List blocks

## Transactions
- `GET /api/v1/transactions/:hash` - Get transaction
- `POST /api/v1/transactions` - Send transaction
- `GET /api/v1/transactions/pending` - Get pending transactions

## Accounts
- `GET /api/v1/accounts/:address` - Get account info
- `GET /api/v1/accounts/:address/balance` - Get account balance

## Network
- `GET /api/v1/network/info` - Get network info
- `GET /api/v1/network/peers` - Get connected peers

## Consensus
- `GET /api/v1/consensus/state` - Get consensus state
- `GET /api/v1/consensus/validators` - Get validator set

## Mempool
- `GET /api/v1/mempool/stats` - Get mempool statistics
"#;
    
    (StatusCode::OK, [("content-type", "text/markdown")], API_DOCS)
}

// ===================================================================
// YARDIMCILAR
// ===================================================================

fn parse_address(s: &str) -> Result<Address, Response> {
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid hex address").into_response())?;
    
    if bytes.len() != 32 {
        return Err((StatusCode::BAD_REQUEST, "Address must be 32 bytes").into_response());
    }
    
    let mut addr = [0u8; 32];
    addr.copy_from_slice(&bytes);
    Ok(addr)
}

fn error_response(e: PacyteError) -> Response {
    let status = match e {
        PacyteError::BlockNotFound(_) => StatusCode::NOT_FOUND,
        PacyteError::AccountNotFound(_) => StatusCode::NOT_FOUND,
        PacyteError::InsufficientBalance { .. } => StatusCode::BAD_REQUEST,
        PacyteError::InvalidSignature => StatusCode::UNAUTHORIZED,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    
    #[derive(Serialize)]
    struct ErrorBody {
        error: String,
    }
    
    (status, Json(ErrorBody { error: e.to_string() })).into_response()
}