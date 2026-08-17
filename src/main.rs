// ===================================================================
// PACYTE NEXUS v25.0.2 - MAIN ENTRY POINT
// ===================================================================

use crate::vault::Vault;
use crate::consensus::Consensus;
use crate::consensus::sentinel::Sentinel;
use crate::mempool::Mempool;
use pacyte_node::network::Network;
use pacyte_node::storage::Storage;
use pacyte_node::*;
use clap::Parser;
use std::sync::Arc;
use tracing::{info, warn, error, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(author, version, about = "Pacyte Nexus Titan Node - Hardware Meritocracy Layer-1")]
struct Args {
    #[arg(short, long, default_value = "1")]
    node_id: u64,
    #[arg(short, long, default_value = "9333")]
    port: u16,
    #[arg(short, long)]
    bootstrap: Option<String>,
    #[arg(short, long, default_value = "./data")]
    data_dir: String,
    #[arg(long)]
    validator: bool,
    #[arg(long, default_value = "1000000")]
    stake: u128,
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(match Args::parse().log_level.as_str() {
            "trace" => Level::TRACE,
            "debug" => Level::DEBUG,
            "warn" => Level::WARN,
            "error" => Level::ERROR,
            _ => Level::INFO,
        })
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .pretty()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let args = Args::parse();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║            PACYTE NEXUS v25.0.2 — TITAN NODE                ║");
    println!("║         Hardware Meritocracy Layer-1 Blockchain             ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    info!("Starting node {} on port {}", args.node_id, args.port);
    info!("Data directory: {}", args.data_dir);

    let mut config = types::NodeConfig::default();
    config.node_id = args.node_id;
    config.data_dir = args.data_dir.into();
    config.listen_addr = format!("0.0.0.0:{}", args.port).parse()?;

    if args.validator {
        config.enable_validator(args.stake * 1_000_000);
        info!("Validator mode enabled with {} PAC stake", args.stake);
    }

    if let Some(bootstrap) = args.bootstrap {
        for peer in bootstrap.split(',') {
            config.add_bootstrap_peer(peer.trim().to_string());
        }
        info!("Bootstrap peers: {:?}", config.bootstrap_peers);
    }

    config.ensure_data_dir()?;

    info!("Initializing RocksDB storage...");
    let storage_config = storage::StorageConfig::default();
    let storage = Arc::new(storage::RocksDBStorage::new(config.db_path(), storage_config)?);

    let state_manager = Arc::new(storage::StateManager::new(storage.clone()));

    info!("Initializing Vault...");
    let vault = Arc::new(vault::VaultManager::new(storage.clone(), state_manager.clone()));

    let genesis_exists = state_manager.account_exists(&vault::GENESIS_VAULT_ADDRESS).await?;
    if !genesis_exists {
        info!("Initializing Genesis Vault...");
        vault.initialize_genesis().await?;
        info!("Genesis Vault created with {} PAC", vault::GENESIS_BALANCE / 1_000_000);
    }

    info!("Checking hardware capabilities...");
    let avx512_supported = consensus::has_avx512();
    let cpu_info = consensus::get_cpu_info();
    info!("CPU: {}", cpu_info);
    info!("AVX-512: {}", if avx512_supported { "✅ Supported" } else { "❌ Not Supported" });

    if config.is_validator && !avx512_supported {
        warn!("⚠️ Validator mode enabled but AVX-512 not supported. Performance may be degraded.");
    }

    info!("Initializing Mempool...");
    let mempool_config = mempool::MempoolConfig {
        max_size: config.mempool_max_size,
        max_tx_age_secs: config.mempool_max_tx_age_secs,
        min_fee_per_byte: config.min_fee_per_byte,
        ..Default::default()
    };
    let mempool = Arc::new(mempool::MempoolImpl::new(mempool_config, state_manager.clone()));

    info!("Initializing P2P Network...");
    let network_config = network::NetworkConfig {
        node_id: config.node_id,
        listen_addr: config.listen_addr,
        public_addr: config.public_addr,
        bootstrap_peers: config.bootstrap_peers.iter().filter_map(|p| p.parse().ok()).collect(),
        max_peers: config.max_peers,
        min_peers: config.min_peers,
        handshake_timeout_ms: config.handshake_timeout_ms,
        ping_interval_ms: config.ping_interval_ms,
        max_message_size: 16 * 1024 * 1024,
    };

    let genesis_hash = storage.get_block(0).await?.map(|b| b.hash()).unwrap_or([0u8; 32]);
    let network = Arc::new(network::P2PNetwork::new(network_config, genesis_hash));

    info!("Initializing Consensus Engine...");
    let consensus_config = consensus::ConsensusConfig {
        validator_count: 21,
        block_time_target_ms: config.block_time_target_ms,
        //consensus_timeout_ms: config.consensus_timeout_ms,
        //max_block_size: config.max_block_size,
        ..Default::default()
    };

    let consensus = Arc::new(consensus::HotStuffEngine::new(
        consensus_config,
        storage.clone(),
        state_manager.clone(),
        mempool.clone(),
        network.clone(),
    ));

    if config.is_validator {
        info!("Registering as validator...");
        let validator_manager = consensus::ValidatorManager::new();
        let signer = crypto::HybridSigner::new_both();

        match validator_manager.register_validator(
            signer.address(),
            signer.public_keys().to_bytes(),
            config.validator_stake,
        ) {
            Ok(id) => info!("✅ Registered as validator #{}", id),
            Err(e) => error!("❌ Failed to register validator: {}", e),
        }
    }

    if config.api_enabled {
        info!("Starting API servers...");
        let api_config = api::ApiConfig {
            enabled: true,
            rest_enabled: true,
            rpc_enabled: config.rpc_enabled,
            ws_enabled: config.ws_enabled,
            rest_addr: config.api_listen_addr,
            rpc_addr: config.rpc_listen_addr,
            ws_addr: config.ws_listen_addr,
            ..Default::default()
        };

        let mut api_server = api::ApiServer::new(
            api_config,
            storage.clone(),
            mempool.clone(),
            consensus.clone(),
            network.clone(),
        );

        tokio::spawn(async move {
            if let Err(e) = api_server.start().await {
                error!("API server error: {}", e);
            }
        });
    }

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel(1);
    ctrlc::set_handler(move || { let _ = shutdown_tx.blocking_send(()); })?;

    info!("Starting P2P network...");
    let network_clone = network.clone();
    tokio::spawn(async move {
        if let Err(e) = network_clone.start().await {
            error!("Network error: {}", e);
        }
    });

    info!("Starting consensus engine...");
    let consensus_clone = consensus.clone();
    tokio::spawn(async move {
        if let Err(e) = consensus_clone.start().await {
            error!("Consensus error: {}", e);
        }
    });

    // Sentinal (Watcher) başlat - Validator değilse
    if !config.is_validator {
        info!("Starting Sentinel (Watcher) node...");
        let sentinel_config = consensus::sentinel::SentinelConfig::default();
        let sentinel = Arc::new(consensus::sentinel::SentinelNode::new(
            sentinel_config,
            network.clone(),
        ));
        sentinel.start_watching().await?;
        info!("✅ Sentinel is watching the network");
    }

    info!("🚀 Pacyte Nexus node is running!");
    info!("📡 P2P: 0.0.0.0:{}", args.port);
    info!("🌐 RPC: http://{}", config.rpc_listen_addr);
    info!("Press Ctrl+C to stop");
    println!();

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let height = storage.get_block_height().await?;
    let peers = network.peer_count();
    let mempool_size = mempool.size();
    let supply = vault.total_supply().await?;

    info!("📊 Status: height={}, peers={}, mempool={}, supply={} PAC", height, peers, mempool_size, supply / 1_000_000);

    shutdown_rx.recv().await;
    info!("Shutting down...");
    consensus.stop().await?;
    network.stop().await?;
    storage.close().await?;

    info!("✅ Pacyte Nexus node stopped.");
    Ok(())
}