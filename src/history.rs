//! Price history buffer: keeps the most recent 2000 points in memory;
//! older points are evicted to a JSON Lines archive file.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tracing::warn;

/// Max points kept in memory. User requests at most 2k at a time.
const MAX_IN_MEMORY: usize = 2000;
/// When over MAX_IN_MEMORY, evict this many to the archive (so we go from 3000 -> 2000).
const EVICT_BATCH: usize = 1000;

/// A single price data point (16 bytes: f64 + i64).
/// Storage: ~56 KB/hour, ~1.35 MB/24h, ~9.2 MB/7 days at ~1 point/sec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub price: f64,
    pub timestamp: i64,
}

/// Default archive path (JSON Lines: one JSON array per line).
const DEFAULT_ARCHIVE_PATH: &str = "price_history_archive.jsonl";

/// Thread-safe price history buffer. Keeps the most recent MAX_IN_MEMORY points;
/// older points are appended to a JSON Lines file.
#[derive(Clone)]
pub struct PriceHistory {
    inner: Arc<RwLock<VecDeque<PricePoint>>>,
    archive_path: PathBuf,
}

impl PriceHistory {
    pub fn new() -> Self {
        Self::with_archive_path(DEFAULT_ARCHIVE_PATH)
    }

    pub fn with_archive_path(path: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(VecDeque::new())),
            archive_path: path.into(),
        }
    }

    /// Add a new price point. When in-memory count exceeds MAX_IN_MEMORY,
    /// the oldest EVICT_BATCH points are appended to the archive file.
    pub async fn push(&self, price: f64, timestamp: i64) {
        let mut buffer = self.inner.write().await;
        buffer.push_back(PricePoint { price, timestamp });

        while buffer.len() > MAX_IN_MEMORY {
            let to_evict = (buffer.len() - MAX_IN_MEMORY).min(EVICT_BATCH);
            let batch: Vec<PricePoint> = buffer.drain(..to_evict).collect();
            drop(buffer);

            if let Err(e) = append_batch_to_archive(&self.archive_path, &batch).await {
                warn!("Failed to append {} points to archive {:?}: {}", batch.len(), self.archive_path, e);
            }

            buffer = self.inner.write().await;
        }
    }

    /// Get historical data from the last N seconds.
    /// Returns (data, limited) where limited=true if we don't have full history.
    pub async fn get_history(&self, seconds: i64) -> (Vec<PricePoint>, bool) {
        let buffer = self.inner.read().await;
        
        if buffer.is_empty() {
            return (Vec::new(), true);
        }

        // Convert seconds to milliseconds for timestamp comparison
        let seconds_ms = seconds * 1000;
        let latest_timestamp = buffer.back().unwrap().timestamp;
        let requested_cutoff = latest_timestamp - seconds_ms;

        let available_start = buffer.front().unwrap().timestamp;
        let limited = requested_cutoff < available_start;

        let data: Vec<PricePoint> = buffer
            .iter()
            .filter(|p| p.timestamp >= requested_cutoff)
            .cloned()
            .collect();

        (data, limited)
    }

    /// Get the latest price point.
    pub async fn latest(&self) -> Option<PricePoint> {
        let buffer = self.inner.read().await;
        buffer.back().cloned()
    }

    /// Get current buffer size (for monitoring).
    #[allow(dead_code)]
    pub async fn len(&self) -> usize {
        let buffer = self.inner.read().await;
        buffer.len()
    }
}

/// Append a batch of points as one JSON array line to the archive file.
async fn append_batch_to_archive(path: &std::path::Path, batch: &[PricePoint]) -> Result<(), std::io::Error> {
    let line = serde_json::to_string(batch).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;
    Ok(())
}

impl Default for PriceHistory {
    fn default() -> Self {
        Self::new()
    }
}
