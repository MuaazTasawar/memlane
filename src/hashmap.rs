//! Lock-Free Hash Map for MemLane.
//!
//! Implements an open-addressing hash map directly on top of the shared
//! memory slot array from shm.rs. Key design decisions:
//!
//!   - Open addressing with linear probing (cache-friendly, no pointers)
//!   - No mutexes — all concurrency via atomic CAS on slot.state
//!   - Fingerprinting: 64-bit SipHash prefix stored in slot lets us reject
//!     mismatches without reading the full key buffer (avoids cache miss)
//!   - Tombstone-based deletion: DEL marks slot as TOMBSTONE so probing
//!     chains remain intact for keys inserted after a collision
//!   - Lazy TTL eviction: expired slots are tombstoned on first GET hit
//!
//! Probe sequence: index = (hash(key) + i) % SLOT_COUNT for i = 0, 1, 2...
//! This is classic linear probing — simple and CPU-cache friendly.
//!
//! Thread safety model:
//!   - Multiple concurrent GETs: always safe (read-only path, no atomics mutated)
//!   - Concurrent SET + GET: safe via Acquire/Release ordering on state
//!   - Concurrent SET + SET on same key: last writer wins (no MVCC in MVP)
//!   - Concurrent DEL + GET: safe via tombstone transition

use std::sync::atomic::Ordering;

use crate::shm::{Arena, SLOT_COUNT};
use crate::slot::{Slot, STATE_EMPTY, STATE_OCCUPIED, STATE_TOMBSTONE};
use crate::ttl::{lazy_evict_if_expired, ttl_to_expiry};

// ── SipHash-1-3 inline implementation ────────────────────────────────────────
// We implement a minimal SipHash here to avoid a dependency and keep the
// fingerprint computation fast and inlinable.

#[inline(always)]
fn sip_round(v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64) {
    *v0 = v0.wrapping_add(*v1);
    *v1 = v1.rotate_left(13);
    *v1 ^= *v0;
    *v0 = v0.rotate_left(32);
    *v2 = v2.wrapping_add(*v3);
    *v3 = v3.rotate_left(16);
    *v3 ^= *v2;
    *v0 = v0.wrapping_add(*v3);
    *v3 = v3.rotate_left(21);
    *v3 ^= *v0;
    *v2 = v2.wrapping_add(*v1);
    *v1 = v1.rotate_left(17);
    *v1 ^= *v2;
    *v2 = v2.rotate_left(32);
}

/// Compute a 64-bit SipHash-1-3 of the given bytes.
/// Uses fixed keys (k0, k1) — sufficient for non-adversarial internal use.
pub fn siphash(data: &[u8]) -> u64 {
    let k0: u64 = 0x736f6d6570736575;
    let k1: u64 = 0x646f72616e646f6d;

    let mut v0 = k0 ^ 0x736f6d6570736575u64;
    let mut v1 = k1 ^ 0x646f72616e646f6du64;
    let mut v2 = k0 ^ 0x6c7967656e657261u64;
    let mut v3 = k1 ^ 0x7465646279746573u64;

    let mut chunks = data.chunks_exact(8);
    for chunk in chunks.by_ref() {
        let m = u64::from_le_bytes(chunk.try_into().unwrap());
        v3 ^= m;
        sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
        v0 ^= m;
    }

    // Handle remaining bytes
    let remainder = chunks.remainder();
    let mut last: u64 = (data.len() as u64 & 0xff) << 56;
    for (i, &byte) in remainder.iter().enumerate() {
        last |= (byte as u64) << (i * 8);
    }

    v3 ^= last;
    sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= last;

    // Finalisation
    v2 ^= 0xff;
    for _ in 0..3 {
        sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
    }

    v0 ^ v1 ^ v2 ^ v3
}

// ── Primary slot index from key hash ─────────────────────────────────────────

/// Compute the starting probe index for a key.
#[inline(always)]
fn primary_index(key: &[u8]) -> usize {
    (siphash(key) as usize) & (SLOT_COUNT - 1) // fast modulo since SLOT_COUNT is power of 2
}

// ── Result types ──────────────────────────────────────────────────────────────

/// Result of a GET operation
#[derive(Debug, Clone, PartialEq)]
pub enum GetResult {
    /// Key found, value returned
    Found(Vec<u8>),
    /// Key not found (or expired)
    NotFound,
    /// Key was found but has expired (slot lazily tombstoned)
    Expired,
}

