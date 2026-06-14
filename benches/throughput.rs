//! MemLane Throughput Benchmark Suite
//!
//! Runs Criterion benchmarks comparing MemLane shared-memory operations
//! against Redis TCP operations side by side.
//!
//! Run with:
//!   cargo bench
//!
//! For HTML report:
//!   cargo bench -- --output-format html
//!   open target/criterion/report/index.html
//!
//! Prerequisites:
//!   Redis must be running on localhost:6379 for the comparison benchmarks.
//!   Start with: redis-server (or via WSL: wsl redis-server)
//!
//! Benchmarks:
//!   1. memlane_get          — MemLane GET via shared memory
//!   2. memlane_set          — MemLane SET via shared memory
//!   3. memlane_get_set_mix  — 80% GET / 20% SET mixed workload
//!   4. redis_get            — Redis GET via TCP (comparison baseline)
//!   5. redis_set            — Redis SET via TCP (comparison baseline)
//!   6. memlane_mget_10      — MemLane MGET of 10 keys at once
//!   7. memlane_concurrent   — MemLane GET from 4 threads simultaneously

use std::sync::Arc;
use std::thread;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use memlane::client::MemLaneClient;

// ── Fixtures ──────────────────────────────────────────────────────────────────

/// Number of pre-seeded keys available for GET benchmarks
const SEED_KEYS: usize = 10_000;

/// Key template
fn make_key(i: usize) -> Vec<u8> {
    format!("bench:key:{:06}", i).into_bytes()
}

/// Value template (64 bytes — typical small cache value)
fn make_value(i: usize) -> Vec<u8> {
    format!("value:{:06}:padding_to_64_bytes_xxxxxxxxxxxxxxxxxx", i).into_bytes()
}

// ── MemLane benchmarks ────────────────────────────────────────────────────────

fn bench_memlane_get(c: &mut Criterion) {
    let client = MemLaneClient::create().expect("Failed to create MemLane arena");

    // Seed keys
    for i in 0..SEED_KEYS {
        client.set(&make_key(i), &make_value(i), 0).unwrap();
    }

    let mut group = c.benchmark_group("memlane");
    group.throughput(Throughput::Elements(1));

    group.bench_function("GET", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let key = make_key(i % SEED_KEYS);
            let result = client.get(black_box(&key)).unwrap();
            black_box(result);
            i += 1;
        });
    });

    group.finish();
}

fn bench_memlane_set(c: &mut Criterion) {
    let client = MemLaneClient::create().expect("Failed to create MemLane arena");

    let mut group = c.benchmark_group("memlane");
    group.throughput(Throughput::Elements(1));

    group.bench_function("SET", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let key = make_key(i % SEED_KEYS);
            let val = make_value(i % SEED_KEYS);
            client.set(black_box(&key), black_box(&val), 0).unwrap();
            i += 1;
        });
    });

    group.finish();
}

fn bench_memlane_mixed(c: &mut Criterion) {
    let client = MemLaneClient::create().expect("Failed to create MemLane arena");

    // Seed keys
    for i in 0..SEED_KEYS {
        client.set(&make_key(i), &make_value(i), 0).unwrap();
    }

    let mut group = c.benchmark_group("memlane");
    group.throughput(Throughput::Elements(1));

    // 80% GET / 20% SET mixed workload
    group.bench_function("GET_SET_80_20_mix", |b| {
        let mut i = 0usize;
        b.iter(|| {
            if i % 5 == 0 {
                // 20% SET
                let key = make_key(i % SEED_KEYS);
                let val = make_value(i % SEED_KEYS);
                client.set(black_box(&key), black_box(&val), 0).unwrap();
            } else {
                // 80% GET
                let key = make_key(i % SEED_KEYS);
                let result = client.get(black_box(&key)).unwrap();
                black_box(result);
            }
            i += 1;
        });
    });

    group.finish();
}

