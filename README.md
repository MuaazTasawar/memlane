# MemLane

> A zero-TCP, shared memory cache written in Rust — faster than Redis, with a live React dashboard.

---

## Overview

MemLane is a high-performance in-process cache that bypasses TCP entirely by storing data in a POSIX shared memory arena mapped directly into process address space. Instead of round-tripping through the kernel's network stack, reads and writes touch physical memory directly — achieving over **1.3 million GET ops/sec** at sub-microsecond latency on commodity hardware. MemLane speaks the Redis RESP2 wire protocol, so any `redis-cli` client works out of the box with zero code changes. A React dashboard connects over WebSocket and renders live throughput, latency percentiles (P50/P99/P99.9), arena capacity, and a side-by-side MemLane vs Redis comparison chart — all updating in real time every second.

---

## Tech Stack

| Layer           | Technology                              |
|-----------------|-----------------------------------------|
| Core Cache      | Rust (stable), shared memory arena      |
| Wire Protocol   | RESP2 (Redis-compatible TCP server)     |
| Metrics Bridge  | Tokio async runtime, WebSocket (tungstenite) |
| Serialisation   | serde + serde_json                      |
| Benchmarks      | Criterion.rs                            |
| Dashboard       | React 18, Vite, Recharts                |
| IPC             | POSIX shm_open / mmap (Linux/WSL)       |
| C FFI           | cbindgen-compatible header (memlane.h)  |

---

## Features

- **Zero-TCP reads** — data lives in shared memory; no kernel network stack on the hot path
- **RESP2 compatibility** — connect with `redis-cli`, any Redis client library, or raw TCP
- **1.3M+ GET ops/sec** — benchmarked on WSL2 with Criterion
- **TTL support** — keys expire automatically; no background thread needed
- **Batch ops** — MSET / MGET for bulk inserts and lookups
- **INCR / DECR** — atomic integer operations
- **C FFI layer** — `memlane.h` header for embedding MemLane in C/C++ programs
- **WebSocket metrics bridge** — aggregates op events into 1-second windows and broadcasts JSON snapshots
- **Live React dashboard** — ops/sec counter, latency histogram (P50/P99/P99.9), arena fill bar, Redis comparison chart
- **Criterion benchmark suite** — reproducible throughput and latency measurements vs Redis baseline
- **Cross-process arena** — multiple processes can attach to the same named shared memory region

---

## Project Structure

```
MemLane/
├── src/
│   ├── lib.rs                  # Crate root — public API, MemLaneClient struct
│   ├── slot.rs                 # Slot layout: key, value, state, TTL atomics
│   ├── hashmap.rs              # Open-addressing hash map over the slot array
│   ├── shm.rs                  # Shared memory arena: create/open, ArenaHeader
│   ├── client.rs               # MemLaneClient: get/set/del/incr/mset/mget
│   └── server/
│       ├── mod.rs              # Async TCP server, RESP2 command dispatch
│       ├── resp.rs             # RESP2 parser and encoder
│       └── ws_bridge.rs        # WebSocket metrics aggregator and broadcaster
├── examples/
│   └── basic_usage.rs          # End-to-end demo: arena + ops + TCP server + WS bridge
├── benches/
│   └── throughput.rs           # Criterion benchmarks: GET/SET vs Redis baseline
├── dashboard/
│   ├── index.html              # Vite entry point
│   ├── package.json            # React + Recharts + Vite dependencies
│   └── src/
│       ├── main.jsx            # React root mount
│       ├── App.jsx             # Dashboard layout and WebSocket connection
│       ├── components/
│       │   ├── ThroughputCard.jsx    # Live ops/sec + peak + Redis baseline
│       │   ├── LatencyChart.jsx      # Recharts line chart: P50/P99/P99.9 over time
│       │   ├── ArenaCapacity.jsx     # Slot fill bar + latency summary
│       │   └── ComparisonBar.jsx     # MemLane vs Redis bar chart
│       └── hooks/
│           └── useWebSocket.js       # WebSocket hook with auto-reconnect
├── memlane.h                   # C FFI header (generated)
├── Cargo.toml                  # Rust dependencies and bench/example targets
└── README.md
```

