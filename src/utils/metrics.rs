// ===================================================================
// PACYTE NEXUS - METRICS (PROMETHEUS)
// ===================================================================

use prometheus::{
    self, 
    register_counter_vec, register_gauge_vec, register_histogram_vec,
    CounterVec, GaugeVec, HistogramVec, HistogramOpts,
    Encoder, TextEncoder,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::net::SocketAddr;
use parking_lot::RwLock;

// ===================================================================
// PACYTE METRICS
// ===================================================================

#[derive(Clone)]
pub struct PacyteMetrics {
    // Blok metrikleri
    pub block_height: GaugeVec,
    pub block_time_seconds: HistogramVec,
    pub blocks_produced_total: CounterVec,
    pub block_size_bytes: HistogramVec,
    
    // İşlem metrikleri
    pub transactions_total: CounterVec,
    pub transactions_pending: GaugeVec,
    pub transaction_size_bytes: HistogramVec,
    pub transaction_gas_used: HistogramVec,
    pub transaction_fee: HistogramVec,
    
    // Peer metrikleri
    pub peers_connected: GaugeVec,
    pub peer_messages_total: CounterVec,
    pub peer_latency_ms: HistogramVec,
    
    // Konsensüs metrikleri
    pub consensus_round: GaugeVec,
    pub consensus_proposals_total: CounterVec,
    pub consensus_votes_total: CounterVec,
    pub consensus_timeouts_total: CounterVec,
    
    // Mempool metrikleri
    pub mempool_size: GaugeVec,
    pub mempool_bytes: GaugeVec,
    
    // Storage metrikleri
    pub storage_disk_bytes: GaugeVec,
    pub storage_read_latency: HistogramVec,
    pub storage_write_latency: HistogramVec,
    
    // API metrikleri
    pub api_requests_total: CounterVec,
    pub api_request_duration: HistogramVec,
    
    // Sistem metrikleri
    pub system_cpu_percent: GaugeVec,
    pub system_memory_bytes: GaugeVec,
    pub system_uptime_seconds: GaugeVec,
}

impl PacyteMetrics {
    pub fn new() -> Result<Self, prometheus::Error> {
        Ok(Self {
            block_height: register_gauge_vec!(
                "pacyte_block_height",
                "Current block height",
                &["node_id"]
            )?,
            block_time_seconds: register_histogram_vec!(
                "pacyte_block_time_seconds",
                "Block production time in seconds",
                &["node_id"],
                vec![0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0]
            )?,
            blocks_produced_total: register_counter_vec!(
                "pacyte_blocks_produced_total",
                "Total number of blocks produced",
                &["node_id", "validator"]
            )?,
            block_size_bytes: register_histogram_vec!(
                "pacyte_block_size_bytes",
                "Block size in bytes",
                &["node_id"],
                vec![1024.0, 10240.0, 102400.0, 1048576.0, 4194304.0]
            )?,
            
            transactions_total: register_counter_vec!(
                "pacyte_transactions_total",
                "Total number of transactions",
                &["node_id", "type"]
            )?,
            transactions_pending: register_gauge_vec!(
                "pacyte_transactions_pending",
                "Number of pending transactions",
                &["node_id"]
            )?,
            transaction_size_bytes: register_histogram_vec!(
                "pacyte_transaction_size_bytes",
                "Transaction size in bytes",
                &["node_id"],
                vec![100.0, 250.0, 500.0, 1000.0, 5000.0]
            )?,
            transaction_gas_used: register_histogram_vec!(
                "pacyte_transaction_gas_used",
                "Gas used per transaction",
                &["node_id"],
                vec![21000.0, 50000.0, 100000.0, 500000.0, 1000000.0]
            )?,
            transaction_fee: register_histogram_vec!(
                "pacyte_transaction_fee",
                "Transaction fee in PAC",
                &["node_id"],
                vec![0.001, 0.01, 0.1, 1.0, 10.0, 100.0]
            )?,
            
            peers_connected: register_gauge_vec!(
                "pacyte_peers_connected",
                "Number of connected peers",
                &["node_id"]
            )?,
            peer_messages_total: register_counter_vec!(
                "pacyte_peer_messages_total",
                "Total peer messages",
                &["node_id", "direction", "type"]
            )?,
            peer_latency_ms: register_histogram_vec!(
                "pacyte_peer_latency_ms",
                "Peer latency in milliseconds",
                &["node_id", "peer_id"],
                vec![10.0, 50.0, 100.0, 250.0, 500.0, 1000.0]
            )?,
            
            consensus_round: register_gauge_vec!(
                "pacyte_consensus_round",
                "Current consensus round",
                &["node_id", "height"]
            )?,
            consensus_proposals_total: register_counter_vec!(
                "pacyte_consensus_proposals_total",
                "Total consensus proposals",
                &["node_id", "result"]
            )?,
            consensus_votes_total: register_counter_vec!(
                "pacyte_consensus_votes_total",
                "Total consensus votes",
                &["node_id", "type"]
            )?,
            consensus_timeouts_total: register_counter_vec!(
                "pacyte_consensus_timeouts_total",
                "Total consensus timeouts",
                &["node_id", "phase"]
            )?,
            
            mempool_size: register_gauge_vec!(
                "pacyte_mempool_size",
                "Number of transactions in mempool",
                &["node_id"]
            )?,
            mempool_bytes: register_gauge_vec!(
                "pacyte_mempool_bytes",
                "Total size of mempool in bytes",
                &["node_id"]
            )?,
            
            storage_disk_bytes: register_gauge_vec!(
                "pacyte_storage_disk_bytes",
                "Disk usage in bytes",
                &["node_id", "type"]
            )?,
            storage_read_latency: register_histogram_vec!(
                "pacyte_storage_read_latency_seconds",
                "Storage read latency",
                &["node_id"],
                vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05]
            )?,
            storage_write_latency: register_histogram_vec!(
                "pacyte_storage_write_latency_seconds",
                "Storage write latency",
                &["node_id"],
                vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05]
            )?,
            
            api_requests_total: register_counter_vec!(
                "pacyte_api_requests_total",
                "Total API requests",
                &["node_id", "method", "path", "status"]
            )?,
            api_request_duration: register_histogram_vec!(
                "pacyte_api_request_duration_seconds",
                "API request duration",
                &["node_id", "method", "path"],
                vec![0.001, 0.01, 0.1, 0.5, 1.0, 5.0]
            )?,
            
            system_cpu_percent: register_gauge_vec!(
                "pacyte_system_cpu_percent",
                "CPU usage percentage",
                &["node_id"]
            )?,
            system_memory_bytes: register_gauge_vec!(
                "pacyte_system_memory_bytes",
                "Memory usage in bytes",
                &["node_id", "type"]
            )?,
            system_uptime_seconds: register_gauge_vec!(
                "pacyte_system_uptime_seconds",
                "Node uptime in seconds",
                &["node_id"]
            )?,
        })
    }
}

