//! WebSocket Metrics Bridge for MemLane.
//!
//! Collects operation timing events from the TCP server and broadcasts
//! aggregated metrics every second to all connected WebSocket clients
//! (the React dashboard).
//!
//! Architecture:
//!   - TCP server sends MetricEvent via an mpsc channel after each command
//!   - ws_bridge aggregates events into 1-second windows
//!   - Every second, broadcasts a JSON MetricsSnapshot to all WS clients
//!   - Dashboard connects to ws://127.0.0.1:9001 and renders live charts
//!
//! JSON payload sent to dashboard every second:
//! {
//!   "ops_per_sec": 1240000,
//!   "p50_us": 0,
//!   "p99_us": 1,
//!   "p999_us": 3,
//!   "used_slots": 12400,
//!   "total_slots": 65536,
//!   "fill_pct": 18.9,
//!   "breakdown": {
//!     "GET": 980000,
//!     "SET": 260000
//!   }
//! }

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio::time;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

// ── Channel types ─────────────────────────────────────────────────────────────

/// A single operation event emitted by the TCP server after each command.
#[derive(Debug, Clone)]
pub struct MetricEvent {
    /// Command name e.g. "GET", "SET", "DEL"
    pub op: String,
    /// How long the operation took in microseconds
    pub latency_us: u64,
}

/// Sender half of the metrics channel (TCP server → aggregator).
/// Uses an async mpsc with a generous buffer so fast paths never block.
pub type MetricsSender = mpsc::Sender<MetricEvent>;

/// Receiver half of the metrics channel.
pub type MetricsReceiver = mpsc::Receiver<MetricEvent>;

/// Create a bounded metrics channel. Buffer = 1M events (covers ~1s at 1M ops/sec).
pub fn metrics_channel() -> (MetricsSender, MetricsReceiver) {
    mpsc::channel(1_000_000)
}

// ── Aggregated snapshot broadcast to dashboard ────────────────────────────────

/// Aggregated metrics snapshot, serialised to JSON and sent to the dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    /// Total operations in the last second
    pub ops_per_sec: u64,
    /// Median latency in microseconds
    pub p50_us: u64,
    /// 99th percentile latency in microseconds
    pub p99_us: u64,
    /// 99.9th percentile latency in microseconds
    pub p999_us: u64,
    /// Currently occupied slots
    pub used_slots: u64,
    /// Total slots in arena
    pub total_slots: u64,
    /// Fill percentage 0.0–100.0
    pub fill_pct: f64,
    /// Per-command breakdown: { "GET": 800000, "SET": 200000 }
    pub breakdown: HashMap<String, u64>,
}

// ── Aggregator ────────────────────────────────────────────────────────────────

/// Accumulates MetricEvents over a 1-second window.
struct Aggregator {
    /// All latency samples in this window (microseconds)
    latencies: Vec<u64>,
    /// Per-command event counts
    breakdown: HashMap<String, u64>,
    /// Window start time
    window_start: Instant,
}

impl Aggregator {
    fn new() -> Self {
        Self {
            latencies: Vec::with_capacity(1_000_000),
            breakdown: HashMap::new(),
            window_start: Instant::now(),
        }
    }

    fn record(&mut self, event: MetricEvent) {
        self.latencies.push(event.latency_us);
        *self.breakdown.entry(event.op).or_insert(0) += 1;
    }

    /// Drain the window and produce a MetricsSnapshot.
    fn drain(
        &mut self,
        used_slots: u64,
        total_slots: u64,
    ) -> MetricsSnapshot {
        let ops_per_sec = self.latencies.len() as u64;

        // Sort latencies for percentile calculation
        self.latencies.sort_unstable();

        let p50_us = percentile(&self.latencies, 50);
        let p99_us = percentile(&self.latencies, 99);
        let p999_us = percentile(&self.latencies, 99_9); // using 999 as proxy for 99.9

        let fill_pct = if total_slots > 0 {
            (used_slots as f64 / total_slots as f64) * 100.0
        } else {
            0.0
        };

        let snapshot = MetricsSnapshot {
            ops_per_sec,
            p50_us,
            p99_us,
            p999_us,
            used_slots,
            total_slots,
            fill_pct,
            breakdown: self.breakdown.clone(),
        };

        // Reset for next window
        self.latencies.clear();
        self.breakdown.clear();
        self.window_start = Instant::now();

        snapshot
    }
}

