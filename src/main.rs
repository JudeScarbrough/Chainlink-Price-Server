//! Production WebSocket server streaming Chainlink BTC prices with historical data.
//!
//! Architecture:
//! - Upstream client: connects to Polymarket RTDS, receives BTC prices
//! - Price history: circular buffer storing last 1 hour of prices
//! - WebSocket server: serves clients with historical + live price updates

mod history;
mod server;
mod upstream;

use history::PriceHistory;
use tokio::sync::broadcast;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

/// Bind address: 0.0.0.0 for cloud (AWS), 127.0.0.1 for local only.
const BIND_ADDR: &str = "0.0.0.0:8080";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting Chainlink BTC Price WebSocket Server");

    // Shared price history (last 1 hour)
    let history = PriceHistory::new();

    // Broadcast channel for live price updates
    let (price_tx, price_rx) = broadcast::channel(1024);

    // Clone for upstream task
    let upstream_history = history.clone();
    let upstream_tx = price_tx.clone();

    // Start upstream Chainlink client
    let upstream_task = tokio::spawn(async move {
        loop {
            match upstream::run_upstream_client(upstream_history.clone()).await {
                Ok(_) => {
                    info!("Upstream client exited normally");
                    break;
                }
                Err(e) => {
                    error!("Upstream client error: {}, reconnecting in 5s...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    });

    // Spawn broadcast forwarder
    let broadcast_history = history.clone();
    tokio::spawn(async move {
        let mut last_timestamp = 0i64;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            if let Some(latest) = broadcast_history.latest().await {
                if latest.timestamp > last_timestamp {
                    last_timestamp = latest.timestamp;
                    let _ = upstream_tx.send(latest);
                }
            }
        }
    });

    // Start WebSocket server
    let server_task = tokio::spawn(async move {
        if let Err(e) = server::run_server(BIND_ADDR, history, price_rx).await {
            error!("Server error: {}", e);
        }
    });

    // Wait for ctrl-c
    tokio::select! {
        _ = upstream_task => {
            error!("Upstream task terminated");
        }
        _ = server_task => {
            error!("Server task terminated");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
    }

    info!("Shutting down");
    Ok(())
}