/// Result of a SET operation
#[derive(Debug, Clone, PartialEq)]
pub enum SetResult {
    /// Key inserted into an empty or tombstone slot
    Inserted,
    /// Key already existed — value updated in place
    Updated,
    /// Arena is full — no empty or tombstone slots found in probe window
    Full,
}

/// Result of a DEL operation
#[derive(Debug, Clone, PartialEq)]
pub enum DelResult {
    /// Key found and deleted (slot → tombstone)
    Deleted,
    /// Key not found
    NotFound,
}

// ── Core HashMap operations ───────────────────────────────────────────────────

/// GET: retrieve the value for a key from the shared memory arena.
///
/// Probe sequence: linear from primary_index(key).
/// Returns on first EMPTY slot (key definitely absent) or after full scan.
///
/// # Safety
/// Arena must be a valid, initialised shared memory region.
pub unsafe fn get(arena: &Arena, key: &[u8]) -> GetResult {
    let hash = siphash(key);
    let start = (hash as usize) & (SLOT_COUNT - 1);

    for i in 0..SLOT_COUNT {
        let idx = (start + i) & (SLOT_COUNT - 1);
        let slot = arena.slot(idx);

        let state = slot.state.load(Ordering::Acquire);

        match state {
            STATE_EMPTY => {
                // Empty slot in probe chain — key definitely not present
                return GetResult::NotFound;
            }

            STATE_TOMBSTONE => {
                // Deleted slot — keep probing (key may be further along)
                continue;
            }

            STATE_OCCUPIED => {
                // Fast reject: fingerprint mismatch means different key
                let fp = slot.fingerprint.load(Ordering::Acquire);
                if fp != hash {
                    continue;
                }

                // Lazy TTL eviction — check expiry before reading key
                if lazy_evict_if_expired(slot) {
                    return GetResult::Expired;
                }

                // Full key comparison to rule out hash collisions
                let stored_key = slot.read_key();
                if stored_key.as_slice() != key {
                    continue;
                }

                // Key matches — read value
                let value = slot.read_value();
                return GetResult::Found(value);
            }

            _ => continue, // Unknown state — skip defensively
        }
    }

    GetResult::NotFound
}

/// SET: insert or update a key-value pair in the shared memory arena.
///
/// Probe sequence: linear from primary_index(key).
/// On first pass, records the first tombstone seen (reuse candidate).
/// If the key already exists, updates in place.
/// Otherwise, claims the first tombstone or empty slot via CAS.
///
/// # Safety
/// Arena must be a valid, initialised shared memory region.
pub unsafe fn set(
    arena: &Arena,
    key: &[u8],
    value: &[u8],
    ttl_secs: u64,
) -> SetResult {
    let hash = siphash(key);
    let start = (hash as usize) & (SLOT_COUNT - 1);
    let expiry = ttl_to_expiry(ttl_secs);

    let mut first_tombstone: Option<usize> = None;

    for i in 0..SLOT_COUNT {
        let idx = (start + i) & (SLOT_COUNT - 1);
        let slot = arena.slot(idx);

        let state = slot.state.load(Ordering::Acquire);

        match state {
            STATE_EMPTY => {
                // Use this slot (or a tombstone we found earlier)
                let target_idx = first_tombstone.unwrap_or(idx);
                let target_slot = arena.slot_mut(target_idx);
                claim_slot(target_slot, hash, key, value, expiry);
                // Update used_count (approximate — relaxed is fine)
                arena
                    .header()
                    .used_count
                    .fetch_add(1, Ordering::Relaxed);
                return SetResult::Inserted;
            }

            STATE_TOMBSTONE => {
                // Record first tombstone as reuse candidate
                if first_tombstone.is_none() {
                    first_tombstone = Some(idx);
                }
                continue;
            }

            STATE_OCCUPIED => {
                // Fast fingerprint check
                let fp = slot.fingerprint.load(Ordering::Acquire);
                if fp != hash {
                    continue;
                }

                // Check if expired — if so, treat as tombstone candidate
                if lazy_evict_if_expired(slot) {
                    if first_tombstone.is_none() {
                        first_tombstone = Some(idx);
                    }
                    continue;
                }

                // Full key comparison
                let stored_key = slot.read_key();
                if stored_key.as_slice() != key {
                    continue;
                }

                // Key already exists — update value and expiry in place
                let slot_mut = arena.slot_mut(idx);
                slot_mut.write_value(value);
                slot_mut.expires_at_ms.store(expiry, Ordering::Release);
                return SetResult::Updated;
            }

            _ => continue,
        }
    }

    // If we found a tombstone but no empty slot, reuse the tombstone
    if let Some(t_idx) = first_tombstone {
        let target_slot = arena.slot_mut(t_idx);
        claim_slot(target_slot, hash, key, value, expiry);
        return SetResult::Inserted;
    }

    SetResult::Full
}

