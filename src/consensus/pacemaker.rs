// ===================================================================
// PACYTE NEXUS - PACEMAKER (Timeout Yönetimi)
// ===================================================================

use std::sync::Arc;
use std::time::Duration;
use parking_lot::RwLock;
use tokio::sync::{mpsc, broadcast};
use tokio::time::{interval, MissedTickBehavior};

use crate::types::{PacyteError, PacyteResult, BlockHeight, Timestamp, current_timestamp};
use super::{ConsensusCommand, RoundManager, TimeoutType};

// ===================================================================
// PACEMAKER
// ===================================================================

pub struct Pacemaker {
    round_manager: Arc<RoundManager>,
    cmd_tx: mpsc::UnboundedSender<ConsensusCommand>,
    
    // Timeout yönetimi
    base_timeout_ms: u64,
    max_timeout_ms: u64,
    consecutive_timeouts: Arc<RwLock<u64>>,
    
    // Exponential backoff çarpanı
    backoff_multiplier: f64,
    
    // Son aktivite zamanı
    last_activity: Arc<RwLock<Timestamp>>,
    
    // Shutdown
    shutdown_tx: broadcast::Sender<()>,
}

impl Pacemaker {
    pub fn new(
        round_manager: Arc<RoundManager>,
        cmd_tx: mpsc::UnboundedSender<ConsensusCommand>,
        base_timeout_ms: u64,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        
        Self {
            round_manager,
            cmd_tx,
            base_timeout_ms,
            max_timeout_ms: base_timeout_ms * 10,
            consecutive_timeouts: Arc::new(RwLock::new(0)),
            backoff_multiplier: 1.5,
            last_activity: Arc::new(RwLock::new(current_timestamp())),
            shutdown_tx,
        }
    }
    
    /// Pacemaker'ı başlat
    pub async fn start(&self) -> PacyteResult<()> {
        let mut ticker = interval(Duration::from_millis(100));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        
        tracing::info!("Pacemaker started with base timeout {}ms", self.base_timeout_ms);
        
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.check_and_handle_timeouts().await;
                }
                
                _ = shutdown_rx.recv() => {
                    tracing::info!("Pacemaker stopped");
                    break;
                }
            }
        }
        
        Ok(())
    }
    
    /// Timeout'ları kontrol et ve yönet
    async fn check_and_handle_timeouts(&self) {
        let state = self.round_manager.state();
        let height = self.round_manager.height();
        let round = self.round_manager.round();
        let elapsed = self.round_manager.elapsed_ms();
        
        // Mevcut timeout süresini hesapla
        let current_timeout = self.get_current_timeout();
        
        if elapsed > current_timeout {
            tracing::warn!(
                "Timeout detected: height={}, round={}, state={:?}, elapsed={}ms, timeout={}ms",
                height, round, state, elapsed, current_timeout
            );
            
            // Consecutive timeout sayacını artır
            {
                let mut consecutive = self.consecutive_timeouts.write();
                *consecutive += 1;
            }
            
            // Timeout komutu gönder
            let cmd = ConsensusCommand::Timeout { height, round };
            let _ = self.cmd_tx.send(cmd);
            
            // Aktivite zamanını güncelle
            *self.last_activity.write() = current_timestamp();
        }
    }
    
    /// Mevcut timeout süresini hesapla (exponential backoff ile)
    fn get_current_timeout(&self) -> u64 {
        let consecutive = *self.consecutive_timeouts.read();
        
        if consecutive == 0 {
            return self.base_timeout_ms;
        }
        
        let multiplier = self.backoff_multiplier.powi(consecutive as i32);
        let timeout = (self.base_timeout_ms as f64 * multiplier) as u64;
        
        timeout.min(self.max_timeout_ms)
    }
    
    /// Aktivite kaydet (timeout sayacını sıfırlar)
    pub fn record_activity(&self) {
        *self.consecutive_timeouts.write() = 0;
        *self.last_activity.write() = current_timestamp();
    }
    
    /// Round değiştiğinde çağrılır
    pub fn on_new_round(&self) {
        self.record_activity();
    }
    
    /// Proposal alındığında çağrılır
    pub fn on_proposal_received(&self) {
        self.record_activity();
    }
    
    /// Vote alındığında çağrılır
    pub fn on_vote_received(&self) {
        self.record_activity();
    }
    
    /// Quorum sağlandığında çağrılır
    pub fn on_quorum_reached(&self) {
        self.record_activity();
        *self.consecutive_timeouts.write() = 0;
    }
    
    /// Timeout sayacını sıfırla
    pub fn reset_timeouts(&self) {
        *self.consecutive_timeouts.write() = 0;
    }
    
    /// Pacemaker'ı durdur
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(());
    }
    
    /// Senkronizasyon moduna geç
    pub fn enter_sync_mode(&self) {
        tracing::info!("Entering sync mode");
        // Sync modunda timeout'ları durdur
        self.reset_timeouts();
    }
    
    /// Senkronizasyondan çık
    pub fn exit_sync_mode(&self) {
        tracing::info!("Exiting sync mode");
        self.record_activity();
    }
    
    /// Mevcut timeout bilgilerini getir
    pub fn status(&self) -> PacemakerStatus {
        PacemakerStatus {
            base_timeout_ms: self.base_timeout_ms,
            current_timeout_ms: self.get_current_timeout(),
            consecutive_timeouts: *self.consecutive_timeouts.read(),
            last_activity: *self.last_activity.read(),
        }
    }
}

