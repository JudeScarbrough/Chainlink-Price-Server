//! Example WebSocket client for Chainlink BTC price server.
//!
//! Usage: cargo run --example client

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const SERVER_URL: &str = "ws://127.0.0.1:8080";

#[derive(Debug, Serialize)]
struct Subscription {
    symbol: String,
    history_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct PricePoint {
    price: f64,
    timestamp: i64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "initial")]
    Initial {
        symbol: String,
        history: Vec<PricePoint>,
        info: Option<String>,
    },
    #[serde(rename = "update")]
    Update {
        symbol: String,
        price: f64,
        timestamp: i64,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to Chainlink BTC Price Server: {}", SERVER_URL);

    let (ws_stream, _) = connect_async(SERVER_URL).await?;
    println!("Connected!");

    let (mut write, mut read) = ws_stream.split();

    // Subscribe to BTC with 1000 seconds of history
    let subscription = Subscription {
        symbol: "btc".to_string(),
        history_seconds: Some(1000),
    };
    let msg = serde_json::to_string(&subscription)?;
    
    use futures_util::SinkExt;
    write.send(Message::Text(msg.into())).await?;
    println!("Subscribed to BTC with 1000 seconds history\n");

    // Receive messages
    while let Some(msg) = read.next().await {
        match msg? {
            Message::Text(text) => {
                match serde_json::from_str::<ServerMessage>(&text) {
                    Ok(ServerMessage::Initial { symbol, history, info }) => {
                        println!("=== INITIAL RESPONSE ===");
                        println!("Symbol: {}", symbol);
                        println!("Historical points: {}", history.len());
                        if let Some(info_msg) = info {
                            println!("Info: {}", info_msg);
                        }
                        if !history.is_empty() {
                            println!("\nAll historical prices:");
                            for (i, point) in history.iter().enumerate() {
                                println!(
                                    "  {}. ${:.2} | ts: {}",
                                    i + 1,
                                    point.price,
                                    point.timestamp
                                );
                            }
                        }
                        println!("\n=== STREAMING LIVE UPDATES ===\n");
                    }
                    Ok(ServerMessage::Update { price, timestamp, .. }) => {
                        println!("BTC/USD: ${:.2} | ts: {}", price, timestamp);
                    }
                    Ok(ServerMessage::Error { message }) => {
                        eprintln!("Server error: {}", message);
                        break;
                    }
                    Err(e) => {
                        eprintln!("Failed to parse message: {}", e);
                    }
                }
            }
            Message::Close(_) => {
                println!("Server closed connection");
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