---

## Getting Started

### Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Rust + Cargo | 1.70+ | https://rustup.rs |
| Node.js | 18 LTS+ | https://nodejs.org |
| Git | any | https://git-scm.com/download/win |
| WSL2 + Ubuntu | any | `wsl --install` in PowerShell as Admin |

> **Important:** MemLane uses POSIX shared memory (`shm_open` / `mmap`) which is only available on Linux. On Windows, run everything through WSL2. The dashboard runs natively on Windows.

---

### Clone the Repo

```bash
git clone https://github.com/MuaazTasawar/MemLane.git
cd MemLane
```

---

### Installation

#### 1. Install the C linker in WSL (required for Rust to compile)

Open Ubuntu (WSL) terminal:

```bash
sudo apt update && sudo apt install build-essential redis-tools -y
```

#### 2. Install Rust inside WSL

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

#### 3. Add the `windows-sys` crate (Windows compat layer)

In PowerShell or WSL inside the repo root:

```bash
cargo add windows-sys --features Win32_System_Memory,Win32_Foundation
```

#### 4. Install dashboard dependencies

In PowerShell (Windows side):

```powershell
cd dashboard
npm install
```

---

### Running the App

You need **three terminals** running simultaneously:

#### Terminal 1 — MemLane server (WSL/Ubuntu)

```bash
cd /mnt/d/Goooo/MemLane        # adjust path to where you cloned
cargo run --example basic_usage
```

Expected output:
```
MemLane — Zero-TCP Cache
✔ Arena ready: 65536 slots (85 MB)
✔ Basic operations: SET/GET/INCR/DEL all passing
✔ TTL demo: expiring keys working
✔ Throughput: 1,300,000+ ops/sec
Starting TCP server (RESP2) + WebSocket metrics bridge...
  TCP  : 127.0.0.1:6399
  WS   : ws://127.0.0.1:9001
  Dashboard: http://localhost:5173
```

#### Terminal 2 — Dashboard (PowerShell, Windows)

```powershell
cd D:\path\to\MemLane\dashboard
npm run dev
```

Open browser → **http://localhost:5173**

#### Terminal 3 — redis-cli test (WSL/Ubuntu)

```bash
/usr/bin/redis-cli -p 6399 PING
# → PONG

/usr/bin/redis-cli -p 6399 SET hello world
# → OK

/usr/bin/redis-cli -p 6399 GET hello
# → "world"

/usr/bin/redis-cli -p 6399 INCR counter
# → 1

/usr/bin/redis-cli -p 6399 TTL mykey
```

To generate live dashboard activity:

```bash
for i in $(seq 1 10000); do /usr/bin/redis-cli -p 6399 SET key$i val$i; done
```

---

### Running Benchmarks

In WSL (MemLane server must be running):

```bash
cargo bench
```

Criterion outputs an HTML report to `target/criterion/report/index.html`.

---

## Port Reference

| Port | Service |
|------|---------|
| 6399 | MemLane TCP server (RESP2 protocol) |
| 9001 | WebSocket metrics bridge |
| 5173 | React dashboard (Vite dev server) |

---

## How It Works

### Shared Memory Arena

MemLane allocates an 85MB named shared memory region (`/memlane_arena`) via `shm_open` + `mmap`. The region is laid out as:

```
[ ArenaHeader — 4096 bytes (1 page) ][ Slot × 65,536 ]
```

Each `Slot` holds a key, value, state flag, and TTL — all accessed via atomic operations. No mutexes. No copies. A GET operation is literally a hash lookup + memory read with no kernel involvement.

### RESP2 TCP Server

