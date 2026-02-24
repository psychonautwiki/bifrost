//! Cache module for bifrost.
//!
//! This module provides the complete caching infrastructure including:
//! - In-memory snapshot with indexes for fast queries
//! - Background revalidation with adaptive shaping
//! - Disk persistence for crash recovery
//! - Prometheus metrics integration
//!
//! The legacy StaleWhileRevalidateCache is preserved for migration purposes.

pub mod persistence;
pub mod revalidation;
pub mod revalidator;
pub mod selftest;
pub mod shaping;
pub mod snapshot;

pub use persistence::{load_from_disk, persist_to_disk};
pub use revalidator::{Revalidator, RevalidatorConfig};
pub use snapshot::{SnapshotHolder, SubstanceSnapshot};

// Legacy cache implementation (preserved for migration)
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};
use tracing::{debug, error};

#[derive(Clone)]
struct CacheEntry<V> {
    data: V,
    fetched_at: Instant,
}

#[derive(Clone)]
pub struct StaleWhileRevalidateCache<K, V> {
    store: Arc<RwLock<HashMap<K, CacheEntry<V>>>>,
    inflight: Arc<RwLock<HashMap<K, Arc<Notify>>>>,
    ttl: Duration,
}

impl<K, V> StaleWhileRevalidateCache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static + std::fmt::Debug,
    V: Clone + Send + Sync + 'static,
{
    pub fn new(ttl_ms: u64) -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
            inflight: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_millis(ttl_ms),
        }
    }

    pub async fn get<F, Fut>(&self, key: K, fetcher: F) -> Result<V, crate::error::BifrostError>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<V, crate::error::BifrostError>> + Send,
    {
        // 1. Check Cache
        {
            let read_guard = self.store.read().await;
            if let Some(entry) = read_guard.get(&key) {
                let age = entry.fetched_at.elapsed();
                if age < self.ttl {
                    debug!(
                        cache_key = ?key,
                        age_ms = age.as_millis() as u64,
                        ttl_ms = self.ttl.as_millis() as u64,
                        "Cache HIT (fresh)"
                    );
                    return Ok(entry.data.clone());
                } else {
                    // 2. Stale Hit - return stale data but refresh in background
                    debug!(
                        cache_key = ?key,
                        age_ms = age.as_millis() as u64,
                        ttl_ms = self.ttl.as_millis() as u64,
                        "Cache HIT (stale), triggering background refresh"
                    );
                    let self_clone = self.clone();
                    let key_clone = key.clone();

                    tokio::spawn(async move {
                        self_clone.try_refresh(key_clone, fetcher).await;
                    });

                    return Ok(entry.data.clone());
                }
            }
        }

        // 3. Cache Miss
        debug!(cache_key = ?key, "Cache MISS, fetching from upstream");
        self.fetch_coalesced(key, fetcher).await
    }

    async fn try_refresh<F, Fut>(&self, key: K, fetcher: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<V, crate::error::BifrostError>> + Send,
    {
        {
            let mut inflight_guard = self.inflight.write().await;
            if inflight_guard.contains_key(&key) {
                return;
            }
            inflight_guard.insert(key.clone(), Arc::new(Notify::new()));
        }

        debug!("Background refreshing {:?}", key);
        match fetcher().await {
            Ok(data) => {
                let mut store_guard = self.store.write().await;
                store_guard.insert(
                    key.clone(),
                    CacheEntry {
                        data,
                        fetched_at: Instant::now(),
                    },
                );
                debug!("Refreshed {:?}", key);
            }
            Err(e) => {
                error!("Failed to refresh cache for {:?}: {}", key, e);
            }
        }

        let mut inflight_guard = self.inflight.write().await;
        inflight_guard.remove(&key);
    }

    async fn fetch_coalesced<F, Fut>(
        &self,
        key: K,
        fetcher: F,
    ) -> Result<V, crate::error::BifrostError>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<V, crate::error::BifrostError>> + Send,
    {
        let notify = {
            let mut inflight_guard = self.inflight.write().await;
            if let Some(notify) = inflight_guard.get(&key) {
                notify.clone()
            } else {
                let notify = Arc::new(Notify::new());
                inflight_guard.insert(key.clone(), notify.clone());

                drop(inflight_guard);

                let result = fetcher().await;

                match result {
                    Ok(data) => {
                        let mut store_guard = self.store.write().await;
                        store_guard.insert(
                            key.clone(),
                            CacheEntry {
                                data: data.clone(),
                                fetched_at: Instant::now(),
                            },
                        );

                        let inflight_guard = self.inflight.read().await;
                        if let Some(n) = inflight_guard.get(&key) {
                            n.notify_waiters();
                        }

                        drop(inflight_guard);
                        let mut inflight_guard = self.inflight.write().await;
                        inflight_guard.remove(&key);

                        return Ok(data);
                    }
                    Err(e) => {
                        let mut inflight_guard = self.inflight.write().await;
                        inflight_guard.remove(&key);
                        notify.notify_waiters();
                        return Err(e);
                    }
                }
            }
        };

        notify.notified().await;

        let read_guard = self.store.read().await;
        if let Some(entry) = read_guard.get(&key) {
            Ok(entry.data.clone())
        } else {
            Err(crate::error::BifrostError::Upstream(
                "Request coalescing failed (leader failed)".into(),
            ))
        }
    }
}
