//! MemLane Basic Usage Example
//!
//! Demonstrates the full MemLane API: create, set, get, del, TTL,
//! batch ops, and the live TCP server with Redis-compatible interface.
//!
//! Run with:
//!   cargo run --example basic_usage
//!
//! Then connect with redis-cli:
//!   redis-cli -p 6399 PING
//!   redis-cli -p 6399 SET hello world
//!   redis-cli -p 6399 GET hello

use std::sync::Arc;
use std::time::Instant;

use memlane::client::MemLaneClient;
use memlane::server::ws_bridge;

#[tokio::main]
async fn main() {
    println!("╔══════════════════════════════════════════════╗");
    println!("║          MemLane — Zero-TCP Cache            ║");
    println!("╚══════════════════════════════════════════════╝");
    println!();

    // ── 1. Create arena ───────────────────────────────────────────────────────
    println!("▶ Creating shared memory arena...");
    let client = Arc::new(
        MemLaneClient::create().expect("Failed to create arena — try running as admin or in WSL"),
    );
    println!(
        "  ✓ Arena ready: {} slots ({} MB)",
        client.capacity(),
        (client.capacity() * 1300) / 1_000_000
    );
    println!();

    // ── 2. Basic SET / GET / DEL ──────────────────────────────────────────────
    println!("▶ Basic operations:");

    client.set(b"hello", b"world", 0).unwrap();
    let val = client.get(b"hello").unwrap();
    println!("  SET hello world  →  OK");
    println!(
        "  GET hello        →  {:?}",
        val.map(|v| String::from_utf8_lossy(&v).to_string())
    );

    client.set(b"counter", b"0", 0).unwrap();
    let n = client.incr(b"counter").unwrap();
    println!("  INCR counter     →  {}", n);
    let n = client.incr(b"counter").unwrap();
    println!("  INCR counter     →  {}", n);
    let n = client.decr(b"counter").unwrap();
    println!("  DECR counter     →  {}", n);

    client.del(b"hello").unwrap();
    let val = client.get(b"hello").unwrap();
    println!("  DEL hello        →  OK");
    println!("  GET hello        →  {:?}", val);
    println!();

    // ── 3. TTL demo ───────────────────────────────────────────────────────────
    println!("▶ TTL demo:");
    client.set(b"expiring_key", b"i will disappear", 2).unwrap();
    println!("  SET expiring_key 'i will disappear' EX 2");

    let val = client.get(b"expiring_key").unwrap();
    println!(
        "  GET expiring_key (immediately)  →  {:?}",
        val.map(|v| String::from_utf8_lossy(&v).to_string())
    );

    println!("  Sleeping 3 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(3));

    let val = client.get(b"expiring_key").unwrap();
    println!("  GET expiring_key (after 3s)     →  {:?}", val);
    println!();

    // ── 4. Batch operations ───────────────────────────────────────────────────
    println!("▶ Batch operations:");
    let pairs: Vec<(&[u8], &[u8])> = vec![
        (b"user:1", b"alice"),
        (b"user:2", b"bob"),
        (b"user:3", b"carol"),
        (b"user:4", b"dave"),
        (b"user:5", b"eve"),
    ];
    let count = client.mset(&pairs, 0).unwrap();
    println!("  MSET 5 users     →  {} inserted", count);

    let keys: Vec<&[u8]> = vec![
        b"user:1", b"user:2", b"user:3", b"user:99",
    ];
    let results = client.mget(&keys).unwrap();
    println!("  MGET user:1 user:2 user:3 user:99  →");
    for (key, result) in keys.iter().zip(results.iter()) {
        let k = String::from_utf8_lossy(key);
        let v = result
            .as_ref()
            .map(|v| String::from_utf8_lossy(v).to_string())
            .unwrap_or_else(|| "(nil)".to_string());
        println!("    {} → {}", k, v);
    }
    println!();

    // ── 5. Throughput demo ────────────────────────────────────────────────────
    println!("▶ Quick throughput demo (1,000,000 ops):");

    // Seed one key
    client.set(b"bench:key", b"bench:value", 0).unwrap();

    // GET throughput
    let start = Instant::now();
    let ops = 1_000_000usize;
    for _ in 0..ops {
        let _ = client.get(b"bench:key").unwrap();
    }
    let elapsed = start.elapsed();
    let ops_per_sec = ops as f64 / elapsed.as_secs_f64();
    let ns_per_op = elapsed.as_nanos() as f64 / ops as f64;
    println!(
        "  GET × {:>9}  →  {:>12.0} ops/sec  ({:.1} ns/op)",
        ops, ops_per_sec, ns_per_op
    );

    // SET throughput
    let start = Instant::now();
    for i in 0..ops {
        let key = format!("bench:{}", i % 10_000);
        client.set(key.as_bytes(), b"value", 0).unwrap();
    }
    let elapsed = start.elapsed();
    let ops_per_sec = ops as f64 / elapsed.as_secs_f64();
    let ns_per_op = elapsed.as_nanos() as f64 / ops as f64;
    println!(
        "  SET × {:>9}  →  {:>12.0} ops/sec  ({:.1} ns/op)",
        ops, ops_per_sec, ns_per_op
    );
    println!();

    // ── 6. Slot stats ─────────────────────────────────────────────────────────
    println!("▶ Arena stats:");
    println!("  Used slots  : {}", client.used_count());
    println!("  Total slots : {}", client.capacity());
    println!("  Fill ratio  : {:.2}%", client.fill_ratio() * 100.0);
    println!();

    // ── 7. TCP server + WS bridge ─────────────────────────────────────────────
    println!("▶ Starting TCP server (RESP2) + WebSocket metrics bridge...");
    println!("  Connect with : redis-cli -p 6399");
    println!("  Dashboard    : open dashboard/ and run npm run dev");
    println!("  Press Ctrl+C to stop.");
    println!();

    ws_bridge::start_all(client, memlane::server::DEFAULT_ADDR)
        .await
        .expect("Server error");
}