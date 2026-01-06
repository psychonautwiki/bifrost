//! Revalidation queue for background substance refresh.
//!
//! This module manages the queue of substances that need revalidation,
//! with retry logic, failure tracking, and deletion detection.

use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, info, warn};

/// State of a substance in the revalidation queue
#[derive(Debug, Clone)]
pub struct RevalidationItem {
    /// Substance name (primary key)
    pub substance_name: String,
    /// When this substance should next be refreshed
    pub next_refresh_at: Instant,
    /// Number of consecutive failures
    pub consecutive_failures: u8,
    /// Number of consecutive "not found" results (for deletion detection)
    pub consecutive_not_found: u8,
    /// When the last revalidation attempt was made
    pub last_attempt: Option<Instant>,
    /// When the last successful revalidation occurred
    pub last_success: Option<Instant>,
}

impl RevalidationItem {
    /// Create a new revalidation item for a substance
    pub fn new(substance_name: String, initial_delay: Duration) -> Self {
        Self {
            substance_name,
            next_refresh_at: Instant::now() + initial_delay,
            consecutive_failures: 0,
            consecutive_not_found: 0,
            last_attempt: None,
            last_success: None,
        }
    }
}

/// Outcome of a revalidation attempt
#[derive(Debug)]
pub enum RevalidationOutcome {
    /// Data fetched successfully
    Success,
    /// Substance deleted from wiki (empty browsebysubject)
    NotFound,
    /// Transient failure, retry later
    Error(String),
}

/// Revalidation queue managing all substances
pub struct RevalidationQueue {
    /// Substance name -> revalidation state
    items: Mutex<HashMap<String, RevalidationItem>>,
    /// Semaphore for concurrent job limiting
    semaphore: Arc<Semaphore>,
    /// Base TTL for cache entries
    base_ttl: Duration,
    /// Maximum concurrent jobs
    max_concurrency: usize,
    /// Flag to stop accepting new jobs
    accepting: Mutex<bool>,
    /// Count of in-flight jobs
    in_flight: Mutex<usize>,
}

impl RevalidationQueue {
    /// Create a new revalidation queue
    pub fn new(base_ttl: Duration, max_concurrency: usize) -> Self {
        Self {
            items: Mutex::new(HashMap::new()),
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            base_ttl,
            max_concurrency,
            accepting: Mutex::new(true),
            in_flight: Mutex::new(0),
        }
    }

    /// Add a substance to the queue with randomized initial delay
    pub async fn add(&self, substance_name: String) {
        let mut items = self.items.lock().await;
        if items.contains_key(&substance_name) {
            return;
        }

        // Randomize initial refresh within [0, TTL]
        let jitter =
            Duration::from_secs(rand::thread_rng().gen_range(0..self.base_ttl.as_secs().max(1)));

        items.insert(
            substance_name.clone(),
            RevalidationItem::new(substance_name, jitter),
        );
    }

    /// Add multiple substances to the queue
    pub async fn add_many(&self, names: Vec<String>) {
        let mut items = self.items.lock().await;
        let mut rng = rand::thread_rng();

        for name in names {
            if items.contains_key(&name) {
                continue;
            }

            let jitter = Duration::from_secs(rng.gen_range(0..self.base_ttl.as_secs().max(1)));

            items.insert(name.clone(), RevalidationItem::new(name, jitter));
        }
    }

    /// Select the next batch of substances to revalidate (randomized from due items)
    pub async fn select_next_batch(&self, max_count: usize) -> Vec<String> {
        let accepting = *self.accepting.lock().await;
        if !accepting {
            return vec![];
        }

        let items = self.items.lock().await;
        let now = Instant::now();

        // Collect all items past their deadline
        let mut due: Vec<&RevalidationItem> = items
            .values()
            .filter(|item| item.next_refresh_at <= now)
            .collect();

        if due.is_empty() {
            return vec![];
        }

        // Shuffle to randomize selection
        use rand::seq::SliceRandom;
        due.shuffle(&mut rand::thread_rng());

        // Take up to max_count
        due.into_iter()
            .take(max_count)
            .map(|item| item.substance_name.clone())
            .collect()
    }

