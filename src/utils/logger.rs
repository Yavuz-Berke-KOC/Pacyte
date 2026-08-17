// ===================================================================
// PACYTE NEXUS - LOGGER
// ===================================================================

use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter,
};
use std::path::PathBuf;

// ===================================================================
// LOGGER CONFIG
// ===================================================================

#[derive(Debug, Clone)]
pub struct LoggerConfig {
    pub level: String,
    pub format: LogFormat,
    pub output: LogOutput,
    pub file_path: Option<PathBuf>,
    pub enable_ansi: bool,
    pub show_target: bool,
    pub show_thread_ids: bool,
    pub show_file_line: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Pretty,
    Json,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogOutput {
    Stdout,
    Stderr,
    File,
    All,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Pretty,
            output: LogOutput::Stdout,
            file_path: None,
            enable_ansi: true,
            show_target: true,
            show_thread_ids: true,
            show_file_line: false,
        }
    }
}

// ===================================================================
// LOGGER INITIALIZER
// ===================================================================

pub fn init_logger(config: LoggerConfig) -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));
    
    let registry = tracing_subscriber::registry().with(env_filter);
    
    match config.format {
        LogFormat::Pretty => {
            let layer = fmt::layer()
                .with_ansi(config.enable_ansi)
                .with_target(config.show_target)
                .with_thread_ids(config.show_thread_ids)
                .with_file(config.show_file_line)
                .with_line_number(config.show_file_line)
                .with_span_events(FmtSpan::CLOSE);
            
            match config.output {
                LogOutput::Stdout => {
                    registry.with(layer.with_writer(std::io::stdout)).init();
                }
                LogOutput::Stderr => {
                    registry.with(layer.with_writer(std::io::stderr)).init();
                }
                LogOutput::File => {
                    if let Some(path) = config.file_path {
                        let file = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(path)?;
                        registry.with(layer.with_writer(file)).init();
                    }
                }

                LogOutput::All => {
    let file_layer = if let Some(path) = config.file_path {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Some(
            fmt::layer()
                .with_ansi(config.enable_ansi)
                .with_target(config.show_target)
                .with_thread_ids(config.show_thread_ids)
                .with_file(config.show_file_line)
                .with_line_number(config.show_file_line)
                .with_span_events(FmtSpan::CLOSE)
                .with_writer(file)
        )
    } else {
        None
    };        
                    let stdout_layer = layer.with_writer(std::io::stdout);
                    
                    if let Some(file_layer) = file_layer {
                        registry.with(stdout_layer).with(file_layer).init();
                    } else {
                        registry.with(stdout_layer).init();
                    }
                }
            }
        }
        LogFormat::Json => {
            let layer = fmt::layer()
                .json()
                .with_target(config.show_target)
                .with_thread_ids(config.show_thread_ids)
                .with_file(config.show_file_line)
                .with_line_number(config.show_file_line)
                .with_span_events(FmtSpan::CLOSE);
            
            registry.with(layer).init();
        }
        LogFormat::Compact => {
            let layer = fmt::layer()
                .compact()
                .with_ansi(config.enable_ansi)
                .with_target(config.show_target)
                .with_thread_ids(config.show_thread_ids)
                .with_file(config.show_file_line)
                .with_line_number(config.show_file_line);
            
            registry.with(layer).init();
        }
    }
    
    Ok(())
}

// ===================================================================
// LOGGER MACROS
// ===================================================================

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        tracing::info!($($arg)*)
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        tracing::error!($($arg)*)
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        tracing::warn!($($arg)*)
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        tracing::debug!($($arg)*)
    };
}

#[macro_export]
macro_rules! log_trace {
    ($($arg:tt)*) => {
        tracing::trace!($($arg)*)
    };
}

// ===================================================================
// PROGRESS LOGGER
// ===================================================================

pub struct ProgressLogger {
    total: u64,
    current: u64,
    last_log: std::time::Instant,
    log_interval_ms: u64,
    prefix: String,
}

impl ProgressLogger {
    pub fn new(total: u64, prefix: impl Into<String>) -> Self {
        Self {
            total,
            current: 0,
            last_log: std::time::Instant::now(),
            log_interval_ms: 1000,
            prefix: prefix.into(),
        }
    }
    
    pub fn with_interval(mut self, interval_ms: u64) -> Self {
        self.log_interval_ms = interval_ms;
        self
    }
    
    pub fn update(&mut self, delta: u64) {
        self.current += delta;
        
        let now = std::time::Instant::now();
        if now.duration_since(self.last_log).as_millis() >= self.log_interval_ms as u128 {
            self.log();
            self.last_log = now;
        }
    }
    
    pub fn set(&mut self, value: u64) {
        self.current = value;
        
        let now = std::time::Instant::now();
        if now.duration_since(self.last_log).as_millis() >= self.log_interval_ms as u128 {
            self.log();
            self.last_log = now;
        }
    }
    
    pub fn log(&self) {
        let percentage = if self.total > 0 {
            (self.current as f64 / self.total as f64) * 100.0
        } else {
            0.0
        };
        
        tracing::info!(
            "{}: {}/{} ({:.1}%)",
            self.prefix,
            self.current,
            self.total,
            percentage
        );
    }
    
    pub fn finish(&mut self) {
        self.log();
        tracing::info!("{}: Completed!", self.prefix);
    }
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logger_config_default() {
        let config = LoggerConfig::default();
        assert_eq!(config.level, "info");
        assert_eq!(config.format, LogFormat::Pretty);
        assert_eq!(config.output, LogOutput::Stdout);
    }
    
    #[test]
    fn test_progress_logger() {
        let mut progress = ProgressLogger::new(100, "Test");
        
        progress.update(25);
        progress.update(25);
        progress.update(50);
        
        progress.finish();
    }
}