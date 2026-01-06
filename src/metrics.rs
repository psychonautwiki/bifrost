//! Prometheus metrics for monitoring bifrost.
//!
//! This module provides comprehensive metrics for cache state, query performance,
//! revalidation operations, and backend health.

use prometheus::{
    Counter, CounterVec, Gauge, Histogram, HistogramOpts, HistogramVec, Opts, Registry,
};
use std::sync::Arc;
use tracing::error;

/// All metrics for the bifrost service
pub struct Metrics {
    pub registry: Registry,

    // Cache metrics
    pub cache_substances_total: Gauge,
    pub cache_snapshot_age_seconds: Gauge,
    pub cache_snapshot_build_duration_seconds: Gauge,
    pub cache_disk_size_bytes: Gauge,
    pub cache_index_by_name_size: Gauge,
    pub cache_index_by_chemical_class_size: Gauge,
    pub cache_index_by_psychoactive_class_size: Gauge,
    pub cache_index_by_effect_size: Gauge,
    pub cache_index_trigrams_total: Gauge,

    // Query metrics
    pub query_duration_seconds: HistogramVec,
    pub query_results_total: CounterVec,
    pub query_total: CounterVec,

    // Revalidation metrics
    pub revalidation_queue_size: Gauge,
    pub revalidation_queue_due: Gauge,
    pub revalidation_in_flight: Gauge,
    pub revalidation_jobs_total: CounterVec,
    pub revalidation_duration_seconds: Histogram,
    pub revalidation_retries_total: Counter,
    pub revalidation_unique_failures_total: Gauge,

    // Adaptive shaping metrics
    pub shaping_concurrency_current: Gauge,
    pub shaping_rate_limit_delay_ms: Gauge,
    pub shaping_circuit_breaker_open: Gauge,
    pub shaping_actions_total: CounterVec,
    pub shaping_error_rate: Gauge,
    pub shaping_p99_latency_ms: Gauge,
    pub shaping_healthy_duration_seconds: Gauge,

    // Backend metrics
    pub backend_requests_total: CounterVec,
    pub backend_request_duration_seconds: HistogramVec,
    pub backend_retries_total: CounterVec,
    pub backend_bytes_received_total: Counter,

    // System metrics
    pub uptime_seconds: Gauge,
    pub boot_type: CounterVec,
    pub boot_duration_seconds: Gauge,
}

impl Metrics {
    /// Create a new metrics registry with all metrics
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        // Cache metrics
        let cache_substances_total = Gauge::with_opts(Opts::new(
            "bifrost_cache_substances_total",
            "Number of substances in the snapshot",
        ))?;
        registry.register(Box::new(cache_substances_total.clone()))?;

        let cache_snapshot_age_seconds = Gauge::with_opts(Opts::new(
            "bifrost_cache_snapshot_age_seconds",
            "Time since snapshot was built",
        ))?;
        registry.register(Box::new(cache_snapshot_age_seconds.clone()))?;

        let cache_snapshot_build_duration_seconds = Gauge::with_opts(Opts::new(
            "bifrost_cache_snapshot_build_duration_seconds",
            "How long the snapshot build took",
        ))?;
        registry.register(Box::new(cache_snapshot_build_duration_seconds.clone()))?;

        let cache_disk_size_bytes = Gauge::with_opts(Opts::new(
            "bifrost_cache_disk_size_bytes",
            "Size of persisted cache on disk",
        ))?;
        registry.register(Box::new(cache_disk_size_bytes.clone()))?;

        let cache_index_by_name_size = Gauge::with_opts(Opts::new(
            "bifrost_cache_index_by_name_size",
            "Size of the by_name index",
        ))?;
        registry.register(Box::new(cache_index_by_name_size.clone()))?;

        let cache_index_by_chemical_class_size = Gauge::with_opts(Opts::new(
            "bifrost_cache_index_by_chemical_class_size",
            "Size of the by_chemical_class index",
        ))?;
        registry.register(Box::new(cache_index_by_chemical_class_size.clone()))?;

