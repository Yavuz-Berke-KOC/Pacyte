// ===================================================================
// PACYTE NEXUS - AĞ MESAJLARI
// ===================================================================

use serde::{Deserialize, Serialize};

use crate::types::{Hash, BlockHeight, Address, Signature};
use crate::types::block::Block;
use crate::types::transaction::Transaction;

// ===================================================================
// ANA MESAJ TİPİ
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    // ========== El Sıkışma ==========
    Handshake(HandshakeData),
    HandshakeAck(HandshakeAck),
    
    // ========== Ping/Pong ==========
    Ping(u64), // Nonce
    Pong(u64), // Nonce
    
    // ========== Peer Yönetimi ==========
    GetPeers,
    Peers(Vec<PeerInfo>),
    PeerConnected(PeerInfo),
    PeerDisconnected(u64),
    
    // ========== Blok Senkronizasyonu ==========
    GetBlocks { from: BlockHeight, to: BlockHeight },
    Blocks(Vec<Block>),
    NewBlock(Block),
    GetBlockHeaders { from: BlockHeight, limit: u32 },
    BlockHeaders(Vec<BlockHeaderData>),
    
    // ========== İşlem Yayını ==========
    NewTransaction(Transaction),
    GetTransactions(Vec<Hash>),
    Transactions(Vec<Transaction>),
    
    // ========== Konsensüs ==========
    Proposal(ConsensusProposal),
    Vote(ConsensusVote),
    NewView { height: BlockHeight, round: u64 },
    Timeout { height: BlockHeight, round: u64 },
    
    // ========== State Senkronizasyonu ==========
    GetStateChunk { root: Hash, chunk_index: u32 },
    StateChunk { root: Hash, chunk_index: u32, data: Vec<u8> },
    
    // ========== Diğer ==========
    Error { code: u32, message: String },
}

// ===================================================================
// HANDSHAKE
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeData {
    pub version: String,
    pub protocol_version: u32,
    pub node_id: u64,
    pub port: u16,
    pub genesis_hash: Hash,
    pub best_height: BlockHeight,
    pub best_hash: Hash,
    pub capabilities: Vec<String>,
    pub timestamp: u64,
}

impl HandshakeData {
    pub fn new(
        node_id: u64,
        port: u16,
        genesis_hash: Hash,
        best_height: BlockHeight,
        best_hash: Hash,
    ) -> Self {
        Self {
            version: crate::VERSION.to_string(),
            protocol_version: 1,
            node_id,
            port,
            genesis_hash,
            best_height,
            best_hash,
            capabilities: vec!["full".to_string(), "titan".to_string()],
            timestamp: crate::types::current_timestamp(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeAck {
    pub accepted: bool,
    pub reason: Option<String>,
    pub peer_id: Option<u64>,
}

// ===================================================================
// PEER INFO
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: u64,
    pub address: String,
    pub port: u16,
    pub best_height: BlockHeight,
    pub best_hash: Hash,
    pub capabilities: Vec<String>,
    pub connected_since: u64,
    pub last_seen: u64,
    pub latency_ms: u64,
}

// ===================================================================
// BLOK BAŞLIĞI (SENKRONİZASYON İÇİN)
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeaderData {
    pub height: BlockHeight,
    pub hash: Hash,
    pub previous_hash: Hash,
    pub timestamp: u64,
    pub transaction_count: u32,
}

// ===================================================================
// KONSENSÜS MESAJLARI
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusProposal {
    pub height: BlockHeight,
    pub round: u64,
    pub block: Block,
    pub proposer: u64,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusVote {
    pub height: BlockHeight,
    pub round: u64,
    pub block_hash: Hash,
    pub voter: u64,
    pub vote_type: VoteType,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoteType {
    Prevote,
    Precommit,
}

// ===================================================================
// MESAJ KODLAMA
// ===================================================================

impl NetworkMessage {
    /// Mesajı byte dizisine çevir
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
    
    /// Byte dizisinden mesaj oluştur
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }
    
    /// Mesaj tipini string olarak döndür
    pub fn message_type(&self) -> &'static str {
        match self {
            Self::Handshake(_) => "Handshake",
            Self::HandshakeAck(_) => "HandshakeAck",
            Self::Ping(_) => "Ping",
            Self::Pong(_) => "Pong",
            Self::GetPeers => "GetPeers",
            Self::Peers(_) => "Peers",
            Self::PeerConnected(_) => "PeerConnected",
            Self::PeerDisconnected(_) => "PeerDisconnected",
            Self::GetBlocks { .. } => "GetBlocks",
            Self::Blocks(_) => "Blocks",
            Self::NewBlock(_) => "NewBlock",
            Self::GetBlockHeaders { .. } => "GetBlockHeaders",
            Self::BlockHeaders(_) => "BlockHeaders",
            Self::NewTransaction(_) => "NewTransaction",
            Self::GetTransactions(_) => "GetTransactions",
            Self::Transactions(_) => "Transactions",
            Self::Proposal(_) => "Proposal",
            Self::Vote(_) => "Vote",
            Self::NewView { .. } => "NewView",
            Self::Timeout { .. } => "Timeout",
            Self::GetStateChunk { .. } => "GetStateChunk",
            Self::StateChunk { .. } => "StateChunk",
            Self::Error { .. } => "Error",
        }
    }
    
    /// Mesaj önceliği (QoS için)
    pub fn priority(&self) -> u8 {
        match self {
            Self::Proposal(_) | Self::Vote(_) => 1, // En yüksek
            Self::NewBlock(_) | Self::NewTransaction(_) => 2,
            Self::Ping(_) | Self::Pong(_) => 3,
            _ => 4, // Normal
        }
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let msg = NetworkMessage::Ping(12345);
        let bytes = msg.to_bytes();
        let decoded = NetworkMessage::from_bytes(&bytes).unwrap();
        
        match decoded {
            NetworkMessage::Ping(nonce) => assert_eq!(nonce, 12345),
            _ => panic!("Wrong message type"),
        }
    }
    
    #[test]
    fn test_handshake_data() {
        let handshake = HandshakeData::new(
            1,
            9333,
            [0u8; 32],
            1000,
            [1u8; 32],
        );
        
        assert_eq!(handshake.node_id, 1);
        assert_eq!(handshake.port, 9333);
        assert_eq!(handshake.best_height, 1000);
    }
    
    #[test]
    fn test_message_type() {
        let msg = NetworkMessage::Ping(0);
        assert_eq!(msg.message_type(), "Ping");
        
        let msg = NetworkMessage::NewBlock(Block::genesis());
        assert_eq!(msg.message_type(), "NewBlock");
    }
}