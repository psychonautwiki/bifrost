//! Bifrost - PsychonautWiki GraphQL API Server
//!
//! A high-performance GraphQL API for substance information with:
//! - In-memory snapshot-based caching for sub-10ms query latency
//! - Background revalidation with adaptive shaping
//! - Prometheus metrics
//! - Disk persistence for crash recovery

mod cache;
mod config;
mod error;
mod graphql;
mod metrics;
mod services;
mod utils;

use crate::cache::{
    load_from_disk, persist_to_disk, Revalidator, SnapshotHolder, SubstanceSnapshot,
};
use crate::cache::snapshot::SubstanceAliases;
use crate::config::Config;
use crate::graphql::create_schema;
use crate::metrics::{create_metrics, SharedMetrics};
use crate::services::plebiscite::PlebisciteService;
use crate::services::psychonaut::api::PsychonautApi;
use crate::services::psychonaut::parser::WikitextParser;
use crate::services::reagents::{ReagentData, ReagentDataHolder};
use crate::utils::ascii::print_startup_banner;
use axum::{extract::State, http::header, response::IntoResponse, routing::get, Router};
use clap::Parser;
use futures::StreamExt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::watch;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Bifrost - PsychonautWiki GraphQL API
#[derive(Parser, Debug)]
#[command(name = "bifrost")]
#[command(author, version, about = "PsychonautWiki GraphQL API Server", long_about = None)]
struct Args {
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Enable JSON logging output
    #[arg(long)]
    json_logs: bool,

    /// Enable debug logging for backend API requests
    #[arg(long)]
    debug_requests: bool,

    /// Server port (overrides PORT env var)
    #[arg(short, long)]
    port: Option<u16>,
}

/// Application state shared across handlers
#[derive(Clone)]
struct AppState {
    schema: graphql::BifrostSchema,
    metrics: SharedMetrics,
    snapshot: SnapshotHolder,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let boot_start = Instant::now();

    // Load .env file first (before parsing args, so env vars are available)
    dotenvy::dotenv().ok();

    let args = Args::parse();

    // Initialize logging
    init_logging(&args.log_level, args.json_logs, args.debug_requests)?;

    // Print banner after logging is initialized
    print_startup_banner();

    info!("Starting Bifrost v{}", env!("CARGO_PKG_VERSION"));

    // Load config (CLI port overrides env var)
    let mut config = Config::from_env()?;
    if let Some(port) = args.port {
        config.server.port = port;
    }
    config.debug_requests = args.debug_requests;

    if args.debug_requests {
        info!("Request debugging enabled");
    }

    // Create metrics
    let metrics = create_metrics();

    // Initialize API client
    let api = PsychonautApi::new(&config.psychonaut.api_url);
    let parser = WikitextParser::new();

    // Initialize snapshot (from disk or cold start)
    let (snapshot_holder, boot_type) =
        initialize_snapshot(&config, &api, &parser, &metrics).await?;

    // Record boot metrics
    metrics.boot_type.with_label_values(&[boot_type]).inc();
    metrics
        .boot_duration_seconds
        .set(boot_start.elapsed().as_secs_f64());

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Create and start background revalidator
    let revalidator_config = config.to_revalidator_config();
    let revalidator = Revalidator::new(
        snapshot_holder.clone(),
        api.clone(),
        revalidator_config,
        shutdown_rx.clone(),
    );

    // Initialize the revalidation queue with all substances
    revalidator.initialize_queue().await;

