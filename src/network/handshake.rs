// ===================================================================
// PACYTE NEXUS - HANDSHAKE PROTOKOLÜ
// Bölüm 15 - Dosya 15.2: src/network/handshake.rs
// ===================================================================

use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::{Hash, PacyteError, PacyteResult};

/// El sıkışma sırasında değiş tokuş edilen veri paketi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeMessage {
    /// Protokol versiyonu
    pub protocol_version: u32,
    /// Ağ ID'si (mainnet/testnet ayırt etmek için)
    pub network_id: u32,
    /// Node'un kendisi için seçtiği benzersiz ID
    pub node_id: u64,
    /// Node'un dinlediği port
    pub port: u16,
    /// Genesis bloğunun hash'i (Aynı ağda olduğumuzu doğrulamak için)
    pub genesis_hash: Hash,
    /// Node'un sahip olduğu en yüksek blok yüksekliği
    pub best_height: u64,
    /// Node'un sahip olduğu en yüksek bloğun hash'i
    pub best_hash: Hash,
    /// Node'un desteklediği yetenekler (ör: "full", "archive")
    pub capabilities: Vec<String>,
    /// Mesajın oluşturulma zamanı (replay attack önlemi)
    pub timestamp: u64,
    /// Bu mesajın imzası (node'un private key'i ile)
    pub signature: Vec<u8>,
}

impl HandshakeMessage {
    pub fn new(
        node_id: u64,
        port: u16,
        genesis_hash: Hash,
        best_height: u64,
        best_hash: Hash,
    ) -> Self {
        Self {
            protocol_version: 1,
            network_id: 1, // 1 = mainnet, 2 = testnet
            node_id,
            port,
            genesis_hash,
            best_height,
            best_hash,
            capabilities: vec!["full".to_string(), "titan".to_string()],
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            signature: Vec::new(),
        }
    }

    /// İmzalanacak verinin hash'ini hesaplar (imza hariç)
    pub fn signing_hash(&self) -> Hash {
        let mut hasher = Sha3_256::new();
        hasher.update(&self.protocol_version.to_le_bytes());
        hasher.update(&self.network_id.to_le_bytes());
        hasher.update(&self.node_id.to_le_bytes());
        hasher.update(&self.port.to_le_bytes());
        hasher.update(&self.genesis_hash);
        hasher.update(&self.best_height.to_le_bytes());
        hasher.update(&self.best_hash);
        hasher.update(&self.timestamp.to_le_bytes());
        // capabilities değişebileceği için onları da ekle
        for cap in &self.capabilities {
            hasher.update(cap.as_bytes());
        }
        hasher.finalize().into()
    }

    /// Mesajın geçerliliğini kontrol eder (zaman aşımı, genesis uyumu)
    pub fn validate(&self, our_genesis_hash: Hash, max_time_diff_secs: u64) -> PacyteResult<()> {
        // Genesis hash uyuşmazlığı
        if self.genesis_hash != our_genesis_hash {
            return Err(PacyteError::HandshakeFailed(
                "Genesis hash mismatch".to_string()
            ));
        }

        // Zaman aşımı kontrolü
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        if self.timestamp > now + max_time_diff_secs || self.timestamp < now - max_time_diff_secs {
            return Err(PacyteError::HandshakeFailed(
                "Timestamp out of range".to_string()
            ));
        }

        Ok(())
    }
}

/// El sıkışma cevabı
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeAck {
    /// Kabul edildi mi?
    pub accepted: bool,
    /// Reddedilme sebebi (opsiyonel)
    pub reason: Option<String>,
    /// Karşı tarafa atanan peer ID (bizim sistemimizdeki)
    pub assigned_peer_id: Option<u64>,
}

impl HandshakeAck {
    pub fn accept(peer_id: u64) -> Self {
        Self {
            accepted: true,
            reason: None,
            assigned_peer_id: Some(peer_id),
        }
    }

    pub fn reject(reason: String) -> Self {
        Self {
            accepted: false,
            reason: Some(reason),
            assigned_peer_id: None,
        }
    }
}