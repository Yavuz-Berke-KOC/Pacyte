// ===================================================================
// PACYTE NEXUS - ROUND YÖNETİMİ
// ===================================================================

use std::sync::Arc;
use parking_lot::RwLock;
use tokio::time::{timeout, Duration, Instant};

use crate::types::{PacyteError, PacyteResult, BlockHeight, Timestamp, current_timestamp};

// ===================================================================
// ROUND STATE
// ===================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundState {
    NewRound,
    ProposalSent,
    ProposalReceived,
    Prevoted,
    Precommitted,
    Committed,
    TimedOut,
}

impl std::fmt::Display for RoundState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ===================================================================
// ROUND MANAGER
// ===================================================================

pub struct RoundManager {
    height: Arc<RwLock<BlockHeight>>,
    round: Arc<RwLock<u64>>,
    state: Arc<RwLock<RoundState>>,
    start_time: Arc<RwLock<Option<Instant>>>,
    
    // Timeout değerleri (ms)
    proposal_timeout_ms: u64,
    prevote_timeout_ms: u64,
    precommit_timeout_ms: u64,
    round_timeout_ms: u64,
}

impl RoundManager {
    pub fn new(
        proposal_timeout_ms: u64,
        prevote_timeout_ms: u64,
        precommit_timeout_ms: u64,
        round_timeout_ms: u64,
    ) -> Self {
        Self {
            height: Arc::new(RwLock::new(0)),
            round: Arc::new(RwLock::new(0)),
            state: Arc::new(RwLock::new(RoundState::NewRound)),
            start_time: Arc::new(RwLock::new(None)),
            proposal_timeout_ms,
            prevote_timeout_ms,
            precommit_timeout_ms,
            round_timeout_ms,
        }
    }
    
    /// Yeni round başlat
    pub fn start_new_round(&self, height: BlockHeight, round: u64) {
        *self.height.write() = height;
        *self.round.write() = round;
        *self.state.write() = RoundState::NewRound;
        *self.start_time.write() = Some(Instant::now());
        
        tracing::info!("Round {} started for height {}", round, height);
    }
    
    /// State'i güncelle
    pub fn set_state(&self, state: RoundState) {
        *self.state.write() = state;
        tracing::debug!("Round state changed to {:?}", state);
    }
    
    /// Mevcut state'i getir
    pub fn state(&self) -> RoundState {
        *self.state.read()
    }
    
    /// Mevcut height
    pub fn height(&self) -> BlockHeight {
        *self.height.read()
    }
    
    /// Mevcut round
    pub fn round(&self) -> u64 {
        *self.round.read()
    }
    
