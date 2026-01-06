//! Disk persistence for the substance snapshot cache.
//!
//! This module handles saving and loading the cache to/from disk
//! with integrity checking via checksums.

use crate::cache::snapshot::SubstanceSnapshot;
use crate::error::BifrostError;
use crate::graphql::model::Substance;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs;
use tracing::{debug, info, warn};

/// Version of the disk cache format
const CACHE_VERSION: u32 = 1;

/// Disk cache format with metadata and integrity check
#[derive(Serialize, Deserialize)]
pub struct DiskCache {
    /// Schema version for forward compatibility
    pub version: u32,
    /// When this cache was created
    pub created_at: DateTime<Utc>,
    /// SHA256 checksum of serialized substances
    pub checksum: String,
    /// Number of substances (for quick validation without deserializing all)
    pub substance_count: usize,
    /// The actual substance data
    pub substances: Vec<Substance>,
}

impl DiskCache {
    /// Create a new disk cache from substances
    pub fn new(substances: Vec<Substance>) -> Result<Self, BifrostError> {
        let substance_count = substances.len();

        // Compute checksum of substances
        let substances_bytes = rmp_serde::to_vec(&substances)
            .map_err(|e| BifrostError::Cache(format!("Failed to serialize substances: {}", e)))?;

        let checksum = compute_sha256_hex(&substances_bytes);

        Ok(Self {
            version: CACHE_VERSION,
            created_at: Utc::now(),
            checksum,
            substance_count,
            substances,
        })
    }

    /// Validate the cache integrity
    pub fn validate(&self) -> Result<(), BifrostError> {
        // Check version
        if self.version > CACHE_VERSION {
            return Err(BifrostError::Cache(format!(
                "Cache version {} is newer than supported version {}",
                self.version, CACHE_VERSION
            )));
        }

        // Check count
        if self.substances.len() != self.substance_count {
            return Err(BifrostError::Cache(format!(
                "Substance count mismatch: expected {}, got {}",
                self.substance_count,
                self.substances.len()
            )));
        }

        // Verify checksum
        let substances_bytes = rmp_serde::to_vec(&self.substances)
            .map_err(|e| BifrostError::Cache(format!("Failed to serialize substances: {}", e)))?;

        let computed = compute_sha256_hex(&substances_bytes);

        if computed != self.checksum {
            return Err(BifrostError::Cache(
                "Checksum mismatch - cache may be corrupt".to_string(),
            ));
        }

        Ok(())
    }
}

/// Compute SHA256 hash and return as hex string
fn compute_sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Load a snapshot from disk
pub async fn load_from_disk(path: &Path) -> Result<SubstanceSnapshot, BifrostError> {
    info!(path = %path.display(), "Loading cache from disk");

    // Read file
    let bytes = fs::read(path)
        .await
        .map_err(|e| BifrostError::Cache(format!("Failed to read cache file: {}", e)))?;

    debug!(bytes = bytes.len(), "Read cache file");

    // Deserialize
    let cache: DiskCache = rmp_serde::from_slice(&bytes)
        .map_err(|e| BifrostError::Cache(format!("Failed to deserialize cache: {}", e)))?;

    info!(
        version = cache.version,
        substances = cache.substance_count,
        created_at = %cache.created_at,
        "Loaded disk cache metadata"
    );

    // Validate integrity
    cache.validate()?;

    info!("Cache integrity validated");

    // Build snapshot with indexes
    let snapshot = SubstanceSnapshot::build(cache.substances);

    Ok(snapshot)
}

/// Save a snapshot to disk
pub async fn persist_to_disk(
    snapshot: &SubstanceSnapshot,
    path: &Path,
) -> Result<(), BifrostError> {
    info!(
        path = %path.display(),
        substances = snapshot.substances.len(),
        "Persisting cache to disk"
    );

    // Create cache structure
    let cache = DiskCache::new(snapshot.substances.clone())?;

    // Serialize to MessagePack
    let bytes = rmp_serde::to_vec(&cache)
        .map_err(|e| BifrostError::Cache(format!("Failed to serialize cache: {}", e)))?;

    debug!(bytes = bytes.len(), "Serialized cache");

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| BifrostError::Cache(format!("Failed to create cache directory: {}", e)))?;
    }

    // Atomic write: write to temp file, then rename
    let temp_path = path.with_extension("tmp");

    fs::write(&temp_path, &bytes)
        .await
        .map_err(|e| BifrostError::Cache(format!("Failed to write temp cache file: {}", e)))?;

    fs::rename(&temp_path, path)
        .await
        .map_err(|e| BifrostError::Cache(format!("Failed to rename cache file: {}", e)))?;

    info!(
        path = %path.display(),
        bytes = bytes.len(),
        checksum = %cache.checksum,
        "Cache persisted successfully"
    );

    Ok(())
}

/// Check if a cache file exists and is valid
pub async fn cache_exists_and_valid(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }

    match load_cache_metadata(path).await {
        Ok(_) => true,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Cache file invalid");
            false
        }
    }
}

/// Load just the metadata from a cache file (for quick validation)
async fn load_cache_metadata(path: &Path) -> Result<(u32, usize, DateTime<Utc>), BifrostError> {
    let bytes = fs::read(path)
        .await
        .map_err(|e| BifrostError::Cache(format!("Failed to read cache file: {}", e)))?;

    // We need to deserialize the full structure unfortunately
    // MessagePack doesn't support partial reads easily
    let cache: DiskCache = rmp_serde::from_slice(&bytes)
        .map_err(|e| BifrostError::Cache(format!("Failed to deserialize cache: {}", e)))?;

    // Quick validation
    cache.validate()?;

    Ok((cache.version, cache.substance_count, cache.created_at))
}

/// Get the size of the cache file in bytes
pub async fn get_cache_size(path: &Path) -> Option<u64> {
    fs::metadata(path).await.ok().map(|m| m.len())
}

/// Delete the cache file
pub async fn delete_cache(path: &Path) -> Result<(), BifrostError> {
    fs::remove_file(path)
        .await
        .map_err(|e| BifrostError::Cache(format!("Failed to delete cache file: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::model::Substance;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_persist_and_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_cache.bin");

        // Create test substances
        let substances = vec![
            Substance {
                name: Some("LSD".to_string()),
                url: Some("https://example.com/lsd".to_string()),
                ..Default::default()
            },
            Substance {
                name: Some("MDMA".to_string()),
                url: Some("https://example.com/mdma".to_string()),
                ..Default::default()
            },
        ];

        // Build and persist snapshot
        let snapshot = SubstanceSnapshot::build(substances);
        persist_to_disk(&snapshot, &path).await.unwrap();

        // Load and verify
        let loaded = load_from_disk(&path).await.unwrap();
        assert_eq!(loaded.substances.len(), 2);
        assert!(loaded.get_by_name("lsd").is_some());
        assert!(loaded.get_by_name("mdma").is_some());
    }

    #[tokio::test]
    async fn test_checksum_validation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_cache.bin");

        // Create and save cache
        let substances = vec![Substance {
            name: Some("Test".to_string()),
            ..Default::default()
        }];
        let snapshot = SubstanceSnapshot::build(substances);
        persist_to_disk(&snapshot, &path).await.unwrap();

        // Corrupt the file
        let mut bytes: Vec<u8> = tokio::fs::read(&path).await.unwrap();
        if let Some(last) = bytes.last_mut() {
            *last = last.wrapping_add(1);
        }
        tokio::fs::write(&path, &bytes).await.unwrap();

        // Loading should fail
        let result = load_from_disk(&path).await;
        assert!(result.is_err());
    }
}
