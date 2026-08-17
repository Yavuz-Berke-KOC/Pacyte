// ===================================================================
// PACYTE NEXUS - JSON-RPC API
// ===================================================================

use axum::extract::State;
use axum::response::IntoResponse;
use crate::types::account::Account;
use std::sync::Arc;
use std::net::SocketAddr;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::types::{PacyteError, PacyteResult, Address, Hash, BlockHeight};
use crate::types::block::Block;
use crate::types::transaction::Transaction;
use crate::storage::Storage;
use crate::mempool::Mempool;
use crate::consensus::Consensus;
use crate::network::Network;

use super::{
    ApiConfig, BlockchainApi, ApiResponse, ApiError,
    PARSE_ERROR, INVALID_REQUEST, METHOD_NOT_FOUND, INVALID_PARAMS, INTERNAL_ERROR,
    BLOCK_NOT_FOUND, TRANSACTION_NOT_FOUND, ACCOUNT_NOT_FOUND, INVALID_TRANSACTION,
};

// ===================================================================
// RPC SERVER
// ===================================================================

pub struct RpcServer {
    config: ApiConfig,
    api: Arc<dyn BlockchainApi>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl RpcServer {
    pub fn new(
        config: ApiConfig,
        storage: Arc<dyn Storage>,
        mempool: Arc<dyn Mempool>,
        consensus: Arc<dyn Consensus>,
        network: Arc<dyn Network>,
    ) -> Self {
        let api = Arc::new(RpcApiImpl::new(storage, mempool, consensus, network));
        Self {
            config,
            api,
            shutdown_tx: None,
        }
    }
    
    pub async fn start(&mut self) -> PacyteResult<()> {
        let addr = self.config.rpc_addr;
        
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);
        
        tracing::info!("JSON-RPC API listening on http://{}", addr);
        
        let api = Arc::clone(&self.api);
        
        tokio::spawn(async move {
            let app = create_rpc_router(api);
            
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            
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
}

fn create_rpc_router(api: Arc<dyn BlockchainApi>) -> axum::Router {
    use axum::{routing::post, Json};
    
    async fn rpc_handler(
        State(api): State<Arc<dyn BlockchainApi>>,
        Json(request): Json<RpcRequest>,
    ) -> impl IntoResponse {
        let response = handle_rpc_request(api, request).await;
        Json(response)
    }
    
    axum::Router::new()
        .route("/", post(rpc_handler))
        .route("/rpc", post(rpc_handler))
        .with_state(api)
}

// ===================================================================
// RPC REQUEST/RESPONSE
// ===================================================================

#[derive(Debug, Clone, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    method: String,
    params: Option<Value>,
    id: Value,
}

impl RpcRequest {
    fn validate(&self) -> bool {
        self.jsonrpc == "2.0"
    }
}

#[derive(Debug, Clone, Serialize)]
struct RpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ApiError>,
}

// ===================================================================
// RPC HANDLER
// ===================================================================

async fn handle_rpc_request(api: Arc<dyn BlockchainApi>, request: RpcRequest) -> RpcResponse {
    if !request.validate() {
        return RpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(ApiError::new(INVALID_REQUEST, "Invalid JSON-RPC request")),
        };
    }
    
    let result = match request.method.as_str() {
        // Block methods
        "eth_blockNumber" => rpc_block_number(api).await,
        "eth_getBlockByNumber" => rpc_get_block_by_number(api, request.params).await,
        "eth_getBlockByHash" => rpc_get_block_by_hash(api, request.params).await,
        
        // Transaction methods
        "eth_getTransactionByHash" => rpc_get_transaction(api, request.params).await,
        "eth_sendRawTransaction" => rpc_send_transaction(api, request.params).await,
        "eth_getTransactionReceipt" => rpc_get_transaction_receipt(api, request.params).await,
        
        // Account methods
        "eth_getBalance" => rpc_get_balance(api, request.params).await,
        "eth_getTransactionCount" => rpc_get_nonce(api, request.params).await,
        "eth_accounts" => rpc_accounts(api).await,
        
        // Network methods
        "net_version" => rpc_net_version(api).await,
        "net_peerCount" => rpc_peer_count(api).await,
        "eth_chainId" => rpc_chain_id(api).await,
        "eth_syncing" => rpc_syncing(api).await,
        "eth_gasPrice" => rpc_gas_price(api).await,
        
        // Web3 methods
        "web3_clientVersion" => rpc_client_version(api).await,
        "web3_sha3" => rpc_sha3(request.params).await,
        
        // Pacyte custom methods
        "pacyte_getValidators" => rpc_get_validators(api).await,
        "pacyte_getConsensusState" => rpc_get_consensus_state(api).await,
        "pacyte_getMempoolStats" => rpc_get_mempool_stats(api).await,
        "pacyte_getNetworkInfo" => rpc_get_network_info(api).await,
        
        _ => Err(ApiError::new(METHOD_NOT_FOUND, format!("Method not found: {}", request.method))),
    };
    
