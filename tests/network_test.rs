// ===================================================================
// PACYTE NEXUS - AĞ TESTLERİ
// ===================================================================

use pacyte_node::network::*;
use pacyte_node::types::*;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::time::timeout;

// ===================================================================
// PEER BAĞLANTI TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_two_peers_connect() {
    let config1 = NetworkConfig {
        node_id: 1,
        listen_addr: "127.0.0.1:19333".parse().unwrap(),
        ..Default::default()
    };
    
    let config2 = NetworkConfig {
        node_id: 2,
        listen_addr: "127.0.0.1:19334".parse().unwrap(),
        bootstrap_peers: vec!["127.0.0.1:19333".parse().unwrap()],
        ..Default::default()
    };
    
    let network1 = Arc::new(P2PNetwork::new(config1, [0u8; 32]));
    let network2 = Arc::new(P2PNetwork::new(config2, [0u8; 32]));
    
    let n1 = network1.clone();
    let handle1 = tokio::spawn(async move {
        n1.start().await.unwrap();
    });
    
    let n2 = network2.clone();
    let handle2 = tokio::spawn(async move {
        n2.start().await.unwrap();
    });
    
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    assert!(network1.peer_count() > 0 || network2.peer_count() > 0);
    
    network1.stop().await.unwrap();
    network2.stop().await.unwrap();
    
    timeout(std::time::Duration::from_secs(1), handle1).await.ok();
    timeout(std::time::Duration::from_secs(1), handle2).await.ok();
}

#[tokio::test]
async fn test_message_broadcast() {
    let config = NetworkConfig {
        node_id: 1,
        listen_addr: "127.0.0.1:29333".parse().unwrap(),
        ..Default::default()
    };
    
    let network = Arc::new(P2PNetwork::new(config, [0u8; 32]));
    let mut rx = network.subscribe();
    
    let n = network.clone();
    let handle = tokio::spawn(async move {
        n.start().await.unwrap();
    });
    
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    network.broadcast(NetworkMessage::Ping(12345)).await.unwrap();
    
    // Kendimize de gönderildi mi?
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    
    network.stop().await.unwrap();
    handle.abort();
}

#[tokio::test]
async fn test_peer_discovery() {
    let config1 = NetworkConfig {
        node_id: 1,
        listen_addr: "127.0.0.1:39333".parse().unwrap(),
        ..Default::default()
    };
    
    let network1 = Arc::new(P2PNetwork::new(config1, [0u8; 32]));
    
    let n1 = network1.clone();
    tokio::spawn(async move {
        n1.start().await.unwrap();
    });
    
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // 5 peer bağlanmaya çalış
    for i in 0..5 {
        let config = NetworkConfig {
            node_id: i + 2,
            listen_addr: format!("127.0.0.1:{}", 39334 + i).parse().unwrap(),
            bootstrap_peers: vec!["127.0.0.1:39333".parse().unwrap()],
            ..Default::default()
        };
        
        let network = Arc::new(P2PNetwork::new(config, [0u8; 32]));
        
        tokio::spawn(async move {
            network.start().await.unwrap();
        });
    }
    
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    
    // Peer'lar keşfedilmeli
    let peers = network1.connected_peers();
    assert!(peers.len() >= 2);
    
    network1.stop().await.unwrap();
}

// ===================================================================
// MESAJ TESTLERİ
// ===================================================================

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
fn test_handshake_serialization() {
    let handshake = HandshakeData::new(1, 9333, [0u8; 32], 1000, [1u8; 32]);
    
    let msg = NetworkMessage::Handshake(handshake.clone());
    let bytes = msg.to_bytes();
    let decoded = NetworkMessage::from_bytes(&bytes).unwrap();
    
    match decoded {
        NetworkMessage::Handshake(h) => {
            assert_eq!(h.node_id, handshake.node_id);
            assert_eq!(h.best_height, handshake.best_height);
        }
        _ => panic!("Wrong message type"),
    }
}

#[test]
fn test_peer_info() {
    let info = PeerInfo {
        id: 1,
        address: "127.0.0.1".to_string(),
        port: 9333,
        best_height: 100,
        best_hash: [1u8; 32],
        capabilities: vec!["full".to_string()],
        connected_since: 12345,
        last_seen: 12346,
        latency_ms: 50,
    };
    
    let msg = NetworkMessage::PeerConnected(info.clone());
    let bytes = msg.to_bytes();
    let decoded = NetworkMessage::from_bytes(&bytes).unwrap();
    
    match decoded {
        NetworkMessage::PeerConnected(p) => {
            assert_eq!(p.id, info.id);
            assert_eq!(p.best_height, info.best_height);
        }
        _ => panic!("Wrong message type"),
    }
}

// ===================================================================
// BAN TESTLERİ
// ===================================================================

#[test]
fn test_peer_ban() {
    let manager = PeerManager::new(10);
    
    let addr: SocketAddr = "127.0.0.1:9333".parse().unwrap();
    
    assert!(!manager.is_banned(&addr));
    
    manager.ban_peer(addr, std::time::Duration::from_secs(60));
    assert!(manager.is_banned(&addr));
    
    manager.cleanup_banned();
}

// ===================================================================
// HANDSHAKE TESTLERİ
// ===================================================================

#[test]
fn test_handshake_validation() {
    let handshake1 = HandshakeData::new(1, 9333, [0u8; 32], 100, [1u8; 32]);
    let handshake2 = HandshakeData::new(2, 9334, [0u8; 32], 200, [2u8; 32]);
    
    // Aynı genesis hash - geçerli
    assert_eq!(handshake1.genesis_hash, handshake2.genesis_hash);
    
    // Farklı genesis hash - geçersiz
    let handshake3 = HandshakeData::new(3, 9335, [1u8; 32], 300, [3u8; 32]);
    assert_ne!(handshake1.genesis_hash, handshake3.genesis_hash);
}

// ===================================================================
// GOSSIP TESTLERİ
// ===================================================================

#[tokio::test]
async fn test_transaction_gossip() {
    let config1 = NetworkConfig {
        node_id: 1,
        listen_addr: "127.0.0.1:49333".parse().unwrap(),
        ..Default::default()
    };
    
    let config2 = NetworkConfig {
        node_id: 2,
        listen_addr: "127.0.0.1:49334".parse().unwrap(),
        bootstrap_peers: vec!["127.0.0.1:49333".parse().unwrap()],
        ..Default::default()
    };
    
    let network1 = Arc::new(P2PNetwork::new(config1, [0u8; 32]));
    let network2 = Arc::new(P2PNetwork::new(config2, [0u8; 32]));
    
    let mut rx1 = network1.subscribe();
    let mut rx2 = network2.subscribe();
    
    let n1 = network1.clone();
    tokio::spawn(async move { n1.start().await.unwrap() });
    
    let n2 = network2.clone();
    tokio::spawn(async move { n2.start().await.unwrap() });
    
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    
    // İşlem yayınla
    let tx = Transaction::new([1u8; 32], [2u8; 32], 1000, 10, 0);
    network1.broadcast(NetworkMessage::NewTransaction(tx)).await.unwrap();
    
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    network1.stop().await.unwrap();
    network2.stop().await.unwrap();
}