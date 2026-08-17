// ===================================================================
// PACYTE NEXUS - TIME UTILITIES
// ===================================================================

use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ===================================================================
// TIMESTAMP
// ===================================================================

pub type Timestamp = u64;

/// Mevcut Unix timestamp (saniye)
pub fn current_timestamp() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Mevcut Unix timestamp (milisaniye)
pub fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Mevcut Unix timestamp (mikrosaniye)
pub fn current_timestamp_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Timestamp'i SystemTime'a çevir
pub fn timestamp_to_system_time(ts: Timestamp) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(ts)
}

/// SystemTime'ı timestamp'e çevir
pub fn system_time_to_timestamp(time: SystemTime) -> Timestamp {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ===================================================================
// TIME UTILITIES
// ===================================================================

/// Süreyi insan okunabilir formata çevir
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}

/// Timestamp'i insan okunabilir formata çevir
pub fn format_timestamp(ts: Timestamp) -> String {
    use chrono::{DateTime, Utc};
    
    let datetime = DateTime::<Utc>::from_timestamp(ts as i64, 0)
        .unwrap_or_default();
    
    datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Timestamp'i ISO 8601 formatına çevir
pub fn timestamp_to_iso8601(ts: Timestamp) -> String {
    use chrono::{DateTime, Utc};
    
    let datetime = DateTime::<Utc>::from_timestamp(ts as i64, 0)
        .unwrap_or_default();
    
    datetime.to_rfc3339()
}

/// ISO 8601 formatından timestamp'e çevir
pub fn iso8601_to_timestamp(s: &str) -> Option<Timestamp> {
    use chrono::{DateTime, Utc};
    
    let datetime: DateTime<Utc> = s.parse().ok()?;
    Some(datetime.timestamp() as Timestamp)
}

// ===================================================================
// BLOCK TIME
// ===================================================================

/// Hedef blok süresi (ms)
pub const TARGET_BLOCK_TIME_MS: u64 = 1000;

/// Beklenen blok zamanını hesapla
pub fn expected_block_time(previous_time: Timestamp, target_ms: u64) -> Timestamp {
    previous_time + (target_ms / 1000)
}

/// Zorluk ayarlaması hesapla
pub fn calculate_difficulty_adjustment(
    actual_time: Timestamp,
    expected_time: Timestamp,
    current_difficulty: u64,
) -> u64 {
    let ratio = actual_time as f64 / expected_time as f64;
    
    if ratio > 1.5 {
        (current_difficulty as f64 * 0.9) as u64 // Kolaylaştır
    } else if ratio < 0.5 {
        (current_difficulty as f64 * 1.1) as u64 // Zorlaştır
    } else {
        current_difficulty
    }
}

// ===================================================================
// TIMER
// ===================================================================

pub struct Timer {
    start: std::time::Instant,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
    
    pub fn start() -> Self {
        Self::new()
    }
    
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    
    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
    
    pub fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
    
    pub fn reset(&mut self) {
        self.start = std::time::Instant::now();
    }
    
    pub fn has_elapsed(&self, duration: Duration) -> bool {
        self.start.elapsed() >= duration
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

// ===================================================================
// INTERVAL
// ===================================================================

pub struct Interval {
    period: Duration,
    last_tick: std::time::Instant,
}

impl Interval {
    pub fn new(period: Duration) -> Self {
        Self {
            period,
            last_tick: std::time::Instant::now(),
        }
    }
    
    pub fn from_millis(millis: u64) -> Self {
        Self::new(Duration::from_millis(millis))
    }
    
    pub fn from_secs(secs: u64) -> Self {
        Self::new(Duration::from_secs(secs))
    }
    
    pub fn tick(&mut self) -> bool {
        let now = std::time::Instant::now();
        if now.duration_since(self.last_tick) >= self.period {
            self.last_tick = now;
            true
        } else {
            false
        }
    }
    
    pub fn reset(&mut self) {
        self.last_tick = std::time::Instant::now();
    }
    
    pub fn remaining(&self) -> Duration {
        let elapsed = self.last_tick.elapsed();
        if elapsed >= self.period {
            Duration::ZERO
        } else {
            self.period - elapsed
        }
    }
}

// ===================================================================
// DEADLINE
// ===================================================================

pub struct Deadline {
    deadline: std::time::Instant,
}

impl Deadline {
    pub fn new(timeout: Duration) -> Self {
        Self {
            deadline: std::time::Instant::now() + timeout,
        }
    }
    
    pub fn from_millis(millis: u64) -> Self {
        Self::new(Duration::from_millis(millis))
    }
    
    pub fn from_secs(secs: u64) -> Self {
        Self::new(Duration::from_secs(secs))
    }
    
    pub fn is_expired(&self) -> bool {
        std::time::Instant::now() >= self.deadline
    }
    
    pub fn remaining(&self) -> Duration {
        let now = std::time::Instant::now();
        if now >= self.deadline {
            Duration::ZERO
        } else {
            self.deadline - now
        }
    }
    
    pub fn remaining_millis(&self) -> u64 {
        self.remaining().as_millis() as u64
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_timestamp() {
        let ts = current_timestamp();
        assert!(ts > 1700000000); // 2023 sonrası
    }
    
    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3665)), "1h 1m");
        assert_eq!(format_duration(Duration::from_secs(90000)), "1d 1h");
    }
    
    #[test]
    fn test_format_timestamp() {
        let ts = 1700000000;
        let formatted = format_timestamp(ts);
        assert!(formatted.contains("UTC"));
    }
    
    #[test]
    fn test_timer() {
        let mut timer = Timer::new();
        std::thread::sleep(Duration::from_millis(10));
        assert!(timer.elapsed_ms() >= 10);
        
        timer.reset();
        assert!(timer.elapsed_ms() < 5);
    }
    
    #[test]
    fn test_interval() {
        let mut interval = Interval::from_millis(10);
        
        assert!(!interval.tick());
        std::thread::sleep(Duration::from_millis(15));
        assert!(interval.tick());
        assert!(!interval.tick());
    }
    
    #[test]
    fn test_deadline() {
        let deadline = Deadline::from_millis(50);
        
        assert!(!deadline.is_expired());
        std::thread::sleep(Duration::from_millis(100));
        assert!(deadline.is_expired());
        assert_eq!(deadline.remaining(), Duration::ZERO);
    }
    
    #[test]
    fn test_difficulty_adjustment() {
        let new_diff = calculate_difficulty_adjustment(1500, 1000, 100);
        assert_eq!(new_diff, 90); // %10 kolaylaştı
        
        let new_diff = calculate_difficulty_adjustment(500, 1000, 100);
        assert_eq!(new_diff, 110); // %10 zorlaştı
        
        let new_diff = calculate_difficulty_adjustment(1000, 1000, 100);
        assert_eq!(new_diff, 100); // Aynı
    }
}