/// DEL: mark a key's slot as a tombstone.
///
/// Does NOT zero out the key/value buffers — tombstone state is sufficient
/// to logically delete. The buffers are overwritten on the next SET that
/// claims this slot.
///
/// # Safety
/// Arena must be a valid, initialised shared memory region.
pub unsafe fn del(arena: &Arena, key: &[u8]) -> DelResult {
    let hash = siphash(key);
    let start = (hash as usize) & (SLOT_COUNT - 1);

    for i in 0..SLOT_COUNT {
        let idx = (start + i) & (SLOT_COUNT - 1);
        let slot = arena.slot(idx);

        let state = slot.state.load(Ordering::Acquire);

        match state {
            STATE_EMPTY => return DelResult::NotFound,

            STATE_TOMBSTONE => continue,

            STATE_OCCUPIED => {
                let fp = slot.fingerprint.load(Ordering::Acquire);
                if fp != hash {
                    continue;
                }

                let stored_key = slot.read_key();
                if stored_key.as_slice() != key {
                    continue;
                }

                // Atomically transition OCCUPIED → TOMBSTONE
                let result = slot.state.compare_exchange(
                    STATE_OCCUPIED,
                    STATE_TOMBSTONE,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );

                if result.is_ok() {
                    arena
                        .header()
                        .used_count
                        .fetch_sub(1, Ordering::Relaxed);
                    return DelResult::Deleted;
                }

                // CAS failed — another thread modified state concurrently.
                // Re-read and retry this slot.
                continue;
            }

            _ => continue,
        }
    }

    DelResult::NotFound
}

/// EXISTS: check if a key is present without returning the value.
///
/// Slightly faster than GET since it skips the value copy.
///
/// # Safety
/// Arena must be a valid, initialised shared memory region.
pub unsafe fn exists(arena: &Arena, key: &[u8]) -> bool {
    matches!(get(arena, key), GetResult::Found(_))
}

/// FLUSH: mark all slots as empty.
///
/// WARNING: Not safe to call while other processes are reading/writing.
/// Intended for testing and reset scenarios only.
///
/// # Safety
/// Caller must ensure exclusive access to the arena.
pub unsafe fn flush_all(arena: &Arena) {
    for i in 0..SLOT_COUNT {
        let slot = arena.slot(i);
        slot.state.store(STATE_EMPTY, Ordering::Release);
        slot.fingerprint.store(0, Ordering::Release);
        slot.expires_at_ms.store(0, Ordering::Release);
        slot.key_len.store(0, Ordering::Release);
        slot.val_len.store(0, Ordering::Release);
    }
    arena.header().used_count.store(0, Ordering::Release);
}

/// Returns the approximate number of occupied slots.
pub fn used_count(arena: &Arena) -> u64 {
    arena.header().used_count.load(Ordering::Relaxed)
}

/// Returns the total slot capacity.
pub fn capacity(_arena: &Arena) -> usize {
    SLOT_COUNT
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Write key, value, fingerprint, and expiry into a slot, then atomically
/// set its state to OCCUPIED.
///
/// The write order matters for safety:
///   1. Write key + value buffers (not yet visible — state is not OCCUPIED)
///   2. Write fingerprint and expiry
///   3. Store state = OCCUPIED with Release ordering (makes all prior writes visible)
///
/// # Safety
/// Caller must own this slot (either it was EMPTY or we won a CAS on a tombstone).
unsafe fn claim_slot(
    slot: &mut Slot,
    fingerprint: u64,
    key: &[u8],
    value: &[u8],
    expiry: u64,
) {
    // Step 1: write data (invisible until state flips to OCCUPIED)
    slot.write_key(key);
    slot.write_value(value);

    // Step 2: write metadata
    slot.fingerprint.store(fingerprint, Ordering::Relaxed);
    slot.expires_at_ms.store(expiry, Ordering::Relaxed);

    // Step 3: publish — Release ensures all prior stores are visible
    // to any thread that subsequently loads state with Acquire
    slot.state.store(STATE_OCCUPIED, Ordering::Release);
}