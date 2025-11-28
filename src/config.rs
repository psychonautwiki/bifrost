use std::env;
use anyhow::{Context, Result};

/// Default cache TTL: 24 hours in milliseconds
const DEFAULT_CACHE_TTL_MS: u64 = 24 * 60 * 60 * 1000;

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

#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub psychonaut: PsychonautConfig,
    pub plebiscite: PlebisciteConfig,
    pub features: FeaturesConfig,
    /// Enable debug logging for backend requests (set via CLI)
    pub debug_requests: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let plebiscite_enabled = env::var("PLEBISCITE").is_ok();

        let mongo_url = if plebiscite_enabled {
            Some(env::var("MONGO_URL").context("MONGO_URL is required when PLEBISCITE is enabled")?)
        } else {
            None
        };

        // Parse cache TTL from environment or use default (24 hours)
        let cache_ttl_ms = env::var("CACHE_TTL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_CACHE_TTL_MS);

        Ok(Self {
            server: ServerConfig {
                port: env::var("PORT").unwrap_or_else(|_| "3000".to_string()).parse()?,
            },
            psychonaut: PsychonautConfig {
                api_url: "https://psychonautwiki.org/w/api.php".to_string(),
                cdn_url: "https://psychonautwiki.org/".to_string(),
                thumb_size: 100,
                cache_ttl_ms,
            },
            plebiscite: PlebisciteConfig {
                mongo_url,
                mongo_db: env::var("MONGO_DB").unwrap_or_else(|_| "bifrost".to_string()),
                mongo_collection: env::var("MONGO_COLLECTION").unwrap_or_else(|_| "plebiscite".to_string()),
            },
            features: FeaturesConfig {
                plebiscite_enabled,
            },
            debug_requests: false, // Set by CLI args in main.rs
        })
    }
}
