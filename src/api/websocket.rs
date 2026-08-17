// ===================================================================
// PACYTE NEXUS - WEBSOCKET API
// ===================================================================

use std::sync::Arc;
use std::net::SocketAddr;
use std::collections::HashMap;
use parking_lot::RwLock;
use tokio::sync::{broadcast, mpsc};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::{tungstenite::protocol::Message, WebSocketStream};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::types::{PacyteError, PacyteResult, Hash, BlockHeight, Address};
use crate::types::block::Block;
use crate::types::transaction::Transaction;
use crate::storage::Storage;
use crate::mempool::Mempool;
use crate::consensus::{Consensus, ConsensusEvent};
use crate::network::Network;
use crate::network::message::NetworkMessage;
use crate::network::PeerMessage;

use super::ApiConfig;

// ===================================================================
// WEBSOCKET SERVER
// ===================================================================

pub struct WebSocketServer {
    config: ApiConfig,
    storage: Arc<dyn Storage>,
    mempool: Arc<dyn Mempool>,
    consensus: Arc<dyn Consensus>,
    network: Arc<dyn Network>,
    
    connections: Arc<RwLock<HashMap<u64, mpsc::UnboundedSender<WsMessage>>>>,
    next_conn_id: Arc<RwLock<u64>>,
    shutdown_tx: Option<broadcast::Sender<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    // Client -> Server
    Subscribe { topics: Vec<String> },
    Unsubscribe { topics: Vec<String> },
    Ping,
    
    // Server -> Client
    Subscribed { topics: Vec<String> },
    Unsubscribed { topics: Vec<String> },
    Pong,
    
    // Events
    NewBlock { block: Block },
    NewTransaction { transaction: Transaction },
    ConsensusEvent { event: String, data: serde_json::Value },
    PeerEvent { event: String, peer_id: u64 },
    Error { message: String },
}

