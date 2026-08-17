// ===================================================================
// PACYTE NEXUS - P2P TCP SUNUCUSU (GERÇEK)
// ===================================================================

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::{mpsc, broadcast};
use parking_lot::RwLock;
use futures::StreamExt;

use crate::types::{PacyteError, PacyteResult, Hash, BlockHeight};
use super::{
    Network, NetworkConfig, PeerMessage,
    peer::{PeerManager, PeerConnection, PeerState}, 
    message::{PeerInfo,HandshakeData, HandshakeAck,NetworkMessage},
};

// ===================================================================
// P2P NETWORK
// ===================================================================

pub struct P2PNetwork {
    config: NetworkConfig,
    peer_manager: Arc<PeerManager>,
    message_tx: mpsc::UnboundedSender<PeerMessage>,
    message_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<PeerMessage>>>>,
    shutdown_tx: broadcast::Sender<()>,
    state: Arc<RwLock<NetworkState>>,
    genesis_hash: Hash,
    best_height: Arc<RwLock<BlockHeight>>,
    best_hash: Arc<RwLock<Hash>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkState {
    Stopped,
    Starting,
    Running,
    Stopping,
}

impl P2PNetwork {
    pub fn new(
        config: NetworkConfig,
        genesis_hash: Hash,
    ) -> Self {
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, _) = broadcast::channel(1);
        
        Self {
            config: config.clone(),
            peer_manager: Arc::new(PeerManager::new(config.max_peers)),
            message_tx,
            message_rx: Arc::new(RwLock::new(Some(message_rx))),
            shutdown_tx,
            state: Arc::new(RwLock::new(NetworkState::Stopped)),
            genesis_hash,
            best_height: Arc::new(RwLock::new(0)),
            best_hash: Arc::new(RwLock::new([0u8; 32])),
        }
    }
    
    pub fn update_best_block(&self, height: BlockHeight, hash: Hash) {
        *self.best_height.write() = height;
        *self.best_hash.write() = hash;
    }
    
    async fn handle_connection(
        &self,
        mut stream: TcpStream,
        addr: SocketAddr,
    ) -> PacyteResult<()> {
        let (reader, writer) = stream.split();
        let mut reader = BufReader::new(reader);
        let mut writer = BufWriter::new(writer);
        
        // Peer connection oluştur
        let mut connection = PeerConnection::new();
	let mut receiver = std::mem::replace(&mut connection.receiver, 
	    {let (_, rx) = mpsc::unbounded_channel();
	    rx
	});
        let peer_id = self.peer_manager.add_peer(addr, connection)?;
        
        // Handshake yap
        let handshake = self.perform_handshake(&mut reader, &mut writer, peer_id).await?;
        
        // Peer'ı bağlı olarak işaretle
        self.peer_manager.update_peer_state(peer_id, PeerState::Connected)?;
        
        tracing::info!("Peer {} connected: {} (height={})", 
            peer_id, addr, handshake.best_height);
        
        // Mesaj döngüsü
        let mut buffer = vec![0u8; self.config.max_message_size];
        
        loop {
	    let mut shutdown_rx = self.shutdown_tx.subscribe();
            tokio::select! {
                // Okuma
                result = reader.read(&mut buffer) => {
                    match result {
                        Ok(0) => break, // Bağlantı kapandı
                        Ok(n) => {
                            if let Some(msg) = NetworkMessage::from_bytes(&buffer[..n]) {
                                self.handle_message(peer_id, msg).await?;
                            }
                        }
                        Err(e) => {
                            tracing::debug!("Read error from peer {}: {}", peer_id, e);
                            break;
                        }
                    }
                }
                
                // Yazma (peer'a mesaj gönderme)
                Some(msg) = receiver.recv() => {
                    let bytes = msg.to_bytes();
                    if let Err(e) = writer.write_all(&bytes).await {
                        tracing::debug!("Write error to peer {}: {}", peer_id, e);
                        break;
                    }
                    writer.flush().await.ok();
                }
                
                // Shutdown
                _ = shutdown_rx.recv() => {
                    break;
                }
            }
        }
        
        // Bağlantıyı temizle
        self.peer_manager.remove_peer(peer_id);
        tracing::info!("Peer {} disconnected", peer_id);
        
        Ok(())
    }
    