// ===================================================================
// PACEMAKER STATUS
// ===================================================================

#[derive(Debug, Clone)]
pub struct PacemakerStatus {
    pub base_timeout_ms: u64,
    pub current_timeout_ms: u64,
    pub consecutive_timeouts: u64,
    pub last_activity: Timestamp,
}

impl std::fmt::Display for PacemakerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Pacemaker: timeout={}ms (base={}ms), consecutive={}, idle={}s",
            self.current_timeout_ms,
            self.base_timeout_ms,
            self.consecutive_timeouts,
            current_timestamp().saturating_sub(self.last_activity)
        )
    }
}

// ===================================================================
// TIMEOUT CALCULATOR
// ===================================================================

pub struct TimeoutCalculator {
    base_timeout: Duration,
    min_timeout: Duration,
    max_timeout: Duration,
    backoff_factor: f64,
}

impl TimeoutCalculator {
    pub fn new(base_timeout: Duration) -> Self {
        Self {
            base_timeout,
            min_timeout: base_timeout,
            max_timeout: base_timeout * 10,
            backoff_factor: 1.5,
        }
    }
    
    pub fn with_backoff(base_timeout: Duration, backoff_factor: f64) -> Self {
        Self {
            base_timeout,
            min_timeout: base_timeout,
            max_timeout: base_timeout * 10,
            backoff_factor,
        }
    }
    
    pub fn calculate(&self, consecutive_failures: u32) -> Duration {
        if consecutive_failures == 0 {
            return self.base_timeout;
        }
        
        let multiplier = self.backoff_factor.powi(consecutive_failures as i32);
        let timeout = Duration::from_millis(
            (self.base_timeout.as_millis() as f64 * multiplier) as u64
        );
        
        timeout.clamp(self.min_timeout, self.max_timeout)
    }
    
    pub fn calculate_for_round(&self, round: u64, is_leader: bool) -> Duration {
        // Leader için daha kısa timeout
        let base = if is_leader {
            self.base_timeout / 2
        } else {
            self.base_timeout
        };
        
        // Round arttıkça timeout artar
        let round_factor = 1.0 + (round as f64 * 0.1);
        let timeout = Duration::from_millis(
            (base.as_millis() as f64 * round_factor) as u64
        );
        
        timeout.clamp(self.min_timeout, self.max_timeout)
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timeout_calculator() {
        let calc = TimeoutCalculator::new(Duration::from_millis(1000));
        
        assert_eq!(calc.calculate(0), Duration::from_millis(1000));
        assert_eq!(calc.calculate(1), Duration::from_millis(1500));
        assert_eq!(calc.calculate(2), Duration::from_millis(2250));
        
        // Max timeout'u aşmamalı
        let timeout = calc.calculate(10);
        assert!(timeout <= Duration::from_millis(10000));
    }
    
    #[test]
    fn test_round_timeout() {
        let calc = TimeoutCalculator::new(Duration::from_millis(1000));
        
        let leader_timeout = calc.calculate_for_round(0, true);
        let validator_timeout = calc.calculate_for_round(0, false);
        
        assert!(leader_timeout < validator_timeout);
    }
    
    #[test]
    fn test_pacemaker_status() {
        let status = PacemakerStatus {
            base_timeout_ms: 1000,
            current_timeout_ms: 1500,
            consecutive_timeouts: 2,
            last_activity: current_timestamp(),
        };
        
        let display = format!("{}", status);
        assert!(display.contains("1000"));
        assert!(display.contains("1500"));
        assert!(display.contains("2"));
    }
}