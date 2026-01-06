//! Background revalidator for proactive cache refresh.
//!
//! This module contains the main revalidation loop that runs in the background,
//! fetching updated substance data and maintaining cache freshness.

use crate::cache::persistence::persist_to_disk;
use crate::cache::revalidation::{RevalidationAction, RevalidationOutcome, RevalidationQueue};
use crate::cache::shaping::AdaptiveShaping;
use crate::cache::snapshot::SnapshotHolder;
use crate::error::BifrostError;
use crate::graphql::model::{Effect, Substance, SubstanceImage};
use crate::services::psychonaut::api::PsychonautApi;
use crate::services::psychonaut::parser::WikitextParser;
use md5::Digest;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Configuration for the revalidator
#[derive(Debug, Clone)]
pub struct RevalidatorConfig {
    /// Base TTL for cache entries
    pub base_ttl: Duration,
    /// Maximum concurrent revalidation jobs
    pub max_concurrency: usize,
    /// Path to the disk cache file
    pub cache_path: PathBuf,
    /// Baseline latency for adaptive shaping (ms)
    pub baseline_latency_ms: u64,
    /// How often to check for due items (ms)
    pub poll_interval_ms: u64,
    /// How often to run full reconciliation (secs)
    pub reconciliation_interval_secs: u64,
    /// CDN URL for image URLs
    pub cdn_url: String,
    /// Thumbnail size for image URLs
    pub thumb_size: u32,
}

impl Default for RevalidatorConfig {
    fn default() -> Self {
        Self {
            base_ttl: Duration::from_secs(86400), // 24 hours
            max_concurrency: 10,
            cache_path: PathBuf::from("./bifrost_cache.bin"),
            baseline_latency_ms: 500,
            poll_interval_ms: 1000,
            reconciliation_interval_secs: 21600, // 6 hours
            cdn_url: "https://psychonautwiki.org/".to_string(),
            thumb_size: 200,
        }
    }
}

/// Background revalidator for maintaining cache freshness
pub struct Revalidator {
    /// Snapshot holder for atomic updates
    snapshot: SnapshotHolder,
    /// Revalidation queue
    queue: Arc<RevalidationQueue>,
    /// Adaptive shaping controller
    shaping: Arc<Mutex<AdaptiveShaping>>,
    /// API client
    api: PsychonautApi,
    /// Parser for wiki data
    parser: WikitextParser,
    /// Configuration
    config: RevalidatorConfig,
    /// Shutdown signal receiver
    shutdown_rx: watch::Receiver<bool>,
    /// Last reconciliation time
    last_reconciliation: Mutex<Instant>,
}

impl Revalidator {
    /// Create a new revalidator
    pub fn new(
        snapshot: SnapshotHolder,
        api: PsychonautApi,
        config: RevalidatorConfig,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        let queue = Arc::new(RevalidationQueue::new(
            config.base_ttl,
            config.max_concurrency,
        ));
        let shaping = Arc::new(Mutex::new(AdaptiveShaping::new(
            config.max_concurrency,
            config.baseline_latency_ms,
        )));

        Self {
            snapshot,
            queue,
            shaping,
            api,
            parser: WikitextParser::new(),
            config,
            shutdown_rx,
            last_reconciliation: Mutex::new(Instant::now()),
        }
    }

    /// Initialize the queue with all substances from the snapshot
    pub async fn initialize_queue(&self) {
        let snapshot = self.snapshot.get().await;
        let names: Vec<String> = snapshot
            .substances
            .iter()
            .filter_map(|s| s.name.clone())
            .collect();

        info!(substances = names.len(), "Initializing revalidation queue");
        self.queue.add_many(names).await;
    }