    // Start the revalidator in a background task
    let revalidator_handle = {
        let revalidator = Arc::new(revalidator);
        let revalidator_clone = revalidator.clone();
        let metrics_clone = metrics.clone();

        tokio::spawn(async move {
            // Start metrics update loop
            let metrics_updater = {
                let revalidator = revalidator_clone.clone();
                let metrics = metrics_clone.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                        // Update cache metrics
                        let snapshot = revalidator.snapshot().get().await;
                        metrics.update_cache_metrics(&*snapshot);

                        // Update queue metrics
                        let stats = revalidator.queue().stats().await;
                        metrics.update_queue_metrics(&stats);

                        // Update shaping metrics
                        let shaping_arc = revalidator.shaping();
                        let shaping_guard = shaping_arc.lock().await;
                        metrics.update_shaping_metrics(&*shaping_guard);
                        drop(shaping_guard);
                    }
                })
            };

            // Run the main revalidation loop
            revalidator_clone.run().await;

            metrics_updater.abort();
        })
    };

    // Initialize Plebiscite service (MongoDB) if enabled
    let plebiscite_service = if config.features.plebiscite_enabled {
        info!("Plebiscite service enabled, connecting to MongoDB");
        match PlebisciteService::new(&config).await {
            Ok(service) => Some(Arc::new(service)),
            Err(e) => {
                error!("Failed to initialize Plebiscite service: {}", e);
                return Err(e.into());
            }
        }
    } else {
        info!("Plebiscite service disabled");
        None
    };

    // Load reagent test data
    let reagent_data = {
        let reagent_path = std::path::Path::new("data/reagents.json");
        if reagent_path.exists() {
            match ReagentData::load_from_file(reagent_path) {
                Ok(data) => {
                    info!(
                        substances = data.substance_count(),
                        "Reagent data loaded successfully"
                    );
                    Some(ReagentDataHolder::new(data))
                }
                Err(e) => {
                    warn!(error = %e, "Failed to load reagent data, reagent queries will be unavailable");
                    None
                }
            }
        } else {
            info!("No reagent data file found at data/reagents.json, reagent queries will be unavailable");
            None
        }
    };

    // Build GraphQL Schema
    let schema = create_schema(
        snapshot_holder.clone(),
        plebiscite_service,
        reagent_data,
        metrics.clone(),
    );

    // Fetch wiki redirects in background (non-blocking, updates aliases after completion)
    {
        let api_clone = api.clone();
        let snapshot_clone = snapshot_holder.clone();
        tokio::spawn(async move {
            fetch_and_cache_redirects(&api_clone, &snapshot_clone).await;
        });
    }

    // App state (currently unused but kept for future expansion)
    let _app_state = AppState {
        schema: schema.clone(),
        metrics: metrics.clone(),
        snapshot: snapshot_holder.clone(),
    };

    // Setup Router
    let app = Router::new()
        .route(
            "/",
            get(graphql::graphql_or_graphiql).post(graphql::graphql_post_handler),
        )
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(schema);

    // Start Server
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    info!(
        "Listening on http://{} (GraphQL: /, Metrics: /metrics, Health: /health)",
        addr
    );

    // Setup graceful shutdown
    let shutdown_signal = async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl-c");
        info!("Shutdown signal received, initiating graceful shutdown...");

        // Signal shutdown to revalidator
        let _ = shutdown_tx.send(true);

        // Wait a bit for revalidator to stop
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Persist final snapshot
        let snapshot = snapshot_holder.get().await;
        if let Err(e) = persist_to_disk(&snapshot, &config.cache.cache_path).await {
            error!("Failed to persist snapshot on shutdown: {}", e);
        } else {
            info!("Snapshot persisted to disk on shutdown");
        }
    };

    // Run server with graceful shutdown
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    // Wait for revalidator to finish
    revalidator_handle.abort();

    info!("Bifrost shutdown complete");
    Ok(())
}