    async fn perform_handshake(
        &self,
        reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
        writer: &mut BufWriter<tokio::net::tcp::WriteHalf<'_>>,
        peer_id: u64,
    ) -> PacyteResult<HandshakeData> {
        // Handshake gönder
        let our_handshake = HandshakeData::new(
            self.config.node_id,
            self.config.listen_addr.port(),
            self.genesis_hash,
            *self.best_height.read(),
            *self.best_hash.read(),
        );
        
        let msg = NetworkMessage::Handshake(our_handshake.clone());
        writer.write_all(&msg.to_bytes()).await
            .map_err(|e| PacyteError::NetworkError(e.to_string()))?;
        writer.flush().await
            .map_err(|e| PacyteError::NetworkError(e.to_string()))?;
        
        // Karşı tarafın handshake'ini bekle
        let mut buffer = vec![0u8; 4096];
        let n = tokio::time::timeout(
            std::time::Duration::from_millis(self.config.handshake_timeout_ms),
            reader.read(&mut buffer)
        ).await
            .map_err(|_| PacyteError::HandshakeTimeout)?
            .map_err(|e| PacyteError::NetworkError(e.to_string()))?;
        
        if n == 0 {
            return Err(PacyteError::HandshakeFailed("Connection closed".to_string()));
        }
        
        let msg = NetworkMessage::from_bytes(&buffer[..n])
            .ok_or_else(|| PacyteError::HandshakeFailed("Invalid message".to_string()))?;
        
        match msg {
            NetworkMessage::Handshake(their_handshake) => {
                // Genesis kontrolü
                if their_handshake.genesis_hash != self.genesis_hash {
                    let ack = HandshakeAck {
                        accepted: false,
                        reason: Some("Genesis hash mismatch".to_string()),
                        peer_id: None,
                    };
                    writer.write_all(&NetworkMessage::HandshakeAck(ack).to_bytes()).await.ok();
                    return Err(PacyteError::HandshakeFailed("Genesis mismatch".to_string()));
                }
                
                // Kabul et
                let ack = HandshakeAck {
                    accepted: true,
                    reason: None,
                    peer_id: Some(peer_id),
                };
                writer.write_all(&NetworkMessage::HandshakeAck(ack).to_bytes()).await.ok();
                writer.flush().await.ok();
                
                Ok(their_handshake)
            }
            _ => Err(PacyteError::HandshakeFailed("Expected Handshake".to_string())),
        }
    }
    
    async fn handle_message(&self, peer_id: u64, msg: NetworkMessage) -> PacyteResult<()> {
        // Peer'a mesaj kaydet
        // (stats güncelleme)
        
        match msg {
            NetworkMessage::Ping(nonce) => {
                let _ = self.send_to(peer_id, NetworkMessage::Pong(nonce)).await;
            }
            
            NetworkMessage::Pong(nonce) => {
                // Latency hesapla
                tracing::trace!("Pong from {}: nonce={}", peer_id, nonce);
            }
            
            NetworkMessage::GetPeers => {
                let peers = self.peer_manager.get_connected_peers();
                let _ = self.send_to(peer_id, NetworkMessage::Peers(peers)).await;
            }
            
            NetworkMessage::NewBlock(ref block) => {
                // Üst katmana ilet
                let _ = self.message_tx.send(PeerMessage {
                    from: peer_id,
                    message: msg,
                    received_at: crate::types::current_timestamp(),
                });
            }
            
            NetworkMessage::NewTransaction(ref tx) => {
                let _ = self.message_tx.send(PeerMessage {
                    from: peer_id,
                    message: msg,
                    received_at: crate::types::current_timestamp(),
                });
            }
            
            _ => {
                // Diğer mesajları üst katmana ilet
                let _ = self.message_tx.send(PeerMessage {
                    from: peer_id,
                    message: msg,
                    received_at: crate::types::current_timestamp(),
                });
            }
        }
        
        Ok(())
    }
    
    async fn connect_to_bootstrap(&self) -> PacyteResult<()> {
        for addr in &self.config.bootstrap_peers {
            if self.peer_manager.connected_count() >= self.config.max_peers {
                break;
            }
            
            if let Err(e) = self.connect(*addr).await {
                tracing::warn!("Failed to connect to bootstrap {}: {}", addr, e);
            }
        }
        
        Ok(())
    }
    
    async fn maintain_connections(&self) {
        let mut interval = tokio::time::interval(
            std::time::Duration::from_millis(self.config.ping_interval_ms)
        );
        
        loop {
            interval.tick().await;
            
            // Minimum peer sayısını koru
            if self.peer_manager.connected_count() < self.config.min_peers {
                let _ = self.connect_to_bootstrap().await;
            }
            
            // Tüm peer'lara ping gönder
            for peer in self.peer_manager.get_connected_peers() {
                let nonce = rand::random();
                let _ = self.send_to(peer.id, NetworkMessage::Ping(nonce)).await;
            }
            
            // Ban listesini temizle
            self.peer_manager.cleanup_banned();
        }
    }
}