    /// Run the main revalidation loop
    pub async fn run(&self) {
        info!("Starting background revalidator");

        loop {
            // Check for shutdown
            if *self.shutdown_rx.borrow() {
                info!("Revalidator received shutdown signal");
                break;
            }

            // Check if shaping says to pause
            {
                let shaping = self.shaping.lock().await;
                if shaping.should_pause() {
                    debug!("Circuit breaker open, waiting...");
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }
            }

            // Check for periodic reconciliation
            self.check_reconciliation().await;

            // Get available concurrency
            let current_concurrency = {
                let shaping = self.shaping.lock().await;
                shaping.current_concurrency()
            };

            // Select substances to revalidate
            let batch = self.queue.select_next_batch(current_concurrency).await;

            if batch.is_empty() {
                // Nothing due, sleep briefly
                sleep(Duration::from_millis(self.config.poll_interval_ms)).await;
                continue;
            }

            debug!(batch_size = batch.len(), "Processing revalidation batch");

            // Process batch concurrently
            let mut handles = Vec::new();
            for name in batch {
                let permit = match self.queue.acquire_permit().await {
                    Some(p) => p,
                    None => continue,
                };

                let revalidator = self.clone_for_job();
                let name_clone = name.clone();

                handles.push(tokio::spawn(async move {
                    let result = revalidator.revalidate_substance(&name_clone).await;
                    drop(permit);
                    revalidator.queue.release_in_flight().await;
                    (name_clone, result)
                }));
            }

            // Wait for all jobs to complete
            for handle in handles {
                match handle.await {
                    Ok((name, result)) => {
                        self.handle_result(&name, result).await;
                    }
                    Err(e) => {
                        error!(error = %e, "Revalidation job panicked");
                    }
                }
            }

            // Apply rate limit if configured
            let rate_limit = {
                let shaping = self.shaping.lock().await;
                shaping.get_rate_limit()
            };
            if !rate_limit.is_zero() {
                sleep(rate_limit).await;
            }
        }

        info!("Revalidator stopped");
    }

    /// Clone references needed for a revalidation job
    fn clone_for_job(&self) -> RevalidatorJob {
        RevalidatorJob {
            api: self.api.clone(),
            parser: self.parser.clone(),
            queue: self.queue.clone(),
            shaping: self.shaping.clone(),
            config: self.config.clone(),
        }
    }

    /// Handle the result of a revalidation job
    async fn handle_result(&self, name: &str, result: RevalidationJobResult) {
        // Record metrics
        {
            let mut shaping = self.shaping.lock().await;
            shaping.record_and_evaluate(
                name.to_string(),
                result.outcome_success(),
                result.latency_ms,
            );
        }

        // Extract fields before consuming result
        let substance = result.substance;
        let outcome = result.outcome.into_revalidation_outcome();

        // Handle queue outcome
        let action = self.queue.handle_outcome(name, outcome).await;

        match action {
            RevalidationAction::UpdateSnapshot => {
                if let Some(s) = substance {
                    self.update_snapshot(name, s).await;
                }
            }
            RevalidationAction::RemoveFromSnapshot => {
                self.remove_from_snapshot(name).await;
            }
            RevalidationAction::None => {}
        }
    }

    /// Update a substance in the snapshot
    async fn update_snapshot(&self, name: &str, substance: Substance) {
        self.snapshot
            .modify(|snapshot| {
                snapshot.update_substance(name, substance);
            })
            .await;

        debug!(substance = name, "Updated substance in snapshot");

        // Periodically persist to disk (every 100 updates or so)
        // This is a simple heuristic; could be more sophisticated
        static UPDATE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = UPDATE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if count % 100 == 0 {
            self.persist_snapshot().await;
        }
    }

    /// Remove a substance from the snapshot
    async fn remove_from_snapshot(&self, name: &str) {
        self.snapshot
            .modify(|snapshot| {
                snapshot.remove_substance(name);
            })
            .await;

        info!(substance = name, "Removed deleted substance from snapshot");
        self.persist_snapshot().await;
    }

    /// Persist the snapshot to disk
    async fn persist_snapshot(&self) {
        let snapshot = self.snapshot.get().await;
        if let Err(e) = persist_to_disk(&snapshot, &self.config.cache_path).await {
            error!(error = %e, "Failed to persist snapshot to disk");
        }
    }

