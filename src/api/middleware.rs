// ===================================================================
// PACYTE NEXUS - API MIDDLEWARE
// ===================================================================

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use serde_json::json;

// ===================================================================
// RATE LIMITER
// ===================================================================

pub struct RateLimiter {
    requests: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    max_requests: u32,
    window_secs: u64,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window_secs,
        }
    }
    
    pub fn check(&self, ip: &str) -> bool {
        let now = Instant::now();
        let window = Duration::from_secs(self.window_secs);
        
        let mut requests = self.requests.write();
        let entry = requests.entry(ip.to_string()).or_insert_with(Vec::new);
        
        // Eski istekleri temizle
        entry.retain(|t| now.duration_since(*t) < window);
        
        if entry.len() >= self.max_requests as usize {
            return false;
        }
        
        entry.push(now);
        true
    }
}

pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Response {
    let ip = get_client_ip(&request);
    
    if !limiter.check(&ip) {
        return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
    }
    
    next.run(request).await
}

fn get_client_ip<B>(request: &Request<B>) -> String {
    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            request
                .headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

// ===================================================================
// AUTH MIDDLEWARE (API Key)
// ===================================================================

pub struct ApiKeyAuth {
    api_keys: Arc<RwLock<HashMap<String, ApiKeyInfo>>>,
}

#[derive(Debug, Clone)]
pub struct ApiKeyInfo {
    pub key: String,
    pub name: String,
    pub permissions: Vec<String>,
    pub rate_limit: Option<u32>,
}

impl ApiKeyAuth {
    pub fn new() -> Self {
        Self {
            api_keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn add_key(&self, key: String, info: ApiKeyInfo) {
        self.api_keys.write().insert(key, info);
    }
    
    pub fn remove_key(&self, key: &str) {
        self.api_keys.write().remove(key);
    }
    
    pub fn validate(&self, key: &str) -> Option<ApiKeyInfo> {
        self.api_keys.read().get(key).cloned()
    }
}

pub async fn api_key_middleware(
    State(auth): State<Arc<ApiKeyAuth>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    
    if let Some(key) = auth_header {
        if auth.validate(key).is_some() {
            return next.run(request).await;
        }
    }
    
    // API key query param'dan da al
    // ...
    
    (StatusCode::UNAUTHORIZED, "Invalid API key").into_response()
}

// ===================================================================
// LOGGING MIDDLEWARE
// ===================================================================

pub async fn logging_middleware(
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let start = Instant::now();
    
    let response = next.run(request).await;
    
    let duration = start.elapsed();
    let status = response.status();
    
    tracing::info!(
        "{} {} -> {} ({:.2}ms)",
        method,
        uri,
        status.as_u16(),
        duration.as_secs_f64() * 1000.0
    );
    
    response
}

// ===================================================================
// CORS MIDDLEWARE
// ===================================================================

pub fn create_cors_layer(allowed_origins: Vec<String>) -> tower_http::cors::CorsLayer {
    use tower_http::cors::{Any, CorsLayer};
    
    if allowed_origins.contains(&"*".to_string()) {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(Any)
            .allow_headers(Any)
    }
}

// ===================================================================
// COMPRESSION MIDDLEWARE
// ===================================================================

pub fn create_compression_layer() -> tower_http::compression::CompressionLayer {
    tower_http::compression::CompressionLayer::new()
        .gzip(true)
        .br(true)
        .no_deflate()
}

// ===================================================================
// REQUEST ID MIDDLEWARE
// ===================================================================

pub async fn request_id_middleware(
    mut request: Request,
    next: Next,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    
    request.headers_mut().insert(
        "X-Request-Id",
        request_id.parse().unwrap(),
    );
    
    let mut response = next.run(request).await;
    
    response.headers_mut().insert(
        "X-Request-Id",
        request_id.parse().unwrap(),
    );
    
    response
}

// ===================================================================
// METRICS MIDDLEWARE
// ===================================================================

#[derive(Debug, Default)]
pub struct Metrics {
    pub total_requests: Arc<RwLock<u64>>,
    pub requests_by_method: Arc<RwLock<HashMap<String, u64>>>,
    pub requests_by_path: Arc<RwLock<HashMap<String, u64>>>,
    pub response_codes: Arc<RwLock<HashMap<u16, u64>>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn record(&self, method: &str, path: &str, status: u16) {
        *self.total_requests.write() += 1;
        
        self.requests_by_method.write()
            .entry(method.to_string())
            .and_modify(|c| *c += 1)
            .or_insert(1);
        
        self.requests_by_path.write()
            .entry(path.to_string())
            .and_modify(|c| *c += 1)
            .or_insert(1);
        
        self.response_codes.write()
            .entry(status)
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }
    
    pub fn get_stats(&self) -> serde_json::Value {
        json!({
            "total_requests": *self.total_requests.read(),
            "requests_by_method": *self.requests_by_method.read(),
            "requests_by_path": *self.requests_by_path.read(),
            "response_codes": *self.response_codes.read(),
        })
    }
}

pub async fn metrics_middleware(
    State(metrics): State<Arc<Metrics>>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    
    let response = next.run(request).await;
    
    metrics.record(&method, &path, response.status().as_u16());
    
    response
}

// ===================================================================
// TIMEOUT MIDDLEWARE
// ===================================================================

pub fn create_timeout_layer(timeout: Duration) -> tower::timeout::TimeoutLayer {
    tower::timeout::TimeoutLayer::new(timeout)
}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(3, 1);
        let ip = "127.0.0.1";
        
        assert!(limiter.check(ip));
        assert!(limiter.check(ip));
        assert!(limiter.check(ip));
        assert!(!limiter.check(ip)); // 4. istek reddedilir
    }
    
    #[test]
    fn test_api_key_auth() {
        let auth = ApiKeyAuth::new();
        
        auth.add_key("test-key".to_string(), ApiKeyInfo {
            key: "test-key".to_string(),
            name: "Test".to_string(),
            permissions: vec!["read".to_string()],
            rate_limit: Some(100),
        });
        
        assert!(auth.validate("test-key").is_some());
        assert!(auth.validate("invalid").is_none());
    }
    
    #[test]
    fn test_metrics() {
        let metrics = Metrics::new();
        
        metrics.record("GET", "/blocks", 200);
        metrics.record("POST", "/tx", 201);
        metrics.record("GET", "/blocks", 200);
        
        assert_eq!(*metrics.total_requests.read(), 3);
        
        let stats = metrics.get_stats();
        assert_eq!(stats["total_requests"], 3);
    }
}