//! Upstream client that connects to Polymarket RTDS and feeds price history.

use crate::history::PriceHistory;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::interval;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};

const RTDS_URL: &str = "wss://ws-live-data.polymarket.com";
const PING_INTERVAL_SECS: u64 = 5;

/// Subscription for Chainlink prices.
#[derive(Debug, Serialize)]
struct Subscription {
    topic: String,
    #[serde(rename = "type")]
    sub_type: String,
    filters: String,
}

/// Subscribe action message.
#[derive(Debug, Serialize)]
struct SubscribeAction {
    action: String,
    subscriptions: Vec<Subscription>,
}

/// Chainlink price payload.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PricePayload {
    symbol: String,
    timestamp: i64,
    value: f64,
}

/// RTDS message envelope.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RtdsMessage {
    topic: String,
    #[serde(rename = "type")]
    msg_type: String,
    timestamp: Option<i64>,
    payload: Option<PricePayload>,
}

/// Start upstream Chainlink client, feeding price history.
pub async fn run_upstream_client(history: PriceHistory) -> Result<(), String> {
    info!("Connecting to Polymarket RTDS: {}", RTDS_URL);

    let (ws_stream, _) = connect_async(RTDS_URL).await
        .map_err(|e| format!("Connect failed: {}", e))?;
    info!("Connected to RTDS");

    let (mut write, mut read) = ws_stream.split();

    // Subscribe to BTC/USD
    let subscribe = SubscribeAction {
        action: "subscribe".to_string(),
        subscriptions: vec![Subscription {
            topic: "crypto_prices_chainlink".to_string(),
            sub_type: "*".to_string(),
            filters: r#"{"symbol":"btc/usd"}"#.to_string(),
        }],
    };
    let msg = serde_json::to_string(&subscribe)
        .map_err(|e| format!("JSON error: {}", e))?;
    write.send(Message::Text(msg.into())).await
        .map_err(|e| format!("Send error: {}", e))?;
    info!("Subscribed to crypto_prices_chainlink (btc/usd)");

    // Ping interval
    let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            Some(Ok(ws_msg)) = read.next() => {
                match ws_msg {
                    Message::Text(text) => {
                        if let Ok(rtds) = serde_json::from_str::<RtdsMessage>(&text) {
                            if rtds.topic == "crypto_prices_chainlink" {
                                if let Some(p) = rtds.payload {
                                    info!("BTC/USD: {} (ts: {})", p.value, p.timestamp);
                                    history.push(p.value, p.timestamp).await;
                                }
                            }
                        }
                    }
                    Message::Ping(data) => {
                        if let Err(e) = write.send(Message::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                        }
                    }
                    Message::Close(_) => {
                        warn!("RTDS closed connection");
                        break;
                    }
                    _ => {}
                }
            }
            _ = ping_interval.tick() => {
                if let Err(e) = write.send(Message::Ping(vec![].into())).await {
                    error!("Failed to send ping: {}", e);
                    break;
                }
            }
        }
    }

    Err("RTDS connection closed".to_string())
}
