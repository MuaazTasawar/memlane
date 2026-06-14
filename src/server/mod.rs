//! TCP Fallback Server for MemLane.
//!
//! Listens on 127.0.0.1:6399 (one port above Redis's 6379 by default)
//! and speaks the Redis RESP2 wire protocol. This means any Redis client
//! library — redis-cli, redis-py, ioredis, Jedis — can connect and use
//! MemLane with ZERO code changes.
//!
//! Architecture:
//!   - One Tokio task per connected client (async, non-blocking)
//!   - Shared Arc<MemLaneClient> across all tasks
//!   - RESP parser runs on each read, command dispatcher handles the result
//!   - Metrics (ops/sec, latency) emitted to the ws_bridge module
//!
//! Supported commands:
//!   PING, GET, SET [EX seconds], DEL, EXISTS, MGET, MSET,
//!   INCR, DECR, FLUSH, INFO, COMMAND, TTL

pub mod resp;
pub mod ws_bridge;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::client::MemLaneClient;
use crate::server::resp::{
    bulk, encode_to_vec, err, extract_command, integer, nil, ok, parse, pong, wrong_args,
    RespValue,
};
use crate::server::ws_bridge::{MetricEvent, MetricsSender};

// ── Server config ─────────────────────────────────────────────────────────────

/// Default TCP address — one port above Redis so both can run simultaneously
pub const DEFAULT_ADDR: &str = "127.0.0.1:6399";

/// Read buffer size per client connection
const READ_BUF_SIZE: usize = 8192;

// ── Server entry point ────────────────────────────────────────────────────────

/// Start the MemLane TCP server.
///
/// Binds to `addr`, accepts connections in a loop, and spawns a Tokio task
/// per client. The `metrics_tx` channel forwards timing events to the
/// WebSocket bridge for the live dashboard.
///
/// # Example
/// ```rust
/// let client = Arc::new(MemLaneClient::create().unwrap());
/// let (tx, rx) = ws_bridge::metrics_channel();
/// tokio::spawn(ws_bridge::run_ws_server(rx));
/// run_server(client, DEFAULT_ADDR, tx).await.unwrap();
/// ```
pub async fn run_server(
    client: Arc<MemLaneClient>,
    addr: &str,
    metrics_tx: MetricsSender,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("MemLane TCP server listening on {}", addr);
    tracing::info!("Connect with: redis-cli -p 6399");

    loop {
        let (socket, peer_addr) = listener.accept().await?;
        let client = Arc::clone(&client);
        let metrics_tx = metrics_tx.clone();

        tokio::spawn(async move {
            tracing::debug!("Client connected: {}", peer_addr);
            if let Err(e) = handle_client(socket, peer_addr, client, metrics_tx).await {
                tracing::debug!("Client {} disconnected: {}", peer_addr, e);
            }
        });
    }
}

// ── Per-client handler ────────────────────────────────────────────────────────

async fn handle_client(
    mut socket: TcpStream,
    peer: SocketAddr,
    client: Arc<MemLaneClient>,
    metrics_tx: MetricsSender,
) -> std::io::Result<()> {
    let mut buf = vec![0u8; READ_BUF_SIZE];
    let mut pending = Vec::new();

    loop {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            // Client disconnected
            return Ok(());
        }

        pending.extend_from_slice(&buf[..n]);

        // Parse and dispatch all complete commands in the buffer
        let mut offset = 0;
        loop {
            match parse(&pending[offset..]) {
                Ok(Some((val, consumed))) => {
                    offset += consumed;
                    let start = Instant::now();

                    let response = dispatch_command(&val, &client);
                    let elapsed_us = start.elapsed().as_micros() as u64;

                    // Emit metric event (non-blocking — drop if channel full)
                    let _ = metrics_tx.try_send(MetricEvent {
                        op: command_name(&val),
                        latency_us: elapsed_us,
                    });

                    let encoded = encode_to_vec(&response);
                    socket.write_all(&encoded).await?;
                }
                Ok(None) => break, // Need more data
                Err(e) => {
                    let response = err(&e.to_string());
                    socket.write_all(&encode_to_vec(&response)).await?;
                    pending.clear();
                    offset = 0;
                    break;
                }
            }
        }

        // Remove consumed bytes from pending buffer
        pending.drain(..offset);
        offset = 0;
    }
}

// ── Command dispatcher ────────────────────────────────────────────────────────

