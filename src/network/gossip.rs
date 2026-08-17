// ===================================================================
// PACYTE NEXUS - GOSSIP PROTOKOLÜ
// Bölüm 15 - Dosya 15.1: src/network/gossip.rs
// ===================================================================

use std::sync::Arc;
use dashmap::DashMap;
use sha3::{Digest, Sha3_256};

use crate::types::{Hash, PacyteResult};
use crate::network::message::NetworkMessage;

pub struct GossipRouter {
    seen_messages: Arc<DashMap<Hash, u64>>,
    max_seen_cache: usize,
    ttl_seconds: u64,
}

impl GossipRouter {
    pub fn new(max_seen_cache: usize, ttl_seconds: u64) -> Self {
        Self {
            seen_messages: Arc::new(DashMap::new()),
            max_seen_cache,
            ttl_seconds,
        }
    }

    pub fn should_propagate(&self, message: &NetworkMessage) -> bool {
        let hash = message.compute_short_hash();
        let now = crate::types::current_timestamp();

        if self.seen_messages.len() > self.max_seen_cache {
            self.cleanup_expired(now);
        }

        if let Some(mut entry) = self.seen_messages.get_mut(&hash) {
            *entry = now;
            false
        } else {
            self.seen_messages.insert(hash, now);
            true
        }
    }

    fn cleanup_expired(&self, now: u64) {
        let cutoff = now.saturating_sub(self.ttl_seconds);
        self.seen_messages.retain(|_, timestamp| *timestamp > cutoff);
    }

    pub fn cache_size(&self) -> usize {
        self.seen_messages.len()
    }
}

impl NetworkMessage {
    pub fn compute_short_hash(&self) -> Hash {
        let mut hasher = Sha3_256::new();
        match self {
            Self::NewTransaction(tx) => {
                hasher.update(b"tx:");
                hasher.update(&tx.hash());
            }
            Self::NewBlock(block) => {
                hasher.update(b"block:");
                hasher.update(&block.hash());
            }
            Self::Proposal(proposal) => {
                hasher.update(b"proposal:");
                hasher.update(&proposal.block.hash());
                hasher.update(&proposal.round.to_le_bytes());
            }
            Self::Vote(vote) => {
                hasher.update(b"vote:");
                hasher.update(&vote.block_hash);
                hasher.update(&vote.voter.to_le_bytes());
                hasher.update(&vote.round.to_le_bytes());
            }
            _ => {
                hasher.update(b"other:");
                hasher.update(&crate::types::current_timestamp().to_le_bytes());
            }
        }
        hasher.finalize().into()
    }
}