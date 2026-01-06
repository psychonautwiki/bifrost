//! Adaptive queue shaping for backend health-aware throttling.
//!
//! This module monitors revalidation job outcomes and adjusts throughput
//! based on error rates, latency, and failure patterns.

use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Record of a single revalidation attempt
#[derive(Debug, Clone)]
pub struct AttemptRecord {
    /// When the attempt was made
    pub timestamp: Instant,
    /// Which substance was being revalidated
    pub substance: String,
    /// Whether the attempt succeeded
    pub success: bool,
    /// How long the attempt took
    pub latency_ms: u64,
}

/// Health metrics computed over a sliding window
#[derive(Debug, Clone)]
pub struct HealthMetrics {
    /// Size of the sliding window
    window_size: Duration,
    /// Ring buffer of recent attempts
    recent_attempts: VecDeque<AttemptRecord>,
    /// Currently active concurrency level
    pub current_concurrency: usize,
    /// Maximum concurrency level
    max_concurrency: usize,
}

impl HealthMetrics {
    /// Create a new health metrics tracker
    pub fn new(window_size: Duration, max_concurrency: usize) -> Self {
        Self {
            window_size,
            recent_attempts: VecDeque::new(),
            current_concurrency: max_concurrency,
            max_concurrency,
        }
    }

    /// Record an attempt
    pub fn record(&mut self, substance: String, success: bool, latency_ms: u64) {
        let now = Instant::now();

        // Prune old entries
        let cutoff = now - self.window_size;
        while let Some(front) = self.recent_attempts.front() {
            if front.timestamp < cutoff {
                self.recent_attempts.pop_front();
            } else {
                break;
            }
        }

        // Add new record
        self.recent_attempts.push_back(AttemptRecord {
            timestamp: now,
            substance,
            success,
            latency_ms,
        });
    }

    /// Get the current error rate (0.0 to 1.0)
    pub fn error_rate(&self) -> f64 {
        if self.recent_attempts.is_empty() {
            return 0.0;
        }

        let failures = self.recent_attempts.iter().filter(|r| !r.success).count();
        failures as f64 / self.recent_attempts.len() as f64
    }

    /// Get the count of unique failing substances
    pub fn unique_failing_substances(&self) -> HashSet<String> {
        self.recent_attempts
            .iter()
            .filter(|r| !r.success)
            .map(|r| r.substance.clone())
            .collect()
    }

    /// Get the total number of failures in the window
    pub fn total_failures(&self) -> usize {
        self.recent_attempts.iter().filter(|r| !r.success).count()
    }

    /// Get the p95 latency in milliseconds
    pub fn p95_latency_ms(&self) -> u64 {
        if self.recent_attempts.is_empty() {
            return 0;
        }

        let mut latencies: Vec<u64> = self.recent_attempts.iter().map(|r| r.latency_ms).collect();
        latencies.sort_unstable();

        let idx = (latencies.len() as f64 * 0.95) as usize;
        latencies
            .get(idx.min(latencies.len() - 1))
            .copied()
            .unwrap_or(0)
    }

    /// Get the p99 latency in milliseconds
    pub fn p99_latency_ms(&self) -> u64 {
        if self.recent_attempts.is_empty() {
            return 0;
        }

        let mut latencies: Vec<u64> = self.recent_attempts.iter().map(|r| r.latency_ms).collect();
        latencies.sort_unstable();

        let idx = (latencies.len() as f64 * 0.99) as usize;
        latencies
            .get(idx.min(latencies.len() - 1))
            .copied()
            .unwrap_or(0)
    }

    /// Get throughput (successful jobs per second)
    pub fn throughput(&self) -> f64 {
        if self.recent_attempts.is_empty() {
            return 0.0;
        }

        let successes = self.recent_attempts.iter().filter(|r| r.success).count();
        let window_secs = self.window_size.as_secs_f64();

        successes as f64 / window_secs
    }

    /// Get the number of attempts in the window
    pub fn attempt_count(&self) -> usize {
        self.recent_attempts.len()
    }