        let cache_index_by_psychoactive_class_size = Gauge::with_opts(Opts::new(
            "bifrost_cache_index_by_psychoactive_class_size",
            "Size of the by_psychoactive_class index",
        ))?;
        registry.register(Box::new(cache_index_by_psychoactive_class_size.clone()))?;

        let cache_index_by_effect_size = Gauge::with_opts(Opts::new(
            "bifrost_cache_index_by_effect_size",
            "Size of the by_effect index",
        ))?;
        registry.register(Box::new(cache_index_by_effect_size.clone()))?;

        let cache_index_trigrams_total = Gauge::with_opts(Opts::new(
            "bifrost_cache_index_trigrams_total",
            "Number of trigrams indexed",
        ))?;
        registry.register(Box::new(cache_index_trigrams_total.clone()))?;

        // Query metrics
        let query_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "bifrost_query_duration_seconds",
                "Query duration in seconds",
            )
            .buckets(vec![
                0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0,
            ]),
            &["query_type"],
        )?;
        registry.register(Box::new(query_duration_seconds.clone()))?;

        let query_results_total = CounterVec::new(
            Opts::new(
                "bifrost_query_results_total",
                "Number of results returned by queries",
            ),
            &["query_type"],
        )?;
        registry.register(Box::new(query_results_total.clone()))?;

        let query_total = CounterVec::new(
            Opts::new("bifrost_query_total", "Total number of queries"),
            &["query_type", "status"],
        )?;
        registry.register(Box::new(query_total.clone()))?;

        // Revalidation metrics
        let revalidation_queue_size = Gauge::with_opts(Opts::new(
            "bifrost_revalidation_queue_size",
            "Total items in revalidation queue",
        ))?;
        registry.register(Box::new(revalidation_queue_size.clone()))?;

        let revalidation_queue_due = Gauge::with_opts(Opts::new(
            "bifrost_revalidation_queue_due",
            "Items due for revalidation",
        ))?;
        registry.register(Box::new(revalidation_queue_due.clone()))?;

        let revalidation_in_flight = Gauge::with_opts(Opts::new(
            "bifrost_revalidation_in_flight",
            "Currently processing revalidation jobs",
        ))?;
        registry.register(Box::new(revalidation_in_flight.clone()))?;

        let revalidation_jobs_total = CounterVec::new(
            Opts::new(
                "bifrost_revalidation_jobs_total",
                "Total revalidation jobs by status",
            ),
            &["status"],
        )?;
        registry.register(Box::new(revalidation_jobs_total.clone()))?;

        let revalidation_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "bifrost_revalidation_duration_seconds",
                "Revalidation job duration",
            )
            .buckets(vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]),
        )?;
        registry.register(Box::new(revalidation_duration_seconds.clone()))?;

        let revalidation_retries_total = Counter::with_opts(Opts::new(
            "bifrost_revalidation_retries_total",
            "Total revalidation retry attempts",
        ))?;
        registry.register(Box::new(revalidation_retries_total.clone()))?;

        let revalidation_unique_failures_total = Gauge::with_opts(Opts::new(
            "bifrost_revalidation_unique_failures_total",
            "Distinct substances failing in window",
        ))?;
        registry.register(Box::new(revalidation_unique_failures_total.clone()))?;

        // Adaptive shaping metrics
        let shaping_concurrency_current = Gauge::with_opts(Opts::new(
            "bifrost_shaping_concurrency_current",
            "Current concurrency limit",
        ))?;
        registry.register(Box::new(shaping_concurrency_current.clone()))?;

        let shaping_rate_limit_delay_ms = Gauge::with_opts(Opts::new(
            "bifrost_shaping_rate_limit_delay_ms",
            "Current rate limit delay in milliseconds",
        ))?;
        registry.register(Box::new(shaping_rate_limit_delay_ms.clone()))?;

        let shaping_circuit_breaker_open = Gauge::with_opts(Opts::new(
            "bifrost_shaping_circuit_breaker_open",
            "Whether circuit breaker is open (0 or 1)",
        ))?;
        registry.register(Box::new(shaping_circuit_breaker_open.clone()))?;

        let shaping_actions_total = CounterVec::new(
            Opts::new("bifrost_shaping_actions_total", "Shaping actions taken"),
            &["action"],
        )?;
        registry.register(Box::new(shaping_actions_total.clone()))?;

        let shaping_error_rate = Gauge::with_opts(Opts::new(
            "bifrost_shaping_error_rate",
            "Current error rate (0.0-1.0)",
        ))?;
        registry.register(Box::new(shaping_error_rate.clone()))?;

        let shaping_p99_latency_ms = Gauge::with_opts(Opts::new(
            "bifrost_shaping_p99_latency_ms",
            "Current p99 latency in milliseconds",
        ))?;
        registry.register(Box::new(shaping_p99_latency_ms.clone()))?;

        let shaping_healthy_duration_seconds = Gauge::with_opts(Opts::new(
            "bifrost_shaping_healthy_duration_seconds",
            "Time since last unhealthy event",
        ))?;
        registry.register(Box::new(shaping_healthy_duration_seconds.clone()))?;

        // Backend metrics
        let backend_requests_total = CounterVec::new(
            Opts::new("bifrost_backend_requests_total", "Backend API requests"),
            &["action", "status"],
        )?;
        registry.register(Box::new(backend_requests_total.clone()))?;

        let backend_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "bifrost_backend_request_duration_seconds",
                "Backend request duration",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
            &["action"],
        )?;
        registry.register(Box::new(backend_request_duration_seconds.clone()))?;

        let backend_retries_total = CounterVec::new(
            Opts::new("bifrost_backend_retries_total", "Backend request retries"),
            &["action"],
        )?;
        registry.register(Box::new(backend_retries_total.clone()))?;

        let backend_bytes_received_total = Counter::with_opts(Opts::new(
            "bifrost_backend_bytes_received_total",
            "Total bytes received from backend",
        ))?;
        registry.register(Box::new(backend_bytes_received_total.clone()))?;

        // System metrics
        let uptime_seconds = Gauge::with_opts(Opts::new(
            "bifrost_uptime_seconds",
            "Time since process start",
        ))?;
        registry.register(Box::new(uptime_seconds.clone()))?;

        let boot_type = CounterVec::new(
            Opts::new("bifrost_boot_type", "Boot type counter"),
            &["type"],
        )?;
        registry.register(Box::new(boot_type.clone()))?;

        let boot_duration_seconds = Gauge::with_opts(Opts::new(
            "bifrost_boot_duration_seconds",
            "Time taken to boot",
        ))?;
        registry.register(Box::new(boot_duration_seconds.clone()))?;

        Ok(Self {
            registry,
            cache_substances_total,
            cache_snapshot_age_seconds,
            cache_snapshot_build_duration_seconds,
            cache_disk_size_bytes,
            cache_index_by_name_size,
            cache_index_by_chemical_class_size,
            cache_index_by_psychoactive_class_size,
            cache_index_by_effect_size,
            cache_index_trigrams_total,
            query_duration_seconds,
            query_results_total,
            query_total,
            revalidation_queue_size,
            revalidation_queue_due,
            revalidation_in_flight,
            revalidation_jobs_total,
            revalidation_duration_seconds,
            revalidation_retries_total,
            revalidation_unique_failures_total,
            shaping_concurrency_current,
            shaping_rate_limit_delay_ms,
            shaping_circuit_breaker_open,
            shaping_actions_total,
            shaping_error_rate,
            shaping_p99_latency_ms,
            shaping_healthy_duration_seconds,
            backend_requests_total,
            backend_request_duration_seconds,
            backend_retries_total,
            backend_bytes_received_total,
            uptime_seconds,
            boot_type,
            boot_duration_seconds,
        })
    }

    /// Record a query execution
    pub fn record_query(
        &self,
        query_type: &str,
        status: &str,
        duration_secs: f64,
        result_count: u64,
    ) {
        self.query_duration_seconds
            .with_label_values(&[query_type])
            .observe(duration_secs);
        self.query_results_total
            .with_label_values(&[query_type])
            .inc_by(result_count as f64);
        self.query_total
            .with_label_values(&[query_type, status])
            .inc();
    }

    /// Record a revalidation job completion
    pub fn record_revalidation(&self, status: &str, duration_secs: f64) {
        self.revalidation_jobs_total
            .with_label_values(&[status])
            .inc();
        self.revalidation_duration_seconds.observe(duration_secs);
    }

    /// Record a shaping action
    pub fn record_shaping_action(&self, action: &str) {
        self.shaping_actions_total
            .with_label_values(&[action])
            .inc();
    }

    /// Record a backend request
    pub fn record_backend_request(&self, action: &str, status: &str, duration_secs: f64) {
        self.backend_requests_total
            .with_label_values(&[action, status])
            .inc();
        self.backend_request_duration_seconds
            .with_label_values(&[action])
            .observe(duration_secs);
    }

    /// Record a backend retry
    pub fn record_backend_retry(&self, action: &str) {
        self.backend_retries_total
            .with_label_values(&[action])
            .inc();
    }

    /// Update cache metrics from snapshot
    pub fn update_cache_metrics(&self, snapshot: &crate::cache::snapshot::SubstanceSnapshot) {
        self.cache_substances_total
            .set(snapshot.substances.len() as f64);
        self.cache_snapshot_age_seconds
            .set(snapshot.meta.created_at.elapsed().as_secs_f64());
        self.cache_snapshot_build_duration_seconds
            .set(snapshot.meta.build_duration_ms as f64 / 1000.0);
        self.cache_index_by_name_size
            .set(snapshot.by_name.len() as f64);
        self.cache_index_by_chemical_class_size
            .set(snapshot.by_chemical_class.len() as f64);
        self.cache_index_by_psychoactive_class_size
            .set(snapshot.by_psychoactive_class.len() as f64);
        self.cache_index_by_effect_size
            .set(snapshot.by_effect.len() as f64);
        self.cache_index_trigrams_total
            .set(snapshot.trigram_index.len() as f64);
    }

    /// Update queue metrics
    pub fn update_queue_metrics(&self, stats: &crate::cache::revalidation::QueueStats) {
        self.revalidation_queue_size.set(stats.total as f64);
        self.revalidation_queue_due.set(stats.due as f64);
        self.revalidation_in_flight.set(stats.in_flight as f64);
    }

    /// Update shaping metrics
    pub fn update_shaping_metrics(&self, shaping: &crate::cache::shaping::AdaptiveShaping) {
        self.shaping_concurrency_current
            .set(shaping.current_concurrency() as f64);
        self.shaping_rate_limit_delay_ms
            .set(shaping.get_rate_limit().as_millis() as f64);
        self.shaping_circuit_breaker_open
            .set(if shaping.is_circuit_broken() {
                1.0
            } else {
                0.0
            });
        self.shaping_error_rate.set(shaping.error_rate());
        self.shaping_p99_latency_ms
            .set(shaping.p99_latency_ms() as f64);
        self.shaping_healthy_duration_seconds
            .set(shaping.healthy_duration_secs() as f64);
    }

    /// Render metrics in Prometheus text format
    pub fn render(&self) -> String {
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();

        match encoder.encode_to_string(&metric_families) {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "Failed to encode metrics");
                String::new()
            }
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new().expect("Failed to create metrics")
    }
}

/// Shared metrics instance
pub type SharedMetrics = Arc<Metrics>;

/// Create a shared metrics instance
pub fn create_metrics() -> SharedMetrics {
    Arc::new(Metrics::new().expect("Failed to create metrics"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new().unwrap();
        assert!(metrics.render().contains("bifrost_cache_substances_total"));
    }

    #[test]
    fn test_query_recording() {
        let metrics = Metrics::new().unwrap();
        metrics.record_query("substances", "success", 0.001, 10);

        let output = metrics.render();
        assert!(output.contains("bifrost_query_total"));
        assert!(output.contains("bifrost_query_duration_seconds"));
    }
}