#[async_trait::async_trait]
impl Network for P2PNetwork {
    async fn start(&self) -> PacyteResult<()> {
        *self.state.write() = NetworkState::Starting;
        
        let listener = TcpListener::bind(&self.config.listen_addr).await
            .map_err(|e| PacyteError::NetworkError(e.to_string()))?;
        
        tracing::info!("P2P server listening on {}", self.config.listen_addr);
        
        *self.state.write() = NetworkState::Running;
        
        // Bootstrap peer'lara bağlan
        self.connect_to_bootstrap().await?;
        
        // Bağlantı yönetimi task'i
        let maintain_self = self.clone_network();
        tokio::spawn(async move {
            maintain_self.maintain_connections().await;
        });
        
        // Gelen bağlantıları kabul et
        loop {
            if *self.state.read() == NetworkState::Stopping {
                break;
            }
            
            let (stream, addr) = listener.accept().await
                .map_err(|e| PacyteError::NetworkError(e.to_string()))?;
            
            let self_clone = self.clone_network();
            tokio::spawn(async move {
                if let Err(e) = self_clone.handle_connection(stream, addr).await {
                    tracing::debug!("Connection from {} closed: {}", addr, e);
                }
            });
        }
        
        Ok(())
    }
    
    async fn stop(&self) -> PacyteResult<()> {
        *self.state.write() = NetworkState::Stopping;
        let _ = self.shutdown_tx.send(());
        *self.state.write() = NetworkState::Stopped;
        Ok(())
    }
    
    async fn broadcast(&self, message: NetworkMessage) -> PacyteResult<()> {
        let bytes = message.to_bytes();
        
        for peer in self.peer_manager.get_connected_peers() {
            let _ = self.send_to(peer.id, message.clone()).await;
        }
        
        Ok(())
    }
    
    async fn send_to(&self, peer_id: u64, message: NetworkMessage) -> PacyteResult<()> {
        if let Some(peer) = self.peer_manager.get_peer(peer_id) {
            peer.connection.send(message)
                .map_err(|_| PacyteError::PeerNotFound(peer_id.to_string()))?;
            Ok(())
        } else {
            Err(PacyteError::PeerNotFound(peer_id.to_string()))
        }
    }
    
    async fn connect(&self, addr: SocketAddr) -> PacyteResult<()> {
        let stream = TcpStream::connect(addr).await
            .map_err(|e| PacyteError::NetworkError(e.to_string()))?;
        
        let self_clone = self.clone_network();
        tokio::spawn(async move {
            let _ = self_clone.handle_connection(stream, addr).await;
        });
        
        Ok(())
    }
    
    async fn disconnect(&self, peer_id: u64) -> PacyteResult<()> {
        self.peer_manager.remove_peer(peer_id);
        Ok(())
    }
    
    fn connected_peers(&self) -> Vec<PeerInfo> {
        self.peer_manager.get_connected_peers()
    }
    
    fn peer_count(&self) -> usize {
        self.peer_manager.connected_count()
    }
    
    fn subscribe(&self) -> mpsc::UnboundedReceiver<PeerMessage> {
    let (tx, rx) = mpsc::unbounded_channel();
    
    // Mevcut receiver'dan gelen mesajları yeni channel'a yönlendir
    let existing_rx = self.message_rx.write().take();
    
    if let Some(mut existing) = existing_rx {
        tokio::spawn(async move {
            while let Some(msg) = existing.recv().await {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });
    }
    
    rx
}
}

impl P2PNetwork {
    fn clone_network(&self) -> Self {
        Self {
            config: self.config.clone(),
            peer_manager: self.peer_manager.clone(),
            message_tx: self.message_tx.clone(),
            message_rx: self.message_rx.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
            state: self.state.clone(),
            genesis_hash: self.genesis_hash,
            best_height: self.best_height.clone(),
            best_hash: self.best_hash.clone(),
        }
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_network_start_stop() {
        let config = NetworkConfig {
            node_id: 1,
            listen_addr: "127.0.0.1:19333".parse().unwrap(),
            ..Default::default()
        };
        
        let network = P2PNetwork::new(config, [0u8; 32]);
        
        let network_clone = network.clone_network();
        let handle = tokio::spawn(async move {
            network_clone.start().await.unwrap();
        });
        
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        
        network.stop().await.unwrap();
        
        timeout(std::time::Duration::from_secs(1), handle).await.ok();
    }
}