    /// Check if it's time for periodic reconciliation
    async fn check_reconciliation(&self) {
        let mut last = self.last_reconciliation.lock().await;
        let interval = Duration::from_secs(self.config.reconciliation_interval_secs);

        if last.elapsed() >= interval {
            info!("Starting periodic reconciliation");
            *last = Instant::now();
            drop(last);

            if let Err(e) = self.run_reconciliation().await {
                error!(error = %e, "Reconciliation failed");
            }
        }
    }

    /// Run full reconciliation to detect new/deleted substances
    async fn run_reconciliation(&self) -> Result<(), BifrostError> {
        // Fetch all names from backend
        let backend_names: HashSet<String> = self
            .fetch_substance_names_only()
            .await?
            .into_iter()
            .collect();

        let snapshot = self.snapshot.get().await;
        let cached_names: HashSet<String> = snapshot
            .substances
            .iter()
            .filter_map(|s| s.name.clone())
            .collect();

        // Find new substances
        let new_names: Vec<String> = backend_names.difference(&cached_names).cloned().collect();

        // Find potentially deleted substances
        let missing_names: Vec<String> = cached_names.difference(&backend_names).cloned().collect();

        if !new_names.is_empty() {
            info!(count = new_names.len(), "New substances detected");
            // Add to queue for immediate fetch
            self.queue.add_many(new_names.clone()).await;
            for name in new_names {
                self.queue.expedite(&name).await;
            }
        }

        if !missing_names.is_empty() {
            info!(
                count = missing_names.len(),
                "Substances missing from backend, scheduling expedited revalidation"
            );
            for name in missing_names {
                self.queue.expedite(&name).await;
            }
        }

        // Persist snapshot
        self.persist_snapshot().await;

        info!("Reconciliation complete");
        Ok(())
    }

    /// Fetch only substance names (lightweight query)
    async fn fetch_substance_names_only(&self) -> Result<Vec<String>, BifrostError> {
        let items = self
            .api
            .ask_query("[[Category:Psychoactive substance]]", 9999, 0)
            .await?;

        Ok(items.into_iter().map(|item| item.fulltext).collect())
    }

    /// Get the queue for external access
    pub fn queue(&self) -> Arc<RevalidationQueue> {
        self.queue.clone()
    }

    /// Get the shaping controller for metrics
    pub fn shaping(&self) -> Arc<Mutex<AdaptiveShaping>> {
        self.shaping.clone()
    }

    /// Get the snapshot holder
    pub fn snapshot(&self) -> SnapshotHolder {
        self.snapshot.clone()
    }
}

/// Job context for a single revalidation
struct RevalidatorJob {
    api: PsychonautApi,
    parser: WikitextParser,
    queue: Arc<RevalidationQueue>,
    shaping: Arc<Mutex<AdaptiveShaping>>,
    config: RevalidatorConfig,
}