fn bench_memlane_mget(c: &mut Criterion) {
    let client = MemLaneClient::create().expect("Failed to create MemLane arena");

    // Seed keys
    for i in 0..SEED_KEYS {
        client.set(&make_key(i), &make_value(i), 0).unwrap();
    }

    let mut group = c.benchmark_group("memlane");

    for batch_size in [1, 10, 50, 100] {
        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("MGET", batch_size),
            &batch_size,
            |b, &size| {
                let keys: Vec<Vec<u8>> = (0..size).map(|i| make_key(i % SEED_KEYS)).collect();
                b.iter(|| {
                    let key_slices: Vec<&[u8]> =
                        keys.iter().map(|k| k.as_slice()).collect();
                    let result = client.mget(black_box(&key_slices)).unwrap();
                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

fn bench_memlane_concurrent(c: &mut Criterion) {
    let client = Arc::new(
        MemLaneClient::create().expect("Failed to create MemLane arena"),
    );

    // Seed keys
    for i in 0..SEED_KEYS {
        client.set(&make_key(i), &make_value(i), 0).unwrap();
    }

    let mut group = c.benchmark_group("memlane");
    group.throughput(Throughput::Elements(4)); // 4 threads × 1 op each

    group.bench_function("GET_4_threads", |b| {
        b.iter(|| {
            let handles: Vec<_> = (0..4)
                .map(|t| {
                    let c = Arc::clone(&client);
                    thread::spawn(move || {
                        let key = make_key(t * 100 % SEED_KEYS);
                        let result = c.get(&key).unwrap();
                        black_box(result);
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
        });
    });

    group.finish();
}

// ── Redis comparison benchmarks ───────────────────────────────────────────────

fn bench_redis_get(c: &mut Criterion) {
    let redis_client = match redis::Client::open("redis://127.0.0.1:6379/") {
        Ok(c) => c,
        Err(_) => {
            eprintln!(
                "⚠ Redis not available on localhost:6379 — skipping Redis benchmarks.\n\
                 Start Redis with: redis-server (or in WSL: wsl redis-server)"
            );
            return;
        }
    };

    let mut con = match redis_client.get_connection() {
        Ok(c) => c,
        Err(_) => {
            eprintln!("⚠ Could not connect to Redis — skipping Redis benchmarks.");
            return;
        }
    };

    // Seed Redis keys
    for i in 0..SEED_KEYS {
        let key = String::from_utf8(make_key(i)).unwrap();
        let val = String::from_utf8(make_value(i)).unwrap();
        let _: () = redis::cmd("SET")
            .arg(&key)
            .arg(&val)
            .query(&mut con)
            .unwrap_or(());
    }

    let mut group = c.benchmark_group("redis_tcp");
    group.throughput(Throughput::Elements(1));

    group.bench_function("GET", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let key = String::from_utf8(make_key(i % SEED_KEYS)).unwrap();
            let result: Option<String> = redis::cmd("GET")
                .arg(black_box(&key))
                .query(&mut con)
                .unwrap_or(None);
            black_box(result);
            i += 1;
        });
    });

    group.finish();
}

fn bench_redis_set(c: &mut Criterion) {
    let redis_client = match redis::Client::open("redis://127.0.0.1:6379/") {
        Ok(c) => c,
        Err(_) => {
            eprintln!("⚠ Redis not available — skipping Redis SET benchmark.");
            return;
        }
    };

    let mut con = match redis_client.get_connection() {
        Ok(c) => c,
        Err(_) => {
            eprintln!("⚠ Could not connect to Redis — skipping.");
            return;
        }
    };

    let mut group = c.benchmark_group("redis_tcp");
    group.throughput(Throughput::Elements(1));

    group.bench_function("SET", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let key = String::from_utf8(make_key(i % SEED_KEYS)).unwrap();
            let val = String::from_utf8(make_value(i % SEED_KEYS)).unwrap();
            let _: () = redis::cmd("SET")
                .arg(black_box(&key))
                .arg(black_box(&val))
                .query(&mut con)
                .unwrap_or(());
            i += 1;
        });
    });

    group.finish();
}

// ── TTL benchmark ─────────────────────────────────────────────────────────────

fn bench_memlane_ttl(c: &mut Criterion) {
    let client = MemLaneClient::create().expect("Failed to create MemLane arena");

    let mut group = c.benchmark_group("memlane");
    group.throughput(Throughput::Elements(1));

    // SET with TTL
    group.bench_function("SET_with_TTL_60s", |b| {
        let mut i = 0usize;
        b.iter(|| {
            let key = make_key(i % SEED_KEYS);
            let val = make_value(i % SEED_KEYS);
            client
                .set(black_box(&key), black_box(&val), 60)
                .unwrap();
            i += 1;
        });
    });

    // Seed with short TTL, then benchmark expired GET (lazy eviction path)
    for i in 0..100 {
        client.set(&make_key(i), &make_value(i), 1).unwrap();
    }

    group.finish();
}

// ── Criterion groups ──────────────────────────────────────────────────────────

criterion_group!(
    memlane_benches,
    bench_memlane_get,
    bench_memlane_set,
    bench_memlane_mixed,
    bench_memlane_mget,
    bench_memlane_concurrent,
    bench_memlane_ttl,
);

criterion_group!(redis_benches, bench_redis_get, bench_redis_set);

criterion_main!(memlane_benches, redis_benches);