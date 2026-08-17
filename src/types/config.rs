use crate::types::error::PacyteError;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub node_id: u64,
    pub node_name: String,
    pub validator_key_path: PathBuf,
    pub listen_addr: SocketAddr,
    pub public_addr: Option<SocketAddr>,
    pub bootstrap_peers: Vec<String>,
    pub max_peers: usize,
    pub min_peers: usize,
    pub handshake_timeout_ms: u64,
    pub ping_interval_ms: u64,
    pub is_validator: bool,
    pub validator_stake: u128,
    pub block_time_target_ms: u64,
    pub consensus_timeout_ms: u64,
    pub max_block_size: usize,
    pub data_dir: PathBuf,
    pub rocksdb_max_open_files: i32,
    pub rocksdb_cache_size_mb: usize,
    pub wal_enabled: bool,
    pub wal_sync_interval_ms: u64,
    pub mempool_max_size: usize,
    pub mempool_max_tx_age_secs: u64,
    pub min_fee_per_byte: u64,
    pub api_enabled: bool,
    pub api_listen_addr: SocketAddr,
    pub rpc_enabled: bool,
    pub rpc_listen_addr: SocketAddr,
    pub ws_enabled: bool,
    pub ws_listen_addr: SocketAddr,
    pub metrics_enabled: bool,
    pub metrics_listen_addr: SocketAddr,
    pub log_level: String,
    pub dev_mode: bool,
    pub unsafe_rpc: bool,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node_id: 1,
            node_name: "pacyte-titan".to_string(),
            validator_key_path: PathBuf::from("./keys/validator.json"),
            listen_addr: "0.0.0.0:9333".parse().unwrap(),
            public_addr: None,
            bootstrap_peers: vec![],
            max_peers: 50,
            min_peers: 3,
            handshake_timeout_ms: 5000,
            ping_interval_ms: 30000,
            is_validator: false,
            validator_stake: 0,
            block_time_target_ms: 1000,
            consensus_timeout_ms: 3000,
            max_block_size: 4 * 1024 * 1024,
            data_dir: PathBuf::from("./data"),
            rocksdb_max_open_files: 1000,
            rocksdb_cache_size_mb: 512,
            wal_enabled: true,
            wal_sync_interval_ms: 100,
            mempool_max_size: 10000,
            mempool_max_tx_age_secs: 3600,
            min_fee_per_byte: 1,
            api_enabled: true,
            api_listen_addr: "127.0.0.1:8080".parse().unwrap(),
            rpc_enabled: true,
            rpc_listen_addr: "127.0.0.1:9332".parse().unwrap(),
            ws_enabled: false,
            ws_listen_addr: "127.0.0.1:9334".parse().unwrap(),
            metrics_enabled: true,
            metrics_listen_addr: "127.0.0.1:9090".parse().unwrap(),
            log_level: "info".to_string(),
            dev_mode: false,
            unsafe_rpc: false,
        }
    }
}

impl NodeConfig {
    pub fn from_file(path: &PathBuf) -> Result<Self, crate::types::PacyteError> {
        let contents = std::fs::read_to_string(path).map_err(|e| crate::types::PacyteError::ConfigError(e.to_string()))?;
        serde_yaml::from_str(&contents).map_err(|e| crate::types::PacyteError::ConfigError(e.to_string()))
    }
    pub fn save_to_file(&self, path: &PathBuf) -> Result<(), crate::types::PacyteError> {
        let contents = serde_yaml::to_string(self).map_err(|e| crate::types::PacyteError::ConfigError(e.to_string()))?;
        std::fs::write(path, contents).map_err(|e| crate::types::PacyteError::ConfigError(e.to_string()))?;
        Ok(())
    }
    pub fn enable_validator(&mut self, stake: u128) { self.is_validator = true; self.validator_stake = stake; }
    pub fn add_bootstrap_peer(&mut self, addr: String) { if !self.bootstrap_peers.contains(&addr) { self.bootstrap_peers.push(addr); } }
    pub fn ensure_data_dir(&self) -> Result<(), crate::types::PacyteError> {
        std::fs::create_dir_all(&self.data_dir).map_err(|e| crate::types::PacyteError::ConfigError(e.to_string()))?;
        let db_path = self.data_dir.join("db"); std::fs::create_dir_all(&db_path).ok();
        let wal_path = self.data_dir.join("wal"); std::fs::create_dir_all(&wal_path).ok();
        let keys_path = self.data_dir.join("keys"); std::fs::create_dir_all(&keys_path).ok();
        Ok(())
    }
    pub fn db_path(&self) -> PathBuf { self.data_dir.join("db") }
    pub fn wal_path(&self) -> PathBuf { self.data_dir.join("wal") }
    pub fn keys_dir(&self) -> PathBuf { self.data_dir.join("keys") }
}