impl WebSocketServer {
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
            connections: Arc::new(RwLock::new(HashMap::new())),
            next_conn_id: Arc::new(RwLock::new(1)),
            shutdown_tx: None,
        }
    }
    
    pub async fn start(&mut self) -> PacyteResult<()> {
        let addr = self.config.ws_addr;
        let (shutdown_tx, _) = broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx.clone());
        
        tracing::info!("WebSocket API listening on ws://{}", addr);
        
        let listener = tokio::net::TcpListener::bind(addr).await
            .map_err(|e| PacyteError::NetworkError(e.to_string()))?;
        
        let server = Arc::new(self.clone_server());
        
        // Event broadcast task'ini başlat
        let event_server = Arc::clone(&server);
        tokio::spawn(async move {
            event_server.broadcast_events().await;
        });
        
        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_tx.subscribe();
            
            loop {
                tokio::select! {
                    Ok((stream, addr)) = listener.accept() => {
                        let server = Arc::clone(&server);
                        tokio::spawn(async move {
                            server.handle_connection(stream, addr).await;
                        });
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });
        
        Ok(())
    }
    
    pub async fn stop(&self) -> PacyteResult<()> {
        if let Some(tx) = &self.shutdown_tx {
            let _ = tx.send(());
        }
        Ok(())
    }
    
    fn clone_server(&self) -> Self {
        Self {
            config: self.config.clone(),
            storage: self.storage.clone(),
            mempool: self.mempool.clone(),
            consensus: self.consensus.clone(),
            network: self.network.clone(),
            connections: self.connections.clone(),
            next_conn_id: self.next_conn_id.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
        }
    }
    
    async fn handle_connection(&self, stream: tokio::net::TcpStream, addr: SocketAddr) {
        let ws_stream = match tokio_tungstenite::accept_async(stream).await {
            Ok(ws) => ws,
            Err(e) => {
                tracing::debug!("WebSocket handshake failed: {}", e);
                return;
            }
        };
        
        let conn_id = {
            let mut next = self.next_conn_id.write();
            let id = *next;
            *next += 1;
            id
        };
        
        tracing::info!("WebSocket client connected: {} (id={})", addr, conn_id);
        
        let (tx, mut rx) = mpsc::unbounded_channel();
        
        {
            let mut connections = self.connections.write();
            connections.insert(conn_id, tx);
        }
        
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        
        // Mesaj gönderme task'i
        let mut msg_rx = rx;
        let send_task = tokio::spawn(async move {
            while let Some(msg) = msg_rx.recv().await {
                let json = serde_json::to_string(&msg).unwrap_or_default();
                if ws_sender.send(Message::Text(json)).await.is_err() {
    		    break;
		}
            }
        });
        
        // Mesaj alma döngüsü
        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                        self.handle_message(conn_id, ws_msg).await;
                    }
                }
                //Message::Ping(data) => {
                    //let _ = ws_sender.send(Message::Pong(data)).await;
                //}
                Message::Close(_) => break,
                _ => {}
            }
        }
        
        send_task.abort();
        
        {
            let mut connections = self.connections.write();
            connections.remove(&conn_id);
        }
        
        tracing::info!("WebSocket client disconnected: {} (id={})", addr, conn_id);
    }
    
    async fn handle_message(&self, conn_id: u64, msg: WsMessage) {
        match msg {
            WsMessage::Subscribe { topics } => {
                tracing::debug!("Client {} subscribed to: {:?}", conn_id, topics);
                self.send_to(conn_id, WsMessage::Subscribed { topics }).await;
            }
            WsMessage::Unsubscribe { topics } => {
                self.send_to(conn_id, WsMessage::Unsubscribed { topics }).await;
            }
            WsMessage::Ping => {
                self.send_to(conn_id, WsMessage::Pong).await;
            }
            _ => {}
        }
    }
    
    async fn send_to(&self, conn_id: u64, msg: WsMessage) {
        let connections = self.connections.read();
        if let Some(tx) = connections.get(&conn_id) {
            let _ = tx.send(msg);
        }
    }
    
    async fn broadcast(&self, msg: WsMessage) {
        let connections = self.connections.read();
        for tx in connections.values() {
            let _ = tx.send(msg.clone());
        }
    }
    
    async fn broadcast_events(&self) {
        let mut consensus_rx = self.consensus.subscribe_events();
        let mut network_rx = self.network.subscribe();
        
        let mut block_height = self.storage.get_block_height().await.unwrap_or(0);
        
        loop {
            tokio::select! {
                Some(event) = consensus_rx.recv() => {
                    match event {
                        ConsensusEvent::BlockCommitted { block, height } => {
                            self.broadcast(WsMessage::NewBlock { block }).await;
                            block_height = height;
                        }
                        ConsensusEvent::NewRound { height, round } => {
                            self.broadcast(WsMessage::ConsensusEvent {
                                event: "new_round".to_string(),
                                data: json!({ "height": height, "round": round }),
                            }).await;
                        }
                        ConsensusEvent::QuorumReached { height, round, vote_type } => {
                            self.broadcast(WsMessage::ConsensusEvent {
                                event: "quorum_reached".to_string(),
                                data: json!({ 
                                    "height": height, 
                                    "round": round,
                                    "vote_type": format!("{:?}", vote_type)
                                }),
                            }).await;
                        }
                        _ => {}
                    }
                }
                
                Some(peer_msg) = network_rx.recv() => {
                    match peer_msg.message {
                        NetworkMessage::NewTransaction(tx) => {
                            self.broadcast(WsMessage::NewTransaction { transaction: tx }).await;
                        }
                        NetworkMessage::PeerConnected(info) => {
                            self.broadcast(WsMessage::PeerEvent {
                                event: "connected".to_string(),
                                peer_id: info.id,
                            }).await;
                        }
                        NetworkMessage::PeerDisconnected(id) => {
                            self.broadcast(WsMessage::PeerEvent {
                                event: "disconnected".to_string(),
                                peer_id: id,
                            }).await;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}