    match result {
        Ok(value) => RpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(value),
            error: None,
        },
        Err(error) => RpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(error),
        },
    }
}

// ===================================================================
// RPC METHOD IMPLEMENTATIONS
// ===================================================================

async fn rpc_block_number(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    let height = api.get_block_height().await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?;
    
    Ok(json!(format!("0x{:x}", height)))
}

async fn rpc_get_block_by_number(api: Arc<dyn BlockchainApi>, params: Option<Value>) -> Result<Value, ApiError> {
    let params = params.ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing params"))?;
    let params: Vec<Value> = serde_json::from_value(params)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid params"))?;
    
    let block_param = params.get(0).ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing block number"))?;
    let include_txs = params.get(1).and_then(|v| v.as_bool()).unwrap_or(false);
    
    let height = parse_block_number(block_param)?;
    
    let block = api.get_block_by_height(height).await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?
        .ok_or_else(|| ApiError::new(BLOCK_NOT_FOUND, "Block not found"))?;
    
    Ok(block_to_rpc(&block, include_txs))
}

async fn rpc_get_block_by_hash(api: Arc<dyn BlockchainApi>, params: Option<Value>) -> Result<Value, ApiError> {
    let params = params.ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing params"))?;
    let params: Vec<Value> = serde_json::from_value(params)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid params"))?;
    
    let hash_str = params.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing hash"))?;
    
    let hash = parse_hash(hash_str)?;
    let include_txs = params.get(1).and_then(|v| v.as_bool()).unwrap_or(false);
    
    let block = api.get_block_by_hash(&hash).await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?
        .ok_or_else(|| ApiError::new(BLOCK_NOT_FOUND, "Block not found"))?;
    
    Ok(block_to_rpc(&block, include_txs))
}

async fn rpc_get_transaction(api: Arc<dyn BlockchainApi>, params: Option<Value>) -> Result<Value, ApiError> {
    let params = params.ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing params"))?;
    let params: Vec<Value> = serde_json::from_value(params)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid params"))?;
    
    let hash_str = params.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing hash"))?;
    
    let hash = parse_hash(hash_str)?;
    
    let tx = api.get_transaction(&hash).await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?
        .ok_or_else(|| ApiError::new(TRANSACTION_NOT_FOUND, "Transaction not found"))?;
    
    Ok(tx_to_rpc(&tx))
}

async fn rpc_send_transaction(api: Arc<dyn BlockchainApi>, params: Option<Value>) -> Result<Value, ApiError> {
    let params = params.ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing params"))?;
    let params: Vec<Value> = serde_json::from_value(params)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid params"))?;
    
    let raw_tx = params.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing raw transaction"))?;
    
    let tx_bytes = hex::decode(raw_tx.trim_start_matches("0x"))
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid hex"))?;
    
    let tx: Transaction = bincode::deserialize(&tx_bytes)
        .map_err(|_| ApiError::new(INVALID_TRANSACTION, "Invalid transaction format"))?;
    
    let hash = api.send_transaction(tx).await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?;
    
    Ok(json!(format!("0x{}", hex::encode(hash))))
}

async fn rpc_get_balance(api: Arc<dyn BlockchainApi>, params: Option<Value>) -> Result<Value, ApiError> {
    let params = params.ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing params"))?;
    let params: Vec<Value> = serde_json::from_value(params)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid params"))?;
    
    let address_str = params.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing address"))?;
    
    let address = parse_address(address_str)?;
    
    let balance = api.get_balance(&address).await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?;
    
    Ok(json!(format!("0x{:x}", balance)))
}