/// Load substance aliases from the curated JSON file, then merge in
/// cached wiki redirect data if available.
fn load_substance_aliases() -> SubstanceAliases {
    // 1. Load curated aliases
    let alias_path = std::path::Path::new("data/substance_aliases.json");
    let mut aliases = if alias_path.exists() {
        match SubstanceAliases::load_from_file(alias_path) {
            Ok(a) => a,
            Err(e) => {
                warn!(error = %e, "Failed to load substance aliases, search will use names only");
                SubstanceAliases::empty()
            }
        }
    } else {
        info!("No substance aliases file found at data/substance_aliases.json, search will use names only");
        SubstanceAliases::empty()
    };

    // 2. Merge cached wiki redirects if available
    let redirect_cache_path = std::path::Path::new("data/wiki_redirects.json");
    if redirect_cache_path.exists() {
        match SubstanceAliases::load_redirect_cache(redirect_cache_path) {
            Ok(redirects) => {
                aliases.merge_redirects(&redirects);
            }
            Err(e) => {
                warn!(error = %e, "Failed to load cached wiki redirects");
            }
        }
    }

    aliases
}

/// Fetch wiki redirects in background and update the snapshot's alias data.
/// This runs after the server is up so it doesn't block startup.
async fn fetch_and_cache_redirects(
    api: &PsychonautApi,
    snapshot_holder: &SnapshotHolder,
) {
    info!("Fetching wiki redirects in background...");

    let redirect_cache_path = std::path::Path::new("data/wiki_redirects.json");

    match api.fetch_all_redirects().await {
        Ok(redirects) => {
            // Cache to disk for next boot
            if let Err(e) = SubstanceAliases::save_redirect_cache(&redirects, redirect_cache_path) {
                warn!(error = %e, "Failed to cache wiki redirects to disk");
            }

            // Merge into the live snapshot's alias data
            snapshot_holder
                .modify(|snapshot| {
                    snapshot.alias_data.merge_redirects(&redirects);
                    snapshot.rebuild_indexes();
                    info!(
                        aliases = snapshot.by_alias.len(),
                        "Snapshot alias index rebuilt with fresh wiki redirects"
                    );
                })
                .await;
        }
        Err(e) => {
            warn!(error = %e, "Failed to fetch wiki redirects, using cached/curated data only");
        }
    }
}

/// Initialize the snapshot from disk or perform cold start
async fn initialize_snapshot(
    config: &Config,
    api: &PsychonautApi,
    parser: &WikitextParser,
    metrics: &SharedMetrics,
) -> anyhow::Result<(SnapshotHolder, &'static str)> {
    let cache_path = &config.cache.cache_path;

    // Load substance aliases for search matching
    let alias_data = load_substance_aliases();

    // Try to load from disk
    match load_from_disk(cache_path).await {
        Ok(snapshot) => {
            info!(
                substances = snapshot.substances.len(),
                path = %cache_path.display(),
                "Loaded snapshot from disk cache"
            );

            // Re-build snapshot with alias data (disk cache doesn't store aliases)
            let snapshot = SubstanceSnapshot::build_with_aliases(
                snapshot.substances,
                alias_data,
            );

            let holder = SnapshotHolder::new(snapshot);
            metrics.update_cache_metrics(&*holder.get().await);

            // Check for new substances in background
            let holder_clone = holder.clone();
            let api_clone = api.clone();
            let parser_clone = parser.clone();
            let config_clone = config.clone();
            tokio::spawn(async move {
                if let Err(e) = check_for_new_substances(
                    &holder_clone,
                    &api_clone,
                    &parser_clone,
                    &config_clone,
                )
                .await
                {
                    warn!("Failed to check for new substances: {}", e);
                }
            });

            Ok((holder, "warm_start"))
        }
        Err(e) => {
            warn!(
                error = %e,
                path = %cache_path.display(),
                "No valid disk cache, performing cold start"
            );

            // Cold start: fetch full snapshot with retries
            info!("Fetching full substance snapshot from backend (this may take a while)...");

            let substances = cold_start_with_retry(api, parser, config).await?;

            info!(
                substances = substances.len(),
                "Full snapshot fetched, building indexes"
            );

            let snapshot = SubstanceSnapshot::build_with_aliases(substances, alias_data);

            // Persist to disk
            if let Err(e) = persist_to_disk(&snapshot, cache_path).await {
                error!("Failed to persist initial snapshot: {}", e);
            }

            let holder = SnapshotHolder::new(snapshot);
            metrics.update_cache_metrics(&*holder.get().await);

            Ok((holder, "cold_start"))
        }
    }
}