// ===================================================================
// METRICS SERVER
// ===================================================================

pub struct MetricsServer {
    metrics: Arc<PacyteMetrics>,
    node_id: String,
    registry: prometheus::Registry,
}

impl MetricsServer {
    pub fn new(node_id: u64) -> Result<Self, prometheus::Error> {
        let metrics = Arc::new(PacyteMetrics::new()?);
        let registry = prometheus::Registry::new();
        
        Ok(Self {
            metrics,
            node_id: node_id.to_string(),
            registry,
        })
    }
    
    pub async fn start(&self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{routing::get, Router};
        
        let registry = self.registry.clone();
        
        let app = Router::new()
            .route("/metrics", get(move || metrics_handler(registry)));
        
        tracing::info!("Metrics server listening on http://{}", addr);
        
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
        
        Ok(())
    }
    
    pub fn metrics(&self) -> &Arc<PacyteMetrics> {
        &self.metrics
    }
}

async fn metrics_handler(registry: prometheus::Registry) -> String {
    let encoder = TextEncoder::new();
    let metric_families = registry.gather();
    encoder.encode_to_string(&metric_families).unwrap_or_default()
}

// ===================================================================
// METRICS RECORDER
// ===================================================================

pub struct MetricsRecorder {
    metrics: Arc<PacyteMetrics>,
    node_id: String,
}

