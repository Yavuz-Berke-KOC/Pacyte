// ===================================================================
// PACYTE NEXUS - NETWORK MODÜLÜ
// ===================================================================

pub mod p2p;
pub mod peer;
pub mod message;
pub mod gossip;
pub mod handshake;

// Re-export'lar
pub use p2p::*;
//pub use peer::*;
//pub use message::*;
pub use gossip::*;
pub use handshake::*;

use crate::network::message::{NetworkMessage,PeerInfo};
use crate::types::{PacyteError, PacyteResult};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

// ===================================================================
// NETWORK TRAIT
// ===================================================================

#[async_trait::async_trait]
pub trait Network: Send + Sync {
    /// Ağı başlat
    async fn start(&self) -> PacyteResult<()>;
    
    /// Ağı durdur
    async fn stop(&self) -> PacyteResult<()>;
    
    /// Mesaj gönder (broadcast)
    async fn broadcast(&self, message: NetworkMessage) -> PacyteResult<()>;
    
    /// Belirli bir peer'a mesaj gönder
    async fn send_to(&self, peer_id: u64, message: NetworkMessage) -> PacyteResult<()>;
    
    /// Peer bağlantısı kur
    async fn connect(&self, addr: SocketAddr) -> PacyteResult<()>;
    
    /// Peer bağlantısını kes
    async fn disconnect(&self, peer_id: u64) -> PacyteResult<()>;
    
    /// Bağlı peer'ları getir
    fn connected_peers(&self) -> Vec<PeerInfo>;
    
    /// Peer sayısı
    fn peer_count(&self) -> usize;
    
    /// Mesaj kanalını al
    fn subscribe(&self) -> mpsc::UnboundedReceiver<PeerMessage>;
}

// ===================================================================
// AĞ MESAJLARI
// ===================================================================

#[derive(Debug, Clone)]
pub struct PeerMessage {
    pub from: u64,
    pub message: NetworkMessage,
    pub received_at: u64,
}

// ===================================================================
// AĞ KONFİGÜRASYONU
// ===================================================================

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub node_id: u64,
    pub listen_addr: SocketAddr,
    pub public_addr: Option<SocketAddr>,
    pub bootstrap_peers: Vec<SocketAddr>,
    pub max_peers: usize,
    pub min_peers: usize,
    pub handshake_timeout_ms: u64,
    pub ping_interval_ms: u64,
    pub max_message_size: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            node_id: 1,
            listen_addr: "0.0.0.0:9333".parse().unwrap(),
            public_addr: None,
            bootstrap_peers: Vec::new(),
            max_peers: 50,
            min_peers: 3,
            handshake_timeout_ms: 5000,
            ping_interval_ms: 30000,
            max_message_size: 16 * 1024 * 1024, // 16 MB
        }
    }
}