/// Cold start with retry logic
/// Retries the entire cold start process if it fails or returns too few substances
async fn cold_start_with_retry(
    api: &PsychonautApi,
    parser: &WikitextParser,
    config: &Config,
) -> anyhow::Result<Vec<crate::graphql::model::Substance>> {
    const MAX_RETRIES: u32 = 5;
    const MIN_SUBSTANCES: usize = 10; // Fail if we get fewer than this

    let mut delay = std::time::Duration::from_secs(2);

    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            warn!(
                attempt = attempt,
                max_retries = MAX_RETRIES,
                delay_secs = delay.as_secs(),
                "Retrying cold start after failure"
            );
            tokio::time::sleep(delay).await;
            delay = std::cmp::min(delay * 2, std::time::Duration::from_secs(60));
        }

        match fetch_full_snapshot(api, parser, config).await {
            Ok(substances) if substances.len() >= MIN_SUBSTANCES => {
                if attempt > 0 {
                    info!(
                        attempt = attempt,
                        substances = substances.len(),
                        "Cold start succeeded after retry"
                    );
                }
                return Ok(substances);
            }
            Ok(substances) => {
                error!(
                    attempt = attempt,
                    substances = substances.len(),
                    min_required = MIN_SUBSTANCES,
                    "Cold start returned too few substances, retrying"
                );
            }
            Err(e) => {
                error!(
                    attempt = attempt,
                    error = %e,
                    "Cold start failed"
                );
            }
        }
    }

    Err(anyhow::anyhow!(
        "Cold start failed after {} retries",
        MAX_RETRIES
    ))
}

/// Fetch full snapshot from backend (cold start)
async fn fetch_full_snapshot(
    api: &PsychonautApi,
    parser: &WikitextParser,
    config: &Config,
) -> anyhow::Result<Vec<crate::graphql::model::Substance>> {
    use futures::stream;

    // Get all substance names with retry logic
    let items = retry_with_backoff(
        || api.ask_query("[[Category:Psychoactive substance]]", 9999, 0),
        "fetch substance list",
        5,                                 // max retries
        std::time::Duration::from_secs(2), // initial delay
    )
    .await?;

    // Filter out non-substance pages (those with : like Category:, Template:, etc.)
    let raw_count = items.len();
    let names: Vec<String> = items
        .into_iter()
        .filter(|item| !item.fulltext.contains(':'))
        .map(|item| item.fulltext)
        .collect();

    let total_count = names.len();
    info!(
        raw = raw_count,
        filtered = total_count,
        skipped = raw_count - total_count,
        "Found substances, fetching details..."
    );
    let failed_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Fetch details for each substance (with limited concurrency)
    let substances: Vec<crate::graphql::model::Substance> = stream::iter(names)
        .map(|name| {
            let api = api.clone();
            let parser = parser.clone();
            let cdn_url = config.psychonaut.cdn_url.clone();
            let thumb_size = config.psychonaut.thumb_size;
            let failed_count = failed_count.clone();
            async move {
                let result =
                    fetch_single_substance(&name, &api, &parser, &cdn_url, thumb_size).await;
                (name, result, failed_count)
            }
        })
        .buffer_unordered(10) // Limit concurrency during cold start
        .filter_map(|(name, result, failed_count)| async move {
            match result {
                Ok(s) => Some(s),
                Err(e) => {
                    failed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    warn!(substance = %name, error = %e, "Failed to fetch substance");
                    None
                }
            }
        })
        .collect()
        .await;

    let failed = failed_count.load(std::sync::atomic::Ordering::Relaxed);
    if failed > 0 {
        error!(
            total = total_count,
            succeeded = substances.len(),
            failed = failed,
            "Cold start completed with failures"
        );
    } else {
        info!(
            total = total_count,
            succeeded = substances.len(),
            "Cold start completed successfully"
        );
    }

    Ok(substances)
}