impl MetricsRecorder {
    pub fn new(metrics: Arc<PacyteMetrics>, node_id: u64) -> Self {
        Self {
            metrics,
            node_id: node_id.to_string(),
        }
    }
    
    pub fn record_block(&self, height: u64, time_secs: f64, size_bytes: u64, is_validator: bool) {
        self.metrics.block_height
            .with_label_values(&[&self.node_id])
            .set(height as f64);
        
        self.metrics.block_time_seconds
            .with_label_values(&[&self.node_id])
            .observe(time_secs);
        
        self.metrics.block_size_bytes
            .with_label_values(&[&self.node_id])
            .observe(size_bytes as f64);
        
        if is_validator {
            self.metrics.blocks_produced_total
                .with_label_values(&[&self.node_id, "true"])
                .inc();
        }
    }
    
    pub fn record_transaction(&self, tx_type: &str, size_bytes: usize, gas_used: u64, fee: u128) {
        self.metrics.transactions_total
            .with_label_values(&[&self.node_id, tx_type])
            .inc();
        
        self.metrics.transaction_size_bytes
            .with_label_values(&[&self.node_id])
            .observe(size_bytes as f64);
        
        self.metrics.transaction_gas_used
            .with_label_values(&[&self.node_id])
            .observe(gas_used as f64);
        
        self.metrics.transaction_fee
            .with_label_values(&[&self.node_id])
            .observe(fee as f64 / 1_000_000.0); // microPAC -> PAC
    }
    
    pub fn record_peer(&self, count: usize) {
        self.metrics.peers_connected
            .with_label_values(&[&self.node_id])
            .set(count as f64);
    }
    
    pub fn record_peer_message(&self, direction: &str, msg_type: &str) {
        self.metrics.peer_messages_total
            .with_label_values(&[&self.node_id, direction, msg_type])
            .inc();
    }
    
    pub fn record_consensus_round(&self, height: u64, round: u64) {
        self.metrics.consensus_round
            .with_label_values(&[&self.node_id, &height.to_string()])
            .set(round as f64);
    }
    
    pub fn record_consensus_proposal(&self, accepted: bool) {
        let result = if accepted { "accepted" } else { "rejected" };
        self.metrics.consensus_proposals_total
            .with_label_values(&[&self.node_id, result])
            .inc();
    }
    
    pub fn record_consensus_vote(&self, vote_type: &str) {
        self.metrics.consensus_votes_total
            .with_label_values(&[&self.node_id, vote_type])
            .inc();
    }
    
    pub fn record_consensus_timeout(&self, phase: &str) {
        self.metrics.consensus_timeouts_total
            .with_label_values(&[&self.node_id, phase])
            .inc();
    }
    
    pub fn record_mempool(&self, size: usize, bytes: usize) {
        self.metrics.mempool_size
            .with_label_values(&[&self.node_id])
            .set(size as f64);
        
        self.metrics.mempool_bytes
            .with_label_values(&[&self.node_id])
            .set(bytes as f64);
    }
    
    pub fn record_api_request(&self, method: &str, path: &str, status: u16, duration_secs: f64) {
        self.metrics.api_requests_total
            .with_label_values(&[&self.node_id, method, path, &status.to_string()])
            .inc();
        
        self.metrics.api_request_duration
            .with_label_values(&[&self.node_id, method, path])
            .observe(duration_secs);
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = PacyteMetrics::new();
        assert!(metrics.is_ok());
    }
    
    #[test]
    fn test_metrics_recorder() {
        let metrics = Arc::new(PacyteMetrics::new().unwrap());
        let recorder = MetricsRecorder::new(metrics, 1);
        
        recorder.record_block(100, 1.5, 50000, true);
        recorder.record_transaction("transfer", 250, 21000, 1000);
        recorder.record_peer(5);
        recorder.record_mempool(100, 50000);
    }
}