    /// Check if we've been healthy for a given duration
    /// "Healthy" means no failures in recent window portion
    pub fn healthy_duration(&self) -> Duration {
        let now = Instant::now();

        // Find the most recent failure
        let last_failure = self
            .recent_attempts
            .iter()
            .rev()
            .find(|r| !r.success)
            .map(|r| r.timestamp);

        match last_failure {
            Some(ts) => now.duration_since(ts),
            None => {
                // No failures in window - we've been healthy for at least window_size
                // or since the first attempt if we have any
                if let Some(first) = self.recent_attempts.front() {
                    now.duration_since(first.timestamp)
                } else {
                    Duration::ZERO
                }
            }
        }
    }

    /// Set the current concurrency level
    pub fn set_concurrency(&mut self, level: usize) {
        self.current_concurrency = level.min(self.max_concurrency).max(1);
    }
}

/// Thresholds for adaptive shaping decisions
#[derive(Debug, Clone)]
pub struct AdaptiveThresholds {
    /// Error rate at which to start monitoring (10%)
    pub error_rate_warning: f64,
    /// Error rate at which to reduce concurrency (25%)
    pub error_rate_critical: f64,
    /// Error rate at which to circuit break (50%)
    pub error_rate_emergency: f64,
    /// p99 latency threshold for warning (baseline × 2.0)
    pub latency_p99_warning_ms: u64,
    /// p99 latency threshold for critical (baseline × 5.0)
    pub latency_p99_critical_ms: u64,
    /// Duration of healthy operation required for recovery (30s)
    pub healthy_window_secs: u64,
    /// Threshold for distinguishing bad data vs bad backend (80%)
    pub failure_diversity_threshold: f64,
}

impl AdaptiveThresholds {
    /// Create thresholds with a baseline latency
    pub fn with_baseline(baseline_latency_ms: u64) -> Self {
        Self {
            error_rate_warning: 0.10,
            error_rate_critical: 0.25,
            error_rate_emergency: 0.50,
            latency_p99_warning_ms: baseline_latency_ms.saturating_mul(2),
            latency_p99_critical_ms: baseline_latency_ms.saturating_mul(5),
            healthy_window_secs: 30,
            failure_diversity_threshold: 0.80,
        }
    }

    /// Default thresholds (500ms baseline)
    pub fn default() -> Self {
        Self::with_baseline(500)
    }
}

/// Actions that can be taken by the shaping system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapingAction {
    /// No change needed
    Maintain,
    /// Decrease permits by 1 (min 1)
    ReduceConcurrency,
    /// Increase permits by 1 (max configured)
    IncreaseConcurrency,
    /// Insert delay between jobs
    RateLimit(u64), // milliseconds
    /// Pause all processing
    CircuitBreak,
    /// Resume from circuit break
    CircuitRecover,
}

/// State for the shaping system with hysteresis
#[derive(Debug, Clone)]
pub struct ShapingState {
    /// Current concurrency limit
    pub current_concurrency: usize,
    /// Maximum allowed concurrency
    max_concurrency: usize,
    /// Current rate limit delay
    pub rate_limit_delay: Duration,
    /// Whether the circuit breaker is open
    pub circuit_broken: bool,
    /// When the last adjustment was made
    last_adjustment: Instant,
    /// Cooldown between adjustments (to prevent oscillation)
    adjustment_cooldown: Duration,
}