A Tokio async TCP server listens on port 6399 and speaks the Redis Serialization Protocol (RESP2). This means any Redis client — `redis-cli`, `redis-py`, `ioredis`, etc. — works with MemLane without any changes.

### WebSocket Metrics Bridge

After each TCP command, the server emits a `MetricEvent` (op name + latency in µs) into an async mpsc channel. A background aggregator collects events over 1-second windows, computes percentiles, and broadcasts a JSON `MetricsSnapshot` to all connected WebSocket clients (the dashboard).

### React Dashboard

The dashboard connects to `ws://127.0.0.1:9001` and renders four panels:
- **Throughput card** — live ops/sec, 60-second peak, Redis baseline reference
- **Latency chart** — rolling time series of P50 / P99 / P99.9 in microseconds
- **Arena capacity** — used slots / total slots, fill percentage bar
- **Comparison bar** — MemLane ops/sec vs Redis ops/sec side by side

---

## Performance Results

Measured on WSL2 (Ubuntu 24.04) on a mid-range Windows laptop:

| Operation | MemLane | Redis (localhost TCP) |
|-----------|---------|----------------------|
| GET       | ~1,300,000 ops/sec | ~400,000 ops/sec |
| SET       | ~700,000 ops/sec  | ~380,000 ops/sec |
| Latency P50 | < 1 µs | 50–300 µs |
| Latency P99 | 1–3 µs | 300–800 µs |

> MemLane is **3–4× faster** than Redis on GET because it never touches the TCP stack.

---

## Phase Build History

| Phase | Name | What Was Built |
|-------|------|----------------|
| 0 | Project Init | `Cargo.toml`, `.gitignore`, crate skeleton |
| 1 | Slot + Arena | `slot.rs`, `shm.rs` — shared memory layout and atomic slot operations |
| 2 | HashMap | `hashmap.rs` — open-addressing hash map over the slot array |
| 3 | Client Library + C FFI | `lib.rs`, `client.rs`, `memlane.h` — public Rust API and C header |
| 4 | TCP Server + RESP Parser | `server/mod.rs`, `server/resp.rs` — async RESP2 server |
| 5 | WebSocket Metrics Bridge | `server/ws_bridge.rs` — aggregator, broadcaster, percentile tracking |
| 6 | Benchmark Suite | `benches/throughput.rs`, `examples/basic_usage.rs` — Criterion benchmarks |
| 7 | React Dashboard | `dashboard/` — live ops/sec, latency chart, arena fill, Redis comparison |

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| `cargo: command not found` | Run `$env:PATH += ";$env:USERPROFILE\.cargo\bin"` in PowerShell |
| `error: linker cc not found` | Run `sudo apt install build-essential -y` in WSL |
| `Address already in use (port 6399)` | Run `sudo fuser -k 6399/tcp` in WSL |
| `redis-cli: command not found` | Use full path `/usr/bin/redis-cli` or run `sudo apt install redis-tools -y` |
| Dashboard blank page | Check `dashboard/src/main.jsx` is not empty; check browser console for errors |
| `windows_sys` compile error | Run `cargo add windows-sys --features Win32_System_Memory,Win32_Foundation` |
| `ArenaInit` panic on Windows | MemLane requires WSL — run `cargo run --example basic_usage` inside Ubuntu |

---

## Contributing

1. Fork the repo
2. Create a feature branch: `git checkout -b feature/your-feature`
3. Make your changes with tests
4. Run `cargo test && cargo bench` to verify nothing regressed
5. Open a PR with a clear description of what changed and why

Areas open for contribution:
- Windows native shared memory via `CreateFileMappingW` (currently stubbed)
- Persistent arena snapshots to disk
- LRU eviction policy when arena is full
- Cluster mode — multiple arena regions across NUMA nodes
- More RESP2 commands: `EXPIRE`, `KEYS`, `SCAN`, `TYPE`

---

## License

MIT © 2026 [Muaaz Tasawar](https://github.com/MuaazTasawar)