/// Fetch a single substance with all its data
async fn fetch_single_substance(
    name: &str,
    api: &PsychonautApi,
    parser: &WikitextParser,
    cdn_url: &str,
    thumb_size: u32,
) -> anyhow::Result<crate::graphql::model::Substance> {
    use crate::graphql::model::{Effect, Substance, SubstanceImage};
    use md5::Digest;

    // Fetch core data
    let raw = api
        .browse_by_subject(name)
        .await
        .map_err(|e| anyhow::anyhow!("[{}] API error: {}", name, e))?;

    let mut parsed = parser
        .parse_smw(raw)
        .map_err(|e| anyhow::anyhow!("[{}] Parse error: {}", name, e))?;

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

    let mut substance: Substance = serde_json::from_value(parsed.clone()).map_err(|e| {
        // Log the problematic JSON for debugging
        let json_preview = serde_json::to_string_pretty(&parsed)
            .unwrap_or_else(|_| "failed to serialize".to_string());
        // Truncate to first 2000 chars to avoid log spam
        let truncated = if json_preview.len() > 2000 {
            format!("{}...(truncated)", &json_preview[..2000])
        } else {
            json_preview
        };
        anyhow::anyhow!(
            "[{}] Deserialization error: {}\nJSON:\n{}",
            name,
            e,
            truncated
        )
    })?;

    // Fetch effects (best effort)
    substance.effects_cache = match api.get_substance_effects(name, 100, 0).await {
        Ok(items) => Some(
            items
                .into_iter()
                .map(|item| Effect {
                    name: Some(item.fulltext),
                    url: Some(item.fullurl),
                })
                .collect(),
        ),
        Err(_) => Some(vec![]),
    };

    // Fetch summary (best effort)
    substance.summary_cache = match api.parse_text(name).await {
        Ok(raw) => {
            let re = regex::Regex::new(r"<[^>]*>").unwrap();
            let no_tags = re.replace_all(&raw, "").to_string();
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
                None
            } else {
                Some(cleaned)
            }
        }
        Err(_) => None,
    };

    // Fetch images (best effort)
    substance.images_cache = match api.parse_images(name).await {
        Ok(images) if !images.is_empty() => {
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
                            cdn_url, filename, thumb_size
                        )),
                        image: Some(format!("{}w/images/{}/{}/{}", cdn_url, a, ab, filename)),
                    }
                })
                .collect();
            Some(mapped)
        }
        _ => None,
    };

    Ok(substance)
}

/// Check for new substances after warm start
async fn check_for_new_substances(
    holder: &SnapshotHolder,
    api: &PsychonautApi,
    parser: &WikitextParser,
    config: &Config,
) -> anyhow::Result<()> {
    use std::collections::HashSet;

    // Fetch all names from backend
    let items = api
        .ask_query("[[Category:Psychoactive substance]]", 9999, 0)
        .await?;

    let backend_names: HashSet<String> = items
        .into_iter()
        .filter(|item| !item.fulltext.contains(':'))
        .map(|item| item.fulltext)
        .collect();

    let current = holder.get().await;
    let cached_names: HashSet<String> = current
        .substances
        .iter()
        .filter_map(|s| s.name.clone())
        .collect();

    let new_names: Vec<String> = backend_names.difference(&cached_names).cloned().collect();

    if new_names.is_empty() {
        info!("No new substances detected");
        return Ok(());
    }

    info!(
        count = new_names.len(),
        names = ?new_names,
        "New substances detected, fetching details"
    );

    // Fetch new substances
    let mut new_substances = Vec::new();
    let mut failed_names = Vec::new();
    for name in &new_names {
        match fetch_single_substance(
            name,
            api,
            parser,
            &config.psychonaut.cdn_url,
            config.psychonaut.thumb_size,
        )
        .await
        {
            Ok(s) => new_substances.push(s),
            Err(e) => {
                warn!(substance = %name, error = %e, "Failed to fetch new substance");
                failed_names.push(name.clone());
            }
        }
    }

    if !failed_names.is_empty() {
        error!(
            total = new_names.len(),
            succeeded = new_substances.len(),
            failed = failed_names.len(),
            failed_names = ?failed_names,
            "Some new substances failed to fetch"
        );
    }

    if new_substances.is_empty() {
        return Ok(());
    }

    // Merge into snapshot
    holder
        .modify(|snapshot| {
            for substance in new_substances {
                snapshot.add_substance(substance);
            }
        })
        .await;

    // Persist
    let updated = holder.get().await;
    persist_to_disk(&updated, &config.cache.cache_path).await?;

    info!("Snapshot updated with new substances");
    Ok(())
}