    /// Get the number of items due for refresh
    pub async fn due_count(&self) -> usize {
        let items = self.items.lock().await;
        let now = Instant::now();
        items
            .values()
            .filter(|item| item.next_refresh_at <= now)
            .count()
    }

    /// Get the total number of items in the queue
    pub async fn len(&self) -> usize {
        self.items.lock().await.len()
    }

    /// Check if the queue is empty
    pub async fn is_empty(&self) -> bool {
        self.items.lock().await.is_empty()
    }

    /// Acquire a permit to run a revalidation job
    pub async fn acquire_permit(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let accepting = *self.accepting.lock().await;
        if !accepting {
            return None;
        }

        match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => {
                *self.in_flight.lock().await += 1;
                Some(permit)
            }
            Err(_) => None,
        }
    }

    /// Release tracking for an in-flight job (called when job completes)
    pub async fn release_in_flight(&self) {
        let mut count = self.in_flight.lock().await;
        *count = count.saturating_sub(1);
    }

    /// Mark the start of a revalidation attempt
    pub async fn mark_attempt_start(&self, name: &str) {
        let mut items = self.items.lock().await;
        if let Some(item) = items.get_mut(name) {
            item.last_attempt = Some(Instant::now());
        }
    }

    /// Handle the outcome of a revalidation attempt
    pub async fn handle_outcome(
        &self,
        name: &str,
        outcome: RevalidationOutcome,
    ) -> RevalidationAction {
        let mut items = self.items.lock().await;
        let base_ttl = self.base_ttl;

        if let Some(item) = items.get_mut(name) {
            match outcome {
                RevalidationOutcome::Success => {
                    item.consecutive_failures = 0;
                    item.consecutive_not_found = 0;
                    item.last_success = Some(Instant::now());

                    // Randomize next refresh within [0.6×TTL, 0.9×TTL]
                    let ttl_secs = base_ttl.as_secs();
                    let min_delay = (ttl_secs as f64 * 0.6) as u64;
                    let max_delay = (ttl_secs as f64 * 0.9) as u64;
                    let delay = Duration::from_secs(
                        rand::thread_rng().gen_range(min_delay.max(1)..max_delay.max(2)),
                    );
                    item.next_refresh_at = Instant::now() + delay;

                    debug!(
                        substance = name,
                        next_refresh_secs = delay.as_secs(),
                        "Revalidation successful, scheduled next refresh"
                    );

                    RevalidationAction::UpdateSnapshot
                }
                RevalidationOutcome::NotFound => {
                    item.consecutive_not_found += 1;

                    if item.consecutive_not_found >= 3 {
                        // Confirmed deletion: 3 consecutive "not found" results
                        info!(
                            substance = name,
                            consecutive_not_found = item.consecutive_not_found,
                            "Substance confirmed deleted, removing from cache"
                        );
                        let name_clone = name.to_string();
                        drop(items);
                        self.remove(&name_clone).await;
                        return RevalidationAction::RemoveFromSnapshot;
                    } else {
                        // Schedule recheck soon (5 minutes)
                        item.next_refresh_at = Instant::now() + Duration::from_secs(300);
                        debug!(
                            substance = name,
                            consecutive_not_found = item.consecutive_not_found,
                            "Substance not found, scheduling recheck"
                        );
                        RevalidationAction::None
                    }
                }
                RevalidationOutcome::Error(ref err) => {
                    item.consecutive_failures += 1;
                    // Don't increment not_found counter - this was an error, not a 404

                    let delay = match item.consecutive_failures {
                        1 => Duration::from_secs(rand::thread_rng().gen_range(30..60)),
                        2 => Duration::from_secs(rand::thread_rng().gen_range(60..120)),
                        _ => {
                            let ttl_secs = base_ttl.as_secs();
                            Duration::from_secs(
                                rand::thread_rng().gen_range(ttl_secs / 2..ttl_secs.max(1)),
                            )
                        }
                    };

                    item.next_refresh_at = Instant::now() + delay;

                    warn!(
                        substance = name,
                        consecutive_failures = item.consecutive_failures,
                        error = %err,
                        retry_secs = delay.as_secs(),
                        "Revalidation failed, scheduling retry"
                    );

                    RevalidationAction::None
                }
            }
        } else {
            RevalidationAction::None
        }
    }

    /// Remove a substance from the queue
    pub async fn remove(&self, name: &str) {
        self.items.lock().await.remove(name);
    }

    /// Expedite revalidation for a substance (for deletion checks)
    pub async fn expedite(&self, name: &str) {
        let mut items = self.items.lock().await;
        if let Some(item) = items.get_mut(name) {
            item.next_refresh_at = Instant::now();
            debug!(substance = name, "Expedited revalidation");
        }
    }

    /// Stop accepting new jobs (for shutdown)
    pub async fn stop_accepting(&self) {
        *self.accepting.lock().await = false;
        info!("Revalidation queue stopped accepting new jobs");
    }

    /// Get the count of in-flight jobs
    pub async fn in_flight_count(&self) -> usize {
        *self.in_flight.lock().await
    }

    /// Get statistics about the queue
    pub async fn stats(&self) -> QueueStats {
        let items = self.items.lock().await;
        let now = Instant::now();

        let mut due = 0;
        let mut failing = 0;
        let mut not_found = 0;

        for item in items.values() {
            if item.next_refresh_at <= now {
                due += 1;
            }
            if item.consecutive_failures > 0 {
                failing += 1;
            }
            if item.consecutive_not_found > 0 {
                not_found += 1;
            }
        }

        QueueStats {
            total: items.len(),
            due,
            failing,
            not_found,
            in_flight: *self.in_flight.lock().await,
        }
    }

    /// Get the item state for a specific substance
    pub async fn get_item(&self, name: &str) -> Option<RevalidationItem> {
        self.items.lock().await.get(name).cloned()
    }

    /// Get all substance names in the queue
    pub async fn all_names(&self) -> Vec<String> {
        self.items.lock().await.keys().cloned().collect()
    }

    /// Get the current concurrency limit
    pub fn max_concurrency(&self) -> usize {
        self.max_concurrency
    }

    /// Get available permits
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