impl RevalidatorJob {
    /// Revalidate a single substance
    async fn revalidate_substance(&self, name: &str) -> RevalidationJobResult {
        let start = Instant::now();
        self.queue.mark_attempt_start(name).await;

        debug!(substance = name, "Starting revalidation");

        // Step 1: Browse by subject for core data
        let browse_result = self.api.browse_by_subject(name).await;

        let raw_data = match browse_result {
            Ok(data) => {
                // Check if empty (substance deleted)
                if data
                    .get("query")
                    .and_then(|q| q.get("data"))
                    .map(|d| d.is_null() || (d.is_array() && d.as_array().unwrap().is_empty()))
                    .unwrap_or(true)
                {
                    return RevalidationJobResult {
                        substance: None,
                        outcome: JobOutcome::NotFound,
                        latency_ms: start.elapsed().as_millis() as u64,
                    };
                }
                data
            }
            Err(e) => {
                if e.to_string().contains("404") || e.to_string().contains("not found") {
                    return RevalidationJobResult {
                        substance: None,
                        outcome: JobOutcome::NotFound,
                        latency_ms: start.elapsed().as_millis() as u64,
                    };
                }
                return RevalidationJobResult {
                    substance: None,
                    outcome: JobOutcome::Error(e.to_string()),
                    latency_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        // Step 2: Parse the raw data
        let mut parsed = match self.parser.parse_smw(raw_data) {
            Ok(p) => p,
            Err(e) => {
                return RevalidationJobResult {
                    substance: None,
                    outcome: JobOutcome::Error(format!("Parse error: {}", e)),
                    latency_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        // Add name and URL
        if let Some(obj) = parsed.as_object_mut() {
            obj.insert(
                "name".to_string(),
                serde_json::Value::String(name.to_string()),
            );
            obj.insert(
                "url".to_string(),
                serde_json::Value::String(format!(
                    "https://psychonautwiki.org/wiki/{}",
                    urlencoding::encode(name)
                )),
            );
        }

        // Step 3: Convert to Substance
        let mut substance: Substance = match serde_json::from_value(parsed) {
            Ok(s) => s,
            Err(e) => {
                return RevalidationJobResult {
                    substance: None,
                    outcome: JobOutcome::Error(format!("Deserialization error: {}", e)),
                    latency_ms: start.elapsed().as_millis() as u64,
                };
            }
        };

        // Step 4: Fetch effects (optional, continue on failure)
        match self.fetch_effects(name).await {
            Ok(effects) => substance.effects_cache = Some(effects),
            Err(e) => {
                warn!(substance = name, error = %e, "Failed to fetch effects");
                substance.effects_cache = Some(vec![]);
            }
        }

        // Step 5: Fetch summary (optional, continue on failure)
        match self.fetch_summary(name).await {
            Ok(summary) => substance.summary_cache = summary,
            Err(e) => {
                warn!(substance = name, error = %e, "Failed to fetch summary");
                substance.summary_cache = None;
            }
        }

        // Step 6: Fetch images (optional, continue on failure)
        match self.fetch_images(name).await {
            Ok(images) => substance.images_cache = images,
            Err(e) => {
                warn!(substance = name, error = %e, "Failed to fetch images");
                substance.images_cache = None;
            }
        }

        debug!(substance = name, "Revalidation successful");

        RevalidationJobResult {
            substance: Some(substance),
            outcome: JobOutcome::Success,
            latency_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Fetch effects for a substance
    async fn fetch_effects(&self, name: &str) -> Result<Vec<Effect>, BifrostError> {
        let query = format!("[[:{}]]|?Effect", name);
        let json = self.api.ask_query(&query, 100, 0).await;

        // The ask query returns effects in printouts
        // For now, return empty if we can't parse - effects can come from another query
        match json {
            Ok(items) => {
                // Effects are usually in printouts, not as separate items
                // This is a simplified implementation
                Ok(items
                    .into_iter()
                    .map(|item| Effect {
                        name: Some(item.fulltext),
                        url: Some(item.fullurl),
                    })
                    .collect())
            }
            Err(_) => Ok(vec![]),
        }
    }

    /// Fetch summary for a substance
    async fn fetch_summary(&self, name: &str) -> Result<Option<String>, BifrostError> {
        let raw = self.api.parse_text(name).await?;

        // Clean HTML tags
        let re = regex::Regex::new(r"<[^>]*>").unwrap();
        let no_tags = re.replace_all(&raw, "").to_string();

        // Clean and format
        let cleaned = no_tags
            .trim()
            .replace("[edit]", "")
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .take(2)
            .collect::<Vec<&str>>()
            .join(" ");

        if cleaned.is_empty() {
            Ok(None)
        } else {
            Ok(Some(cleaned))
        }
    }

    /// Fetch images for a substance
    async fn fetch_images(&self, name: &str) -> Result<Option<Vec<SubstanceImage>>, BifrostError> {
        let images = self.api.parse_images(name).await?;

        if images.is_empty() {
            return Ok(None);
        }

        let mapped: Vec<SubstanceImage> = images
            .into_iter()
            .map(|filename| {
                let mut hasher = md5::Md5::new();
                hasher.update(filename.as_bytes());
                let hash = hasher.finalize();
                let hash_hex = hex::encode(hash);
                let a = &hash_hex[0..1];
                let ab = &hash_hex[0..2];

                SubstanceImage {
                    thumb: Some(format!(
                        "{}w/thumb.php?f={}&width={}",
                        self.config.cdn_url, filename, self.config.thumb_size
                    )),
                    image: Some(format!(
                        "{}w/images/{}/{}/{}",
                        self.config.cdn_url, a, ab, filename
                    )),
                }
            })
            .collect();

        Ok(Some(mapped))
    }
}

/// Outcome of a revalidation job
#[derive(Debug)]
enum JobOutcome {
    Success,
    NotFound,
    Error(String),
}

impl JobOutcome {
    fn into_revalidation_outcome(self) -> RevalidationOutcome {
        match self {
            JobOutcome::Success => RevalidationOutcome::Success,
            JobOutcome::NotFound => RevalidationOutcome::NotFound,
            JobOutcome::Error(e) => RevalidationOutcome::Error(e),
        }
    }
}

/// Result of a revalidation job
#[derive(Debug)]
struct RevalidationJobResult {
    substance: Option<Substance>,
    outcome: JobOutcome,
    latency_ms: u64,
}

impl RevalidationJobResult {
    fn outcome_success(&self) -> bool {
        matches!(self.outcome, JobOutcome::Success)
    }

    fn to_outcome(self) -> RevalidationOutcome {
        match self.outcome {
            JobOutcome::Success => RevalidationOutcome::Success,
            JobOutcome::NotFound => RevalidationOutcome::NotFound,
            JobOutcome::Error(e) => RevalidationOutcome::Error(e),
        }
    }
}

/// Check for new substances (lightweight, names only)
pub async fn check_for_new_substances(
    api: &PsychonautApi,
    snapshot: &SnapshotHolder,
) -> Result<Vec<String>, BifrostError> {
    // Fetch all names from backend
    let items = api
        .ask_query("[[Category:Psychoactive substance]]", 9999, 0)
        .await?;

    let backend_names: HashSet<String> = items.into_iter().map(|item| item.fulltext).collect();

    let current = snapshot.get().await;
    let cached_names: HashSet<String> = current
        .substances
        .iter()
        .filter_map(|s| s.name.clone())
        .collect();

    let new_names: Vec<String> = backend_names.difference(&cached_names).cloned().collect();

    Ok(new_names)
}

/// Fetch full data for a list of substances
pub async fn fetch_substances_by_names(
    api: &PsychonautApi,
    parser: &WikitextParser,
    names: &[String],
    config: &RevalidatorConfig,
) -> Result<Vec<Substance>, BifrostError> {
    use futures::stream::{self, StreamExt};

    let substances: Vec<Substance> = stream::iter(names)
        .map(|name| {
            let api = api.clone();
            let parser = parser.clone();
            let _config = config.clone();
            async move {
                // Simplified fetch - just core data
                let raw = match api.browse_by_subject(name).await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(substance = name, error = %e, "Failed to fetch substance");
                        return None;
                    }
                };

                let mut parsed = match parser.parse_smw(raw) {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(substance = name, error = %e, "Failed to parse substance");
                        return None;
                    }
                };

                if let Some(obj) = parsed.as_object_mut() {
                    obj.insert("name".to_string(), serde_json::Value::String(name.clone()));
                    obj.insert(
                        "url".to_string(),
                        serde_json::Value::String(format!(
                            "https://psychonautwiki.org/wiki/{}",
                            urlencoding::encode(name)
                        )),
                    );
                }

                match serde_json::from_value::<Substance>(parsed) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        warn!(substance = name, error = %e, "Failed to deserialize substance");
                        None
                    }
                }
            }
        })
        .buffer_unordered(10)
        .filter_map(|x| async { x })
        .collect()
        .await;

    Ok(substances)
}