    /// Round başlangıcından beri geçen süre (ms)
    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.read()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }
    
    /// Timeout kontrolü
    pub fn check_timeout(&self) -> Option<TimeoutType> {
        let elapsed = self.elapsed_ms();
        let state = self.state();
        
        match state {
            RoundState::NewRound => {
                if elapsed > self.proposal_timeout_ms {
                    Some(TimeoutType::Proposal)
                } else {
                    None
                }
            }
            RoundState::ProposalReceived | RoundState::ProposalSent => {
                if elapsed > self.prevote_timeout_ms {
                    Some(TimeoutType::Prevote)
                } else {
                    None
                }
            }
            RoundState::Prevoted => {
                if elapsed > self.precommit_timeout_ms {
                    Some(TimeoutType::Precommit)
                } else {
                    None
                }
            }
            RoundState::Precommitted => {
                if elapsed > self.round_timeout_ms {
                    Some(TimeoutType::Round)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    
    /// Belirli bir state'e kadar bekle (timeout ile)
    pub async fn wait_for_state(
        &self,
        expected: RoundState,
        timeout_duration: Duration,
    ) -> Result<(), TimeoutType> {
        let start = Instant::now();
        
        loop {
            if self.state() == expected {
                return Ok(());
            }
            
            if start.elapsed() > timeout_duration {
                return Err(self.check_timeout().unwrap_or(TimeoutType::Round));
            }
            
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    
    /// Round'u ilerlet
    pub fn advance_round(&self) -> (BlockHeight, u64) {
        let height = self.height();
        let current_round = self.round();
        let next_round = current_round + 1;
        
        *self.round.write() = next_round;
        *self.state.write() = RoundState::NewRound;
        *self.start_time.write() = Some(Instant::now());
        
        (height, next_round)
    }
    
    /// Round'u sıfırla (yeni height için)
    pub fn reset_for_height(&self, height: BlockHeight) {
        *self.height.write() = height;
        *self.round.write() = 0;
        *self.state.write() = RoundState::NewRound;
        *self.start_time.write() = Some(Instant::now());
    }
    
    /// Timeout süresini exponential backoff ile artır
    pub fn get_timeout_with_backoff(&self, base_timeout: u64, consecutive_timeouts: u64) -> u64 {
        let multiplier = 2u64.pow(consecutive_timeouts.min(5) as u32);
        (base_timeout * multiplier).min(base_timeout * 10)
    }
    
    /// Round'un timeout olup olmadığını kontrol et ve gerekirse ilerlet
    pub async fn run_timeout_loop<F>(&self, mut on_timeout: F)
    where
        F: FnMut(TimeoutType, BlockHeight, u64) + Send,
    {
        let mut consecutive_timeouts = 0;
        
        loop {
            if let Some(timeout_type) = self.check_timeout() {
                let height = self.height();
                let round = self.round();
                
                tracing::warn!("{:?} timeout for height={}, round={}", timeout_type, height, round);
                
                on_timeout(timeout_type, height, round);
                
                if timeout_type == TimeoutType::Round {
                    consecutive_timeouts += 1;
                    let (new_height, new_round) = self.advance_round();
                    tracing::info!("Advanced to round {} for height {}", new_round, new_height);
                }
            }
            
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

// ===================================================================
// TIMEOUT TİPLERİ
// ===================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutType {
    Proposal,
    Prevote,
    Precommit,
    Round,
}

impl std::fmt::Display for TimeoutType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ===================================================================
// ROUND TIMER
// ===================================================================

pub struct RoundTimer {
    start: Instant,
    timeout_ms: u64,
}

impl RoundTimer {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            start: Instant::now(),
            timeout_ms,
        }
    }
    
    pub fn reset(&mut self) {
        self.start = Instant::now();
    }
    
    pub fn is_expired(&self) -> bool {
        self.elapsed_ms() > self.timeout_ms
    }
    
    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
    
    pub fn remaining_ms(&self) -> u64 {
        self.timeout_ms.saturating_sub(self.elapsed_ms())
    }
    
    pub fn progress(&self) -> f64 {
        self.elapsed_ms() as f64 / self.timeout_ms as f64
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_manager() {
        let manager = RoundManager::new(1000, 2000, 3000, 5000);
        
        manager.start_new_round(1, 0);
        
        assert_eq!(manager.height(), 1);
        assert_eq!(manager.round(), 0);
        assert_eq!(manager.state(), RoundState::NewRound);
        
        manager.set_state(RoundState::ProposalSent);
        assert_eq!(manager.state(), RoundState::ProposalSent);
    }
    
    #[test]
    fn test_advance_round() {
        let manager = RoundManager::new(1000, 2000, 3000, 5000);
        
        manager.start_new_round(1, 0);
        let (height, round) = manager.advance_round();
        
        assert_eq!(height, 1);
        assert_eq!(round, 1);
        assert_eq!(manager.state(), RoundState::NewRound);
    }
    
    #[test]
    fn test_round_timer() {
        let mut timer = RoundTimer::new(100);
        
        assert!(!timer.is_expired());
        assert!(timer.remaining_ms() <= 100);
        
        std::thread::sleep(Duration::from_millis(150));
        assert!(timer.is_expired());
        
        timer.reset();
        assert!(!timer.is_expired());
    }
    
    #[test]
    fn test_timeout_with_backoff() {
        let manager = RoundManager::new(1000, 2000, 3000, 5000);
        
        assert_eq!(manager.get_timeout_with_backoff(1000, 0), 1000);
        assert_eq!(manager.get_timeout_with_backoff(1000, 1), 2000);
        assert_eq!(manager.get_timeout_with_backoff(1000, 2), 4000);
        assert_eq!(manager.get_timeout_with_backoff(1000, 3), 8000);
        assert_eq!(manager.get_timeout_with_backoff(1000, 5), 10000); // Max
        assert_eq!(manager.get_timeout_with_backoff(1000, 6), 10000); // Capped
    }
}