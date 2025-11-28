mod cache;
mod config;
mod error;
mod graphql;
mod services;
mod utils;

use crate::config::Config;
use crate::graphql::create_schema;
use crate::services::plebiscite::PlebisciteService;
use crate::services::psychonaut::PsychonautService;
use crate::utils::ascii::print_startup_banner;
use axum::{routing::get, Router};
use clap::Parser;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info, Level};
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file first (before parsing args, so env vars are available)
    dotenvy::dotenv().ok();

    let args = Args::parse();

    // Initialize logging
    init_logging(&args.log_level, args.json_logs, args.debug_requests)?;

    // Print banner after logging is initialized
    print_startup_banner();

    info!("Initializing Bifrost v{}", env!("CARGO_PKG_VERSION"));

    // Load config (CLI port overrides env var)
    let mut config = Config::from_env()?;
    if let Some(port) = args.port {
        config.server.port = port;
    }

    // Store debug_requests flag in config for services to use
    config.debug_requests = args.debug_requests;

    if args.debug_requests {
        info!("Backend request debugging is ENABLED");
    }

    // Initialize Services
    let psychonaut_service = Arc::new(PsychonautService::new(&config));

    let plebiscite_service = if config.features.plebiscite_enabled {
        info!("Feature 'Plebiscite' is ENABLED. Connecting to MongoDB...");
        match PlebisciteService::new(&config).await {
            Ok(service) => Some(Arc::new(service)),
            Err(e) => {
                error!("Failed to initialize Plebiscite service: {}", e);
                return Err(e.into());
            }
        }
    } else {
        info!("Feature 'Plebiscite' is DISABLED.");
        None
    };

    // Build GraphQL Schema
    let schema = create_schema(psychonaut_service, plebiscite_service);

    // Setup Router
    let app = Router::new()
        .route("/", get(graphql::graphiql).post(graphql::graphql_handler))
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
    info!("Bifrost Online: http://{}", addr);

    // Handle shutdown signal
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            error!("Failed to listen for ctrl-c: {}", e);
            return;
        }
        info!("Received shutdown signal, exiting...");
        std::process::exit(0);
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
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