async fn rpc_get_nonce(api: Arc<dyn BlockchainApi>, params: Option<Value>) -> Result<Value, ApiError> {
    let params = params.ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing params"))?;
    let params: Vec<Value> = serde_json::from_value(params)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid params"))?;
    
    let address_str = params.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing address"))?;
    
    let address = parse_address(address_str)?;
    
    let nonce = api.get_nonce(&address).await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?;
    
    Ok(json!(format!("0x{:x}", nonce)))
}

async fn rpc_accounts(_api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    Ok(json!([]))
}

async fn rpc_net_version(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    Ok(json!("1"))
}

async fn rpc_peer_count(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    let peers = api.get_peers().await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?;
    
    Ok(json!(format!("0x{:x}", peers.len())))
}

async fn rpc_chain_id(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    Ok(json!("0x1"))
}

async fn rpc_syncing(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    Ok(json!(false))
}

async fn rpc_gas_price(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    Ok(json!("0x3b9aca00")) // 1 Gwei
}

async fn rpc_client_version(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    Ok(json!(format!("PacyteNexus/v{}", crate::VERSION)))
}

async fn rpc_sha3(params: Option<Value>) -> Result<Value, ApiError> {
    use sha3::{Digest, Keccak256};
    
    let params = params.ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing params"))?;
    let params: Vec<Value> = serde_json::from_value(params)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid params"))?;
    
    let data = params.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing data"))?;
    
    let bytes = hex::decode(data.trim_start_matches("0x"))
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid hex"))?;
    
    let mut hasher = Keccak256::new();
    hasher.update(&bytes);
    let hash: [u8; 32] = hasher.finalize().into();
    
    Ok(json!(format!("0x{}", hex::encode(hash))))
}

async fn rpc_get_validators(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    let validators = api.get_validators().await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?;
    
    Ok(json!(validators))
}

async fn rpc_get_consensus_state(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    let state = api.get_consensus_state().await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?;
    
    Ok(json!(state))
}

async fn rpc_get_mempool_stats(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    let stats = api.get_mempool_stats().await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?;
    
    Ok(json!(stats))
}

async fn rpc_get_network_info(api: Arc<dyn BlockchainApi>) -> Result<Value, ApiError> {
    let info = api.get_network_info().await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?;
    
    Ok(json!(info))
}

async fn rpc_get_transaction_receipt(api: Arc<dyn BlockchainApi>, params: Option<Value>) -> Result<Value, ApiError> {
    let params = params.ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing params"))?;
    let params: Vec<Value> = serde_json::from_value(params)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid params"))?;
    
    let hash_str = params.get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::new(INVALID_PARAMS, "Missing hash"))?;
    
    let hash = parse_hash(hash_str)?;
    
    let tx = api.get_transaction(&hash).await
        .map_err(|e| ApiError::new(INTERNAL_ERROR, e.to_string()))?
        .ok_or_else(|| ApiError::new(TRANSACTION_NOT_FOUND, "Transaction not found"))?;
    
    Ok(json!({
        "transactionHash": format!("0x{}", hex::encode(hash)),
        "transactionIndex": "0x0",
        "blockHash": null,
        "blockNumber": null,
        "from": format!("0x{}", hex::encode(tx.from)),
        "to": format!("0x{}", hex::encode(tx.to)),
        "cumulativeGasUsed": "0x0",
        "gasUsed": "0x0",
        "contractAddress": null,
        "logs": [],
        "logsBloom": format!("0x{}", "0".repeat(512)),
        "status": "0x1",
    }))
}

// ===================================================================
// RPC API IMPLEMENTATION
// ===================================================================

struct RpcApiImpl {
    storage: Arc<dyn Storage>,
    mempool: Arc<dyn Mempool>,
    consensus: Arc<dyn Consensus>,
    network: Arc<dyn Network>,
}

impl RpcApiImpl {
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
impl BlockchainApi for RpcApiImpl {
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
    
    async fn get_network_info(&self) -> PacyteResult<super::NetworkInfo> {
        Ok(super::NetworkInfo {
            node_id: 1,
            network_id: 1,
            peer_count: self.network.peer_count(),
            is_listening: true,
            protocol_version: crate::PROTOCOL_VERSION.to_string(),
        })
    }
    
