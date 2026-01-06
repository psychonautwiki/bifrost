use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;
use std::time::Duration;

/// Default cache TTL: 24 hours in milliseconds
const DEFAULT_CACHE_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// Default max concurrent revalidation jobs
const DEFAULT_MAX_REVALIDATION_CONCURRENCY: usize = 10;

/// Default poll interval for revalidation queue (milliseconds)
const DEFAULT_POLL_INTERVAL_MS: u64 = 1000;

/// Default reconciliation interval (6 hours in seconds)
const DEFAULT_RECONCILIATION_INTERVAL_SECS: u64 = 6 * 60 * 60;

/// Default baseline latency for adaptive shaping (milliseconds)
const DEFAULT_BASELINE_LATENCY_MS: u64 = 500;

/// Default trigram search threshold
const DEFAULT_TRIGRAM_THRESHOLD: f32 = 0.3;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct PsychonautConfig {
    pub api_url: String,
    pub cdn_url: String,
    pub thumb_size: u32,
    pub cache_ttl_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PlebisciteConfig {
    pub mongo_url: Option<String>,
    pub mongo_db: String,
    pub mongo_collection: String,
}

#[derive(Debug, Clone)]
pub struct FeaturesConfig {
    pub plebiscite_enabled: bool,
}

/// Configuration for the snapshot cache system
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Base TTL for cache entries
    pub ttl: Duration,
    /// Path to the disk cache file
    pub cache_path: PathBuf,
    /// Maximum concurrent revalidation jobs
    pub max_revalidation_concurrency: usize,
    /// How often to check for due items (milliseconds)
    pub poll_interval_ms: u64,
    /// How often to run full reconciliation (seconds)
    pub reconciliation_interval_secs: u64,
    /// Baseline latency for adaptive shaping (milliseconds)
    pub baseline_latency_ms: u64,
    /// Trigram search match threshold (0.0-1.0)
    pub trigram_threshold: f32,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_millis(DEFAULT_CACHE_TTL_MS),
            cache_path: PathBuf::from("./bifrost_cache.bin"),
            max_revalidation_concurrency: DEFAULT_MAX_REVALIDATION_CONCURRENCY,
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            reconciliation_interval_secs: DEFAULT_RECONCILIATION_INTERVAL_SECS,
            baseline_latency_ms: DEFAULT_BASELINE_LATENCY_MS,
            trigram_threshold: DEFAULT_TRIGRAM_THRESHOLD,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub psychonaut: PsychonautConfig,
    pub plebiscite: PlebisciteConfig,
    pub features: FeaturesConfig,
    pub cache: CacheConfig,
    /// Enable debug logging for backend requests (set via CLI)
    pub debug_requests: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let plebiscite_enabled = env::var("PLEBISCITE").is_ok();

        let mongo_url = if plebiscite_enabled {
            Some(
                env::var("MONGO_URL")
                    .context("MONGO_URL is required when PLEBISCITE is enabled")?,
            )
        } else {
            None
        };

        // Parse cache TTL from environment or use default (24 hours)
        let cache_ttl_ms = env::var("CACHE_TTL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_CACHE_TTL_MS);

        // Parse cache configuration
        let cache_path = env::var("CACHE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./bifrost_cache.bin"));

        let max_revalidation_concurrency = env::var("MAX_REVALIDATION_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_REVALIDATION_CONCURRENCY);

        let poll_interval_ms = env::var("POLL_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_POLL_INTERVAL_MS);

        let reconciliation_interval_secs = env::var("RECONCILIATION_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_RECONCILIATION_INTERVAL_SECS);

        let baseline_latency_ms = env::var("BASELINE_LATENCY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_BASELINE_LATENCY_MS);

        let trigram_threshold = env::var("TRIGRAM_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_TRIGRAM_THRESHOLD);

        Ok(Self {
            server: ServerConfig {
                port: env::var("PORT")
                    .unwrap_or_else(|_| "3000".to_string())
                    .parse()?,
            },
            psychonaut: PsychonautConfig {
                api_url: "https://psychonautwiki.org/w/api.php".to_string(),
                cdn_url: "https://psychonautwiki.org/".to_string(),
                thumb_size: 200,
                cache_ttl_ms,
            },
            plebiscite: PlebisciteConfig {
                mongo_url,
                mongo_db: env::var("MONGO_DB").unwrap_or_else(|_| "bifrost".to_string()),
                mongo_collection: env::var("MONGO_COLLECTION")
                    .unwrap_or_else(|_| "plebiscite".to_string()),
            },
            features: FeaturesConfig { plebiscite_enabled },
            cache: CacheConfig {
                ttl: Duration::from_millis(cache_ttl_ms),
                cache_path,
                max_revalidation_concurrency,
                poll_interval_ms,
                reconciliation_interval_secs,
                baseline_latency_ms,
                trigram_threshold,
            },
            debug_requests: false, // Set by CLI args in main.rs
        })
    }

    /// Convert cache config to revalidator config
    pub fn to_revalidator_config(&self) -> crate::cache::RevalidatorConfig {
        crate::cache::RevalidatorConfig {
            base_ttl: self.cache.ttl,
            max_concurrency: self.cache.max_revalidation_concurrency,
            cache_path: self.cache.cache_path.clone(),
            baseline_latency_ms: self.cache.baseline_latency_ms,
            poll_interval_ms: self.cache.poll_interval_ms,
            reconciliation_interval_secs: self.cache.reconciliation_interval_secs,
            cdn_url: self.psychonaut.cdn_url.clone(),
            thumb_size: self.psychonaut.thumb_size,
        }
    }
}
