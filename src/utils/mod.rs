// ===================================================================
// PACYTE NEXUS - UTILS MODÜLÜ
// ===================================================================

pub mod metrics;
pub mod logger;
pub mod config;
pub mod time;

// Re-export'lar
pub use metrics::*;
pub use logger::*;
pub use config::*;
pub use time::*;

// ===================================================================
// GENEL YARDIMCILAR
// ===================================================================

/// Byte dizisini hex string'e çevir
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// Hex string'i byte dizisine çevir
pub fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    let hex = hex.trim_start_matches("0x");
    hex::decode(hex).ok()
}

/// İnsan okunabilir boyut
pub fn human_readable_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    
    format!("{:.2} {}", size, UNITS[unit_idx])
}

/// İnsan okunabilir süre
pub fn human_readable_duration(secs: u64) -> String {
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

/// Retry mekanizması
pub async fn retry<F, T, E, Fut>(
    mut f: F,
    max_retries: usize,
    delay_ms: u64,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut last_error = None;
    
    for attempt in 0..=max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = Some(e);
                if attempt < max_retries {
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }
    
    Err(last_error.unwrap())
}

/// Backoff stratejisi ile retry
pub async fn retry_with_backoff<F, T, E, Fut>(
    mut f: F,
    max_retries: usize,
    base_delay_ms: u64,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut last_error = None;
    
    for attempt in 0..=max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = Some(e);
                if attempt < max_retries {
                    let delay = base_delay_ms * 2u64.pow(attempt as u32);
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
            }
        }
    }
    
    Err(last_error.unwrap())
}

/// Şans eseri seçim (weighted random)
pub fn weighted_random<T: Clone>(items: &[(T, f64)]) -> Option<T> {
    use rand::Rng;
    
    let total_weight: f64 = items.iter().map(|(_, w)| w).sum();
    if total_weight == 0.0 {
        return None;
    }
    
    let mut rng = rand::thread_rng();
    let mut random = rng.gen::<f64>() * total_weight;
    
    for (item, weight) in items {
        random -= weight;
        if random <= 0.0 {
            return Some(item.clone());
        }
    }
    
    items.last().map(|(item, _)| item.clone())
}

/// Thread-safe ID generator
#[derive(Debug, Default)]
pub struct IdGenerator {
    next_id: std::sync::atomic::AtomicU64,
}

impl IdGenerator {
    pub fn new() -> Self {
        Self {
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }
    
    pub fn next(&self) -> u64 {
        self.next_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }
    
    pub fn current(&self) -> u64 {
        self.next_id.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Stopwatch - süre ölçümü
pub struct Stopwatch {
    start: std::time::Instant,
}

impl Stopwatch {
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
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
}

impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_hex_conversion() {
        let bytes = vec![0xde, 0xad, 0xbe, 0xef];
        let hex = bytes_to_hex(&bytes);
        assert_eq!(hex, "0xdeadbeef");
        
        let parsed = hex_to_bytes(&hex).unwrap();
        assert_eq!(parsed, bytes);
    }
    
    #[test]
    fn test_human_readable_size() {
        assert_eq!(human_readable_size(500), "500 B");
        assert_eq!(human_readable_size(1500), "1.46 KB");
        assert_eq!(human_readable_size(1500000), "1.43 MB");
    }
    
    #[test]
    fn test_human_readable_duration() {
        assert_eq!(human_readable_duration(30), "30s");
        assert_eq!(human_readable_duration(90), "1m 30s");
        assert_eq!(human_readable_duration(3665), "1h 1m");
        assert_eq!(human_readable_duration(90000), "1d 1h");
    }
    
    #[test]
    fn test_weighted_random() {
        let items = vec![
            ("a", 0.5),
            ("b", 0.3),
            ("c", 0.2),
        ];
        
        let selected = weighted_random(&items);
        assert!(selected.is_some());
    }
    
    #[test]
    fn test_id_generator() {
        let gen = IdGenerator::new();
        assert_eq!(gen.next(), 1);
        assert_eq!(gen.next(), 2);
        assert_eq!(gen.next(), 3);
    }
    
    #[test]
    fn test_stopwatch() {
        let mut sw = Stopwatch::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(sw.elapsed_ms() >= 10);
        
        sw.reset();
        assert!(sw.elapsed_ms() < 5);
    }
}