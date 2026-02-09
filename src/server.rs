//! WebSocket server for client connections.

//! WebSocket server for client connections.

use crate::history::{PriceHistory, PricePoint};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{error, info, warn};

/// Client subscription request.
#[derive(Debug, Deserialize)]
struct ClientSubscription {
    symbol: String,
    history_seconds: Option<i64>,
}

/// Initial response with historical data.
#[derive(Debug, Serialize)]
struct InitialResponse {
    #[serde(rename = "type")]
    msg_type: String,
    symbol: String,
    history: Vec<PricePoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    info: Option<String>,
}

/// Price update message.
#[derive(Debug, Clone, Serialize)]
struct PriceUpdate {
    #[serde(rename = "type")]
    msg_type: String,
    symbol: String,
    price: f64,
    timestamp: i64,
}

/// Start the WebSocket server.
pub async fn run_server(
    bind_addr: &str,
    history: PriceHistory,
    price_rx: broadcast::Receiver<PricePoint>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(bind_addr).await?;
    info!("WebSocket server listening on {}", bind_addr);

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let history = history.clone();
                let rx = price_rx.resubscribe();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, addr, history, rx).await {
                        error!("Client {} error: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}

async fn handle_client(
    stream: TcpStream,
    addr: SocketAddr,
    history: PriceHistory,
    mut price_rx: broadcast::Receiver<PricePoint>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("New client connected: {}", addr);

    let ws_stream = accept_async(stream).await?;
    let (mut write, mut read) = ws_stream.split();

    // Wait for subscription message
    let subscription = match read.next().await {
        Some(Ok(Message::Text(text))) => match serde_json::from_str::<ClientSubscription>(&text) {
            Ok(sub) => sub,
            Err(e) => {
                warn!("Client {} sent invalid subscription: {}", addr, e);
                return Ok(());
            }
        },
        _ => {
            warn!("Client {} disconnected before subscribing", addr);
            return Ok(());
        }
    };

    // Only support BTC for now
    if subscription.symbol.to_lowercase() != "btc" && subscription.symbol.to_lowercase() != "btc/usd" {
        let error_msg = serde_json::json!({
            "type": "error",
            "message": "Only 'btc' or 'btc/usd' is supported"
        });
        write.send(Message::Text(error_msg.to_string().into())).await?;
        return Ok(());
    }

    // Get historical data
    let history_seconds = subscription.history_seconds.unwrap_or(0).max(0);
    let (historical_data, limited) = history.get_history(history_seconds).await;

    let info = if limited && history_seconds > 0 {
        Some(format!(
            "Limited history: requested {} seconds, but full history may not be available yet",
            history_seconds
        ))
    } else {
        None
    };

    // Send initial response with history
    let initial = InitialResponse {
        msg_type: "initial".to_string(),
        symbol: "btc/usd".to_string(),
        history: historical_data,
        info,
    };
    let msg = serde_json::to_string(&initial)?;
    write.send(Message::Text(msg.into())).await?;
    info!(
        "Client {} subscribed to BTC with {} seconds history",
        addr, history_seconds
    );

    // Stream live updates
    loop {
        tokio::select! {
            Some(msg_result) = read.next() => {
                match msg_result {
                    Ok(Message::Ping(data)) => {
                        write.send(Message::Pong(data)).await?;
                    }
                    Ok(Message::Close(_)) => {
                        info!("Client {} closed connection", addr);
                        break;
                    }
                    Err(e) => {
                        warn!("Client {} error: {}", addr, e);
                        break;
                    }
                    _ => {}
                }
            }
            Ok(price_point) = price_rx.recv() => {
                let update = PriceUpdate {
                    msg_type: "update".to_string(),
                    symbol: "btc/usd".to_string(),
                    price: price_point.price,
                    timestamp: price_point.timestamp,
                };
                let msg = serde_json::to_string(&update)?;
                if let Err(e) = write.send(Message::Text(msg.into())).await {
                    warn!("Failed to send update to client {}: {}", addr, e);
                    break;
                }
            }
        }
    }

    info!("Client {} disconnected", addr);
    Ok(())
}