fn dispatch_command(val: &RespValue, client: &MemLaneClient) -> RespValue {
    let cmd = match extract_command(val) {
        Ok(c) => c,
        Err(e) => return err(&e),
    };

    match cmd.name.as_str() {
        "PING" => {
            if cmd.args.is_empty() {
                pong()
            } else {
                // PING with message: return the message as bulk string
                bulk(cmd.args[0].clone())
            }
        }

        "GET" => {
            if cmd.args.len() != 1 {
                return wrong_args("GET");
            }
            match client.get(&cmd.args[0]) {
                Ok(Some(val)) => bulk(val),
                Ok(None) => nil(),
                Err(e) => err(&e.to_string()),
            }
        }

        "SET" => {
            if cmd.args.len() < 2 {
                return wrong_args("SET");
            }
            let key = &cmd.args[0];
            let val = &cmd.args[1];

            // Parse optional EX <seconds> argument
            let mut ttl_secs: u64 = 0;
            let mut i = 2;
            while i < cmd.args.len() {
                let opt = String::from_utf8_lossy(&cmd.args[i]).to_uppercase();
                match opt.as_str() {
                    "EX" if i + 1 < cmd.args.len() => {
                        ttl_secs = String::from_utf8_lossy(&cmd.args[i + 1])
                            .parse()
                            .unwrap_or(0);
                        i += 2;
                    }
                    "PX" if i + 1 < cmd.args.len() => {
                        // PX = milliseconds — convert to seconds (round up)
                        let px: u64 = String::from_utf8_lossy(&cmd.args[i + 1])
                            .parse()
                            .unwrap_or(0);
                        ttl_secs = (px + 999) / 1000;
                        i += 2;
                    }
                    _ => i += 1,
                }
            }

            match client.set(key, val, ttl_secs) {
                Ok(_) => ok(),
                Err(e) => err(&e.to_string()),
            }
        }

        "DEL" => {
            if cmd.args.is_empty() {
                return wrong_args("DEL");
            }
            let mut deleted: i64 = 0;
            for key in &cmd.args {
                if let Ok(true) = client.del(key) {
                    deleted += 1;
                }
            }
            integer(deleted)
        }

        "EXISTS" => {
            if cmd.args.is_empty() {
                return wrong_args("EXISTS");
            }
            let mut count: i64 = 0;
            for key in &cmd.args {
                if let Ok(true) = client.exists(key) {
                    count += 1;
                }
            }
            integer(count)
        }

        "MGET" => {
            if cmd.args.is_empty() {
                return wrong_args("MGET");
            }
            let keys: Vec<&[u8]> = cmd.args.iter().map(|a| a.as_slice()).collect();
            match client.mget(&keys) {
                Ok(results) => RespValue::Array(
                    results
                        .into_iter()
                        .map(|r| match r {
                            Some(v) => bulk(v),
                            None => nil(),
                        })
                        .collect(),
                ),
                Err(e) => err(&e.to_string()),
            }
        }

        "MSET" => {
            if cmd.args.is_empty() || cmd.args.len() % 2 != 0 {
                return wrong_args("MSET");
            }
            let pairs: Vec<(&[u8], &[u8])> = cmd
                .args
                .chunks(2)
                .map(|c| (c[0].as_slice(), c[1].as_slice()))
                .collect();
            match client.mset(&pairs, 0) {
                Ok(_) => ok(),
                Err(e) => err(&e.to_string()),
            }
        }

        "INCR" => {
            if cmd.args.len() != 1 {
                return wrong_args("INCR");
            }
            match client.incr(&cmd.args[0]) {
                Ok(n) => integer(n as i64),
                Err(e) => err(&e.to_string()),
            }
        }

        "DECR" => {
            if cmd.args.len() != 1 {
                return wrong_args("DECR");
            }
            match client.decr(&cmd.args[0]) {
                Ok(n) => integer(n as i64),
                Err(e) => err(&e.to_string()),
            }
        }

        "TTL" => {
            if cmd.args.len() != 1 {
                return wrong_args("TTL");
            }
            // TTL support: look up slot expiry
            // For MVP, return -1 (no TTL info exposed at client level yet)
            integer(-1)
        }

        "FLUSH" | "FLUSHALL" | "FLUSHDB" => match client.flush() {
            Ok(_) => ok(),
            Err(e) => err(&e.to_string()),
        },

        "INFO" => {
            let used = client.used_count();
            let cap = client.capacity();
            let fill = client.fill_ratio() * 100.0;
            let info = format!(
                "# MemLane\r\nmemlane_version:0.1.0\r\nused_slots:{}\r\ntotal_slots:{}\r\nfill_ratio:{:.2}%\r\ntransport:shared_memory\r\nprotocol:RESP2\r\n",
                used, cap, fill
            );
            bulk(info.into_bytes())
        }

        "COMMAND" => {
            // Return empty array — enough for redis-cli to connect without errors
            RespValue::Array(vec![])
        }

        unknown => RespValue::Error(format!(
            "ERR unknown command '{}', try PING, GET, SET, DEL, EXISTS, MGET, MSET, INCR, DECR, FLUSH, INFO",
            unknown.to_lowercase()
        )),
    }
}

// ── Utility ───────────────────────────────────────────────────────────────────

fn command_name(val: &RespValue) -> String {
    match val {
        RespValue::Array(items) => match items.first() {
            Some(RespValue::BulkString(b)) => {
                String::from_utf8_lossy(b).to_uppercase()
            }
            _ => "UNKNOWN".to_string(),
        },
        _ => "UNKNOWN".to_string(),
    }
}