/// Metrics endpoint handler
async fn metrics_handler(State(schema): State<graphql::BifrostSchema>) -> impl IntoResponse {
    let metrics = schema
        .data::<SharedMetrics>()
        .expect("Metrics not found in schema");

    let output = metrics.render();

    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        output,
    )
}

/// Health check endpoint
async fn health_handler(State(schema): State<graphql::BifrostSchema>) -> impl IntoResponse {
    let snapshot = schema
        .data::<SnapshotHolder>()
        .expect("Snapshot not found in schema");

    let current = snapshot.get().await;
    let substance_count = current.substances.len();
    let age_secs = current.meta.created_at.elapsed().as_secs();

    let status = if substance_count > 0 {
        "healthy"
    } else {
        "degraded"
    };

    let body = serde_json::json!({
        "status": status,
        "substances": substance_count,
        "snapshot_age_seconds": age_secs,
        "version": env!("CARGO_PKG_VERSION"),
    });

    (
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
}

/// Retry an async operation with exponential backoff
async fn retry_with_backoff<T, E, F, Fut>(
    operation: F,
    operation_name: &str,
    max_retries: u32,
    initial_delay: std::time::Duration,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut delay = initial_delay;
    let mut last_error: Option<E> = None;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            warn!(
                attempt = attempt,
                max_retries = max_retries,
                delay_secs = delay.as_secs(),
                operation = operation_name,
                "Retrying after failure"
            );
            tokio::time::sleep(delay).await;
            delay = std::cmp::min(delay * 2, std::time::Duration::from_secs(60));
            // Cap at 60s
        }

        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    info!(
                        attempt = attempt,
                        operation = operation_name,
                        "Operation succeeded after retry"
                    );
                }
                return Ok(result);
            }
            Err(e) => {
                error!(
                    attempt = attempt,
                    max_retries = max_retries,
                    error = %e,
                    operation = operation_name,
                    "Operation failed"
                );
                last_error = Some(e);
            }
        }
    }

    // All retries exhausted
    Err(last_error.expect("Should have error after failed retries"))
}

fn init_logging(level: &str, json: bool, debug_requests: bool) -> anyhow::Result<()> {
    let level = level.parse::<Level>().unwrap_or(Level::INFO);

    // Build filter: set bifrost to requested level, and optionally enable request debugging
    let filter = if debug_requests {
        EnvFilter::new(format!(
            "bifrost={},bifrost::services::psychonaut::api=debug,tower_http=debug,hyper=warn",
            level
        ))
    } else {
        EnvFilter::new(format!("bifrost={},tower_http=info,hyper=warn", level))
    };

    if json {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(
                fmt::layer()
                    .with_target(true)
                    .with_thread_ids(false)
                    .with_file(false)
                    .with_line_number(false),
            )
            .init();
    }

    Ok(())
}