/// Action to take after handling a revalidation outcome
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevalidationAction {
    /// No action needed
    None,
    /// Update the snapshot with new data
    UpdateSnapshot,
    /// Remove the substance from the snapshot
    RemoveFromSnapshot,
}

/// Statistics about the revalidation queue
#[derive(Debug, Clone)]
pub struct QueueStats {
    /// Total items in queue
    pub total: usize,
    /// Items due for refresh
    pub due: usize,
    /// Items with consecutive failures
    pub failing: usize,
    /// Items with consecutive not-found
    pub not_found: usize,
    /// Currently processing
    pub in_flight: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_queue_add_and_select() {
        let queue = RevalidationQueue::new(Duration::from_secs(3600), 10);

        // Add substances
        queue.add("LSD".to_string()).await;
        queue.add("MDMA".to_string()).await;

        assert_eq!(queue.len().await, 2);

        // Initially nothing should be due (randomized delay)
        // But we can check the queue isn't empty
        assert!(!queue.is_empty().await);
    }

    #[tokio::test]
    async fn test_outcome_handling() {
        let queue = RevalidationQueue::new(Duration::from_secs(3600), 10);
        queue.add("LSD".to_string()).await;

        // Success should reset counters
        let action = queue
            .handle_outcome("LSD", RevalidationOutcome::Success)
            .await;
        assert_eq!(action, RevalidationAction::UpdateSnapshot);

        let item = queue.get_item("LSD").await.unwrap();
        assert_eq!(item.consecutive_failures, 0);
        assert!(item.last_success.is_some());
    }

    #[tokio::test]
    async fn test_deletion_detection() {
        let queue = RevalidationQueue::new(Duration::from_secs(3600), 10);
        queue.add("DeletedSubstance".to_string()).await;

        // Three consecutive not-found should trigger removal
        queue
            .handle_outcome("DeletedSubstance", RevalidationOutcome::NotFound)
            .await;
        queue
            .handle_outcome("DeletedSubstance", RevalidationOutcome::NotFound)
            .await;
        let action = queue
            .handle_outcome("DeletedSubstance", RevalidationOutcome::NotFound)
            .await;

        assert_eq!(action, RevalidationAction::RemoveFromSnapshot);
        assert!(queue.get_item("DeletedSubstance").await.is_none());
    }
}