/// Compute the Nth percentile of a sorted slice. N is 0–100 (or 999 for 99.9th).
fn percentile(sorted: &[u64], n: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    // Handle the 999 → 99.9th percentile special case
    let pct = if n == 999 { 99.9f64 } else { n as f64 };
    let idx = ((pct / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ── WebSocket broadcast server ────────────────────────────────────────────────

/// Default WebSocket server address for the dashboard
pub const WS_ADDR: &str = "127.0.0.1:9001";

/// Run the WebSocket metrics server.
///
/// Receives MetricEvents from the TCP server via `metrics_rx`,
/// aggregates them into 1-second windows, and broadcasts JSON snapshots
/// to all connected WebSocket clients.
///
/// Also accepts an optional `client` reference to read used_slots/total_slots.
pub async fn run_ws_server(
    mut metrics_rx: MetricsReceiver,
    used_slots_fn: Arc<dyn Fn() -> (u64, u64) + Send + Sync>,
) {
    let listener = TcpListener::bind(WS_ADDR)
        .await
        .expect("Failed to bind WebSocket server");

    tracing::info!("WebSocket metrics server on ws://{}", WS_ADDR);

    // Broadcast channel: aggregator → all connected WS clients
    let (broadcast_tx, _) = broadcast::channel::<String>(64);
    let broadcast_tx = Arc::new(broadcast_tx);

    // Spawn aggregator task
    let agg_tx = Arc::clone(&broadcast_tx);
    let slots_fn = Arc::clone(&used_slots_fn);
    tokio::spawn(async move {
        let mut aggregator = Aggregator::new();
        let mut interval = time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                // Drain all pending events (non-blocking)
                event = metrics_rx.recv() => {
                    match event {
                        Some(e) => aggregator.record(e),
                        None => break, // Channel closed
                    }
                }

                // Every second, broadcast a snapshot
                _ = interval.tick() => {
                    let (used, total) = slots_fn();
                    let snapshot = aggregator.drain(used, total);
                    if let Ok(json) = serde_json::to_string(&snapshot) {
                        // Ignore send errors (no subscribers connected)
                        let _ = agg_tx.send(json);
                    }
                }
            }
        }
    });

    // Accept WebSocket connections
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                tracing::debug!("Dashboard connected from {}", peer);
                let rx = broadcast_tx.subscribe();
                tokio::spawn(handle_ws_client(stream, rx));
            }
            Err(e) => {
                tracing::error!("WS accept error: {}", e);
            }
        }
    }
}

/// Handle one WebSocket client connection.
/// Forwards broadcast JSON messages to the client until disconnected.
async fn handle_ws_client(
    stream: TcpStream,
    mut rx: broadcast::Receiver<String>,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::debug!("WS handshake failed: {}", e);
            return;
        }
    };

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Send a welcome message so the dashboard knows it's connected
    let welcome = serde_json::json!({
        "type": "connected",
        "server": "MemLane",
        "version": "0.1.0"
    });
    let _ = ws_tx
        .send(Message::Text(welcome.to_string()))
        .await;

    loop {
        tokio::select! {
            // Forward metric snapshots to dashboard
            msg = rx.recv() => {
                match msg {
                    Ok(json) => {
                        if ws_tx.send(Message::Text(json)).await.is_err() {
                            break; // Client disconnected
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Dashboard lagged behind {} snapshots", n);
                    }
                    Err(_) => break,
                }
            }

            // Handle incoming messages from dashboard (e.g. ping/pong)
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(data))) => {
                        let _ = ws_tx.send(Message::Pong(data)).await;
                    }
                    _ => {} // Ignore other message types
                }
            }
        }
    }

    tracing::debug!("Dashboard client disconnected");
}

// ── Main entry point helper ───────────────────────────────────────────────────

/// Convenience function: start both TCP server and WS bridge together.
///
/// Call this from main.rs or an integration test.
///
/// ```rust
/// use std::sync::Arc;
/// use memlane::client::MemLaneClient;
/// use memlane::server::ws_bridge;
/// use memlane::server::DEFAULT_ADDR;
///
/// #[tokio::main]
/// async fn main() {
///     let client = Arc::new(MemLaneClient::create().unwrap());
///     ws_bridge::start_all(client, DEFAULT_ADDR).await.unwrap();
/// }
/// ```
pub async fn start_all(
    client: Arc<MemLaneClient>,
    tcp_addr: &str,
) -> std::io::Result<()> {
    // Initialise tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let (metrics_tx, metrics_rx) = metrics_channel();

    // Clone client for slot count reporting
    let client_for_slots = Arc::clone(&client);
    let slots_fn: Arc<dyn Fn() -> (u64, u64) + Send + Sync> =
        Arc::new(move || {
            (
                client_for_slots.used_count(),
                client_for_slots.capacity() as u64,
            )
        });

    // Spawn WebSocket bridge
    tokio::spawn(run_ws_server(metrics_rx, slots_fn));

    tracing::info!("MemLane started.");
    tracing::info!("  TCP (RESP2):  {}", tcp_addr);
    tracing::info!("  WebSocket:    ws://{}", WS_ADDR);
    tracing::info!("  Connect:      redis-cli -p 6399");
    tracing::info!("  Dashboard:    http://localhost:5173");

    // Run TCP server (blocks)
    crate::server::run_server(client, tcp_addr, metrics_tx).await
}