//! High-performance gRPC server for KV lookups.
//!
//! Provides O(1) key-value lookups with p99 < 5ms latency at 10k+ RPS.
//!
//! # Features
//!
//! - Memory-mapped storage for zero-copy lookups
//! - MPHF indexing for O(1) slot lookup
//! - Prometheus metrics on separate port
//! - Hot reload support via ArcSwap
//!
//! # Usage
//!
//! ```bash
//! kv-server --snapshot /data/snapshot.bin --port 9090 --metrics-port 9091
//! ```

use anyhow::{Context, Result};
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

mod metrics;
mod server;

pub mod proto {
    tonic::include_proto!("kv");
}

use metrics::Metrics;
use server::KvServiceImpl;

#[derive(Parser)]
#[command(name = "kv-server")]
#[command(about = "High-performance gRPC server for KV lookups")]
#[command(version)]
struct Cli {
    /// Path to the snapshot file
    #[arg(short, long)]
    snapshot: PathBuf,

    /// gRPC server port
    #[arg(short, long, default_value = "9090")]
    port: u16,

    /// Metrics server port (Prometheus)
    #[arg(short, long, default_value = "9091")]
    metrics_port: u16,

    /// Bind address
    #[arg(short, long, default_value = "0.0.0.0")]
    bind: String,

    /// Preload snapshot into RAM before starting
    #[arg(long, default_value = "true")]
    preload: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    info!(
        snapshot = %cli.snapshot.display(),
        port = cli.port,
        metrics_port = cli.metrics_port,
        "Starting KV server"
    );

    // Load snapshot
    let store = kv_store::KvStore::open(&cli.snapshot)
        .context("Failed to open snapshot")?;

    info!(
        record_count = store.len(),
        "Snapshot loaded"
    );

    // Optionally preload into RAM
    if cli.preload {
        info!("Preloading snapshot into RAM");
        store.preload()?;
    }

    // Initialize metrics
    let metrics = Arc::new(Metrics::new());

    // Create service
    let service = KvServiceImpl::new(store, metrics.clone());

    // Build gRPC server
    let grpc_addr: SocketAddr = format!("{}:{}", cli.bind, cli.port)
        .parse()
        .context("Invalid gRPC address")?;

    let metrics_addr: SocketAddr = format!("{}:{}", cli.bind, cli.metrics_port)
        .parse()
        .context("Invalid metrics address")?;

    // Start metrics server in background
    let metrics_clone = metrics.clone();
    tokio::spawn(async move {
        if let Err(e) = run_metrics_server(metrics_addr, metrics_clone).await {
            warn!(error = %e, "Metrics server error");
        }
    });

    info!(addr = %grpc_addr, "Starting gRPC server");

    // Start gRPC server
    tonic::transport::Server::builder()
        .add_service(proto::kv_service_server::KvServiceServer::new(service))
        .serve(grpc_addr)
        .await
        .context("gRPC server error")?;

    Ok(())
}

async fn run_metrics_server(addr: SocketAddr, metrics: Arc<Metrics>) -> Result<()> {
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Request, Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(addr).await?;
    info!(addr = %addr, "Metrics server listening");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let metrics = metrics.clone();

        tokio::spawn(async move {
            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                let metrics = metrics.clone();
                async move {
                    match req.uri().path() {
                        "/metrics" => {
                            let body = metrics.encode();
                            Ok::<_, hyper::Error>(
                                Response::builder()
                                    .status(StatusCode::OK)
                                    .header("Content-Type", "text/plain; version=0.0.4")
                                    .body(Full::new(Bytes::from(body)))
                                    .unwrap(),
                            )
                        }
                        "/health" => Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(Full::new(Bytes::from("OK")))
                            .unwrap()),
                        _ => Ok(Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(Full::new(Bytes::from("Not Found")))
                            .unwrap()),
                    }
                }
            });

            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                warn!(error = %err, "Metrics connection error");
            }
        });
    }
}