impl ShapingState {
    /// Create a new shaping state
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            current_concurrency: max_concurrency,
            max_concurrency,
            rate_limit_delay: Duration::ZERO,
            circuit_broken: false,
            last_adjustment: Instant::now() - Duration::from_secs(60), // Allow immediate adjustment
            adjustment_cooldown: Duration::from_secs(10),
        }
    }

    /// Apply a shaping action, returning whether the action was applied
    pub fn apply(&mut self, action: ShapingAction) -> bool {
        // Circuit breaker actions bypass cooldown
        if !matches!(
            action,
            ShapingAction::CircuitBreak | ShapingAction::CircuitRecover
        ) {
            // Enforce cooldown to prevent oscillation
            if self.last_adjustment.elapsed() < self.adjustment_cooldown {
                return false;
            }
        }

        match action {
            ShapingAction::Maintain => false,
            ShapingAction::ReduceConcurrency => {
                if self.current_concurrency > 1 {
                    self.current_concurrency -= 1;
                    self.last_adjustment = Instant::now();
                    info!(
                        new_concurrency = self.current_concurrency,
                        "Reduced concurrency"
                    );
                    true
                } else {
                    false
                }
            }
            ShapingAction::IncreaseConcurrency => {
                if self.current_concurrency < self.max_concurrency {
                    self.current_concurrency += 1;
                    self.last_adjustment = Instant::now();
                    info!(
                        new_concurrency = self.current_concurrency,
                        "Increased concurrency"
                    );
                    true
                } else {
                    false
                }
            }
            ShapingAction::RateLimit(delay_ms) => {
                self.rate_limit_delay = Duration::from_millis(delay_ms);
                debug!(delay_ms = delay_ms, "Applied rate limit");
                true
            }
            ShapingAction::CircuitBreak => {
                if !self.circuit_broken {
                    self.circuit_broken = true;
                    self.last_adjustment = Instant::now();
                    warn!("Circuit breaker OPENED - pausing revalidation");
                    true
                } else {
                    false
                }
            }
            ShapingAction::CircuitRecover => {
                if self.circuit_broken {
                    self.circuit_broken = false;
                    // Recover to half capacity
                    self.current_concurrency = (self.max_concurrency / 2).max(1);
                    self.rate_limit_delay = Duration::ZERO;
                    self.last_adjustment = Instant::now();
                    info!(
                        new_concurrency = self.current_concurrency,
                        "Circuit breaker CLOSED - resuming at reduced capacity"
                    );
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Check if processing should pause
    pub fn should_pause(&self) -> bool {
        self.circuit_broken
    }

    /// Get the current rate limit delay
    pub fn get_rate_limit(&self) -> Duration {
        self.rate_limit_delay
    }
}

/// Evaluate health metrics and determine shaping action
pub fn evaluate_health(
    metrics: &HealthMetrics,
    thresholds: &AdaptiveThresholds,
    state: &ShapingState,
) -> ShapingAction {
    let error_rate = metrics.error_rate();
    let p99_latency = metrics.p99_latency_ms();
    let healthy_duration = metrics.healthy_duration();

    // Circuit breaker check - highest priority
    if error_rate >= thresholds.error_rate_emergency {
        return ShapingAction::CircuitBreak;
    }

    // Circuit is broken - check for recovery
    if state.circuit_broken {
        if healthy_duration >= Duration::from_secs(thresholds.healthy_window_secs) {
            return ShapingAction::CircuitRecover;
        }
        return ShapingAction::Maintain;
    }

    // Recovery check - if healthy for threshold duration
    if healthy_duration >= Duration::from_secs(thresholds.healthy_window_secs) {
        if metrics.current_concurrency < state.current_concurrency {
            return ShapingAction::IncreaseConcurrency;
        }
        return ShapingAction::Maintain;
    }

    // Critical error rate
    if error_rate >= thresholds.error_rate_critical {
        let total_failures = metrics.total_failures();
        if total_failures > 0 {
            // Check if it's widespread (backend issue) or concentrated (bad data)
            let unique_failing = metrics.unique_failing_substances().len();
            let diversity = unique_failing as f64 / total_failures as f64;

            if diversity >= thresholds.failure_diversity_threshold {
                // Many different substances failing = backend problem
                return ShapingAction::ReduceConcurrency;
            }
            // Same few substances failing = bad data, maintain throughput
        }
        return ShapingAction::Maintain;
    }

    // Latency-based shaping
    if p99_latency >= thresholds.latency_p99_critical_ms {
        return ShapingAction::RateLimit(100);
    }

    if p99_latency >= thresholds.latency_p99_warning_ms {
        return ShapingAction::RateLimit(50);
    }

    ShapingAction::Maintain
}

/// Adaptive shaping controller combining metrics, thresholds, and state
pub struct AdaptiveShaping {
    pub metrics: HealthMetrics,
    pub thresholds: AdaptiveThresholds,
    pub state: ShapingState,
}

impl AdaptiveShaping {
    /// Create a new adaptive shaping controller
    pub fn new(max_concurrency: usize, baseline_latency_ms: u64) -> Self {
        Self {
            metrics: HealthMetrics::new(Duration::from_secs(60), max_concurrency),
            thresholds: AdaptiveThresholds::with_baseline(baseline_latency_ms),
            state: ShapingState::new(max_concurrency),
        }
    }

    /// Record an attempt and evaluate shaping
    pub fn record_and_evaluate(
        &mut self,
        substance: String,
        success: bool,
        latency_ms: u64,
    ) -> ShapingAction {
        self.metrics.record(substance, success, latency_ms);
        let action = evaluate_health(&self.metrics, &self.thresholds, &self.state);

        if self.state.apply(action) {
            // Update metrics with new concurrency if changed
            self.metrics.set_concurrency(self.state.current_concurrency);
        }

        action
    }

    /// Check if we should pause processing
    pub fn should_pause(&self) -> bool {
        self.state.should_pause()
    }

    /// Get the current rate limit delay
    pub fn get_rate_limit(&self) -> Duration {
        self.state.get_rate_limit()
    }

    /// Get the current concurrency level
    pub fn current_concurrency(&self) -> usize {
        self.state.current_concurrency
    }

    /// Check circuit breaker state
    pub fn is_circuit_broken(&self) -> bool {
        self.state.circuit_broken
    }

    /// Get current error rate
    pub fn error_rate(&self) -> f64 {
        self.metrics.error_rate()
    }

    /// Get current p99 latency
    pub fn p99_latency_ms(&self) -> u64 {
        self.metrics.p99_latency_ms()
    }

    /// Get healthy duration
    pub fn healthy_duration_secs(&self) -> u64 {
        self.metrics.healthy_duration().as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_rate_calculation() {
        let mut metrics = HealthMetrics::new(Duration::from_secs(60), 10);

        // Record 10 attempts, 3 failures (i=7,8,9 are failures when i < 7 is success)
        for i in 0..10 {
            metrics.record(format!("substance_{}", i), i < 7, 100);
        }

        let error_rate = metrics.error_rate();
        assert!((error_rate - 0.3).abs() < 0.01);
    }

    #[test]
    fn test_circuit_breaker() {
        let mut metrics = HealthMetrics::new(Duration::from_secs(60), 10);
        let thresholds = AdaptiveThresholds::default();
        let state = ShapingState::new(10);

        // Record lots of failures (>50%)
        for i in 0..20 {
            metrics.record(format!("substance_{}", i), i < 5, 100);
        }

        let action = evaluate_health(&metrics, &thresholds, &state);
        assert_eq!(action, ShapingAction::CircuitBreak);
    }

    #[test]
    fn test_latency_shaping() {
        let mut metrics = HealthMetrics::new(Duration::from_secs(60), 10);
        let thresholds = AdaptiveThresholds::with_baseline(100); // 100ms baseline
        let state = ShapingState::new(10);

        // Record attempts with high latency (>500ms = 5x baseline)
        for i in 0..10 {
            metrics.record(format!("substance_{}", i), true, 600);
        }

        let action = evaluate_health(&metrics, &thresholds, &state);
        assert_eq!(action, ShapingAction::RateLimit(100));
    }

    #[test]
    fn test_hysteresis() {
        let mut state = ShapingState::new(10);

        // First reduction should succeed
        assert!(state.apply(ShapingAction::ReduceConcurrency));
        assert_eq!(state.current_concurrency, 9);

        // Immediate second reduction should be blocked by cooldown
        assert!(!state.apply(ShapingAction::ReduceConcurrency));
        assert_eq!(state.current_concurrency, 9);
    }
}