    async fn get_peers(&self) -> PacyteResult<Vec<super::PeerInfo>> {
        Ok(self.network.connected_peers()
            .into_iter()
            .map(|p| super::PeerInfo {
                id: p.id,
                address: p.address,
                best_height: p.best_height,
                latency_ms: p.latency_ms,
            })
            .collect())
    }
    
    async fn get_consensus_state(&self) -> PacyteResult<super::ConsensusStateInfo> {
        Ok(super::ConsensusStateInfo {
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
    
    async fn get_validators(&self) -> PacyteResult<Vec<super::ValidatorInfo>> {
        Ok(Vec::new())
    }
    
    async fn get_mempool_stats(&self) -> PacyteResult<super::MempoolStats> {
        let stats = self.mempool.stats();
        Ok(super::MempoolStats {
            size: stats.total_transactions,
            total_size_bytes: stats.total_size_bytes,
            avg_fee_per_byte: stats.avg_fee_per_byte,
            oldest_tx_age_secs: stats.oldest_tx_age_secs,
        })
    }
}

// ===================================================================
// YARDIMCILAR
// ===================================================================

fn parse_block_number(value: &Value) -> Result<BlockHeight, ApiError> {
    match value {
        Value::String(s) if s == "latest" => Ok(0), // En son blok handler'da çözülür
        Value::String(s) if s == "earliest" => Ok(0),
        Value::String(s) if s == "pending" => Ok(0),
        Value::String(s) => {
            let s = s.trim_start_matches("0x");
            BlockHeight::from_str_radix(s, 16)
                .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid block number"))
        }
        Value::Number(n) => n.as_u64()
            .ok_or_else(|| ApiError::new(INVALID_PARAMS, "Invalid block number")),
        _ => Err(ApiError::new(INVALID_PARAMS, "Invalid block number format")),
    }
}

fn parse_hash(s: &str) -> Result<Hash, ApiError> {
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid hex"))?;
    
    if bytes.len() != 32 {
        return Err(ApiError::new(INVALID_PARAMS, "Hash must be 32 bytes"));
    }
    
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}

fn parse_address(s: &str) -> Result<Address, ApiError> {
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s)
        .map_err(|_| ApiError::new(INVALID_PARAMS, "Invalid hex"))?;
    
    if bytes.len() != 32 {
        return Err(ApiError::new(INVALID_PARAMS, "Address must be 32 bytes"));
    }
    
    let mut addr = [0u8; 32];
    addr.copy_from_slice(&bytes);
    Ok(addr)
}

fn block_to_rpc(block: &Block, include_txs: bool) -> Value {
    json!({
        "number": format!("0x{:x}", block.header.height),
        "hash": format!("0x{}", hex::encode(block.hash())),
        "parentHash": format!("0x{}", hex::encode(block.header.previous_hash)),
        "transactionsRoot": format!("0x{}", hex::encode(block.header.transactions_root)),
        "stateRoot": format!("0x{}", hex::encode(block.header.state_root)),
        "timestamp": format!("0x{:x}", block.header.timestamp),
        "miner": format!("0x{}", hex::encode(block.header.proposer)),
        "size": format!("0x{:x}", block.header.block_size),
        "gasLimit": "0x989680",
        "gasUsed": "0x0",
        "transactions": if include_txs {
            block.body.transactions.iter().map(tx_to_rpc).collect::<Vec<_>>()
        } else {
            block.body.transactions.iter().map(|tx| {
                json!(format!("0x{}", hex::encode(tx.hash())))
            }).collect::<Vec<_>>()
        },
        "uncles": [],
    })
}

fn tx_to_rpc(tx: &Transaction) -> Value {
    json!({
        "hash": format!("0x{}", hex::encode(tx.hash())),
        "nonce": format!("0x{:x}", tx.nonce),
        "blockHash": null,
        "blockNumber": null,
        "transactionIndex": null,
        "from": format!("0x{}", hex::encode(tx.from)),
        "to": format!("0x{}", hex::encode(tx.to)),
        "value": format!("0x{:x}", tx.amount),
        "gas": "0x0",
        "gasPrice": format!("0x{:x}", tx.fee),
        "input": "0x",
        "v": "0x0",
        "r": "0x0",
        "s": "0x0",
        "chainId": "0x1",
    })
}