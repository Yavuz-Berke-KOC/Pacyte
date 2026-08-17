// ===================================================================
// PACYTE NEXUS - PEER YÖNETİMİ
// ===================================================================

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tokio::sync::mpsc;

use crate::types::{PacyteError, PacyteResult, BlockHeight, Hash};
use super::message::{NetworkMessage, PeerInfo, HandshakeData};

// ===================================================================
// PEER
// ===================================================================

#[derive(Debug)]
pub struct Peer {
    pub id: u64,
    pub address: SocketAddr,
    pub info: PeerInfo,
    pub connection: PeerConnection,
    pub state: PeerState,
    pub stats: PeerStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    Connecting,
    Handshaking,
    Connected,
    Disconnected,
    Banned,
}

#[derive(Debug, Default)]
pub struct PeerStats {
    pub connected_at: Option<Instant>,
    pub last_message_at: Option<Instant>,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub ping_latency_ms: u64,
    pub failed_pings: u32,
}

impl Peer {
    pub fn new(id: u64, address: SocketAddr, connection: PeerConnection) -> Self {
        Self {
            id,
            address,
            info: PeerInfo {
                id,
                address: address.ip().to_string(),
                port: address.port(),
                best_height: 0,
                best_hash: [0u8; 32],
                capabilities: Vec::new(),
                connected_since: crate::types::current_timestamp(),
                last_seen: crate::types::current_timestamp(),
                latency_ms: 0,
            },
            connection,
            state: PeerState::Connecting,
            stats: PeerStats::default(),
        }
    }
    
    pub fn is_connected(&self) -> bool {
        matches!(self.state, PeerState::Connected)
    }
    
    pub fn update_info(&mut self, handshake: &HandshakeData) {
        self.info.best_height = handshake.best_height;
        self.info.best_hash = handshake.best_hash;
        self.info.capabilities = handshake.capabilities.clone();
    }
    
    pub fn record_message_sent(&mut self, bytes: usize) {
        self.stats.messages_sent += 1;
        self.stats.bytes_sent += bytes as u64;
        self.stats.last_message_at = Some(Instant::now());
    }
    
    pub fn record_message_received(&mut self, bytes: usize) {
        self.stats.messages_received += 1;
        self.stats.bytes_received += bytes as u64;
        self.stats.last_message_at = Some(Instant::now());
        self.info.last_seen = crate::types::current_timestamp();
    }
}

// ===================================================================
// PEER CONNECTION
// ===================================================================

#[derive(Debug)]
pub struct PeerConnection {
    pub sender: mpsc::UnboundedSender<NetworkMessage>,
    pub receiver: mpsc::UnboundedReceiver<NetworkMessage>,
}

impl PeerConnection {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            sender: tx,
            receiver: rx,
        }
    }
    
    pub fn send(&self, message: NetworkMessage) -> Result<(), mpsc::error::SendError<NetworkMessage>> {
        self.sender.send(message)
    }
}

// ===================================================================
// PEER MANAGER
// ===================================================================

pub struct PeerManager {
    peers: Arc<RwLock<HashMap<u64, Peer>>>,
    address_to_id: Arc<RwLock<HashMap<SocketAddr, u64>>>,
    next_peer_id: Arc<RwLock<u64>>,
    max_peers: usize,
    banned_peers: Arc<RwLock<HashMap<SocketAddr, Instant>>>,
}

impl PeerManager {
    pub fn new(max_peers: usize) -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            address_to_id: Arc::new(RwLock::new(HashMap::new())),
            next_peer_id: Arc::new(RwLock::new(1)),
            max_peers,
            banned_peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn add_peer(&self, address: SocketAddr, connection: PeerConnection) -> PacyteResult<u64> {
        // Ban kontrolü
        if self.is_banned(&address) {
            return Err(PacyteError::PeerBanned(address.to_string()));
        }
        
        // Zaten bağlı mı?
        {
            let addr_map = self.address_to_id.read();
            if addr_map.contains_key(&address) {
                return Err(PacyteError::PeerAlreadyConnected(address.to_string()));
            }
        }
        
        // Kapasite kontrolü
        {
            let peers = self.peers.read();
            if peers.len() >= self.max_peers {
                return Err(PacyteError::MaxPeersReached(self.max_peers));
            }
        }
        
        let peer_id = {
            let mut next_id = self.next_peer_id.write();
            let id = *next_id;
            *next_id += 1;
            id
        };
        
        let peer = Peer::new(peer_id, address, connection);
        
        {
            let mut peers = self.peers.write();
            peers.insert(peer_id, peer);
        }
        
        {
            let mut addr_map = self.address_to_id.write();
            addr_map.insert(address, peer_id);
        }
        
        Ok(peer_id)
    }
    
    pub fn remove_peer(&self, peer_id: u64) -> Option<Peer> {
        let peer = {
            let mut peers = self.peers.write();
            peers.remove(&peer_id)
        };
        
        if let Some(ref peer) = peer {
            let mut addr_map = self.address_to_id.write();
            addr_map.remove(&peer.address);
        }
        
        peer
    }
    
    pub fn get_peer(&self, peer_id: u64) -> Option<Peer> {
        self.peers.read().get(&peer_id).map(|p| {
            // Clone yapmadan referans döndüremeyiz, bu yüzden bilgi kopyası
            Peer {
                id: p.id,
                address: p.address,
                info: p.info.clone(),
                connection: PeerConnection::new(), // Placeholder
                state: p.state,
                stats: PeerStats::default(), // Placeholder
            }
        })
    }
    
    pub fn get_peer_by_address(&self, address: &SocketAddr) -> Option<u64> {
        self.address_to_id.read().get(address).copied()
    }
    
    pub fn update_peer_state(&self, peer_id: u64, state: PeerState) -> PacyteResult<()> {
        let mut peers = self.peers.write();
        if let Some(peer) = peers.get_mut(&peer_id) {
            peer.state = state;
            if state == PeerState::Connected {
                peer.stats.connected_at = Some(Instant::now());
            }
            Ok(())
        } else {
            Err(PacyteError::PeerNotFound(peer_id.to_string()))
        }
    }
    
    pub fn get_all_peers(&self) -> Vec<PeerInfo> {
        self.peers.read()
            .values()
            .map(|p| p.info.clone())
            .collect()
    }
    
    pub fn get_connected_peers(&self) -> Vec<PeerInfo> {
        self.peers.read()
            .values()
            .filter(|p| p.is_connected())
            .map(|p| p.info.clone())
            .collect()
    }
    
    pub fn peer_count(&self) -> usize {
        self.peers.read().len()
    }
    
    pub fn connected_count(&self) -> usize {
        self.peers.read()
            .values()
            .filter(|p| p.is_connected())
            .count()
    }
    
    pub fn ban_peer(&self, address: SocketAddr, duration: Duration) {
        self.banned_peers.write().insert(address, Instant::now() + duration);
        
        // Bağlıysa kes
        if let Some(peer_id) = self.get_peer_by_address(&address) {
            self.remove_peer(peer_id);
        }
    }
    
    pub fn is_banned(&self, address: &SocketAddr) -> bool {
        let banned = self.banned_peers.read();
        if let Some(until) = banned.get(address) {
            if Instant::now() < *until {
                return true;
            }
        }
        false
    }
    
    pub fn cleanup_banned(&self) {
        let now = Instant::now();
        self.banned_peers.write().retain(|_, until| now < *until);
    }
    
    pub fn get_random_peers(&self, count: usize) -> Vec<PeerInfo> {
        use rand::seq::IteratorRandom;
        
        let peers = self.peers.read();
        peers.values()
            .filter(|p| p.is_connected())
            .map(|p| p.info.clone())
            .choose_multiple(&mut rand::thread_rng(), count)
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_manager() {
        let manager = PeerManager::new(10);
        
        let addr: SocketAddr = "127.0.0.1:9333".parse().unwrap();
        let conn = PeerConnection::new();
        
        let peer_id = manager.add_peer(addr, conn).unwrap();
        assert_eq!(manager.peer_count(), 1);
        
        manager.update_peer_state(peer_id, PeerState::Connected).unwrap();
        assert_eq!(manager.connected_count(), 1);
        
        let peers = manager.get_connected_peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].id, peer_id);
        
        manager.remove_peer(peer_id);
        assert_eq!(manager.peer_count(), 0);
    }
    
    #[test]
    fn test_ban_peer() {
        let manager = PeerManager::new(10);
        
        let addr: SocketAddr = "127.0.0.1:9333".parse().unwrap();
        
        assert!(!manager.is_banned(&addr));
        
        manager.ban_peer(addr, Duration::from_secs(60));
        assert!(manager.is_banned(&addr));
    }
}