//! TTL (Time-To-Live) utilities for MemLane.
//!
//! Expiry is stored as an absolute Unix timestamp in milliseconds inside
//! each Slot. This file provides helpers to:
//!   - Convert a relative TTL (seconds) to an absolute expiry timestamp
//!   - Check whether a slot has expired
//!   - Lazily mark expired slots as tombstones on read
//!
//! There is no background GC thread. Expiry is checked on every GET.
//! Expired slots are lazily converted to tombstones so future probes
//! can reclaim them on the next SET that lands on that slot.

use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::slot::{Slot, STATE_TOMBSTONE};

/// Returns the current time as Unix milliseconds.
#[inline]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System clock is before Unix epoch")
        .as_millis() as u64
}

/// Converts a TTL in seconds to an absolute expiry timestamp in milliseconds.
/// Returns 0 if ttl_secs is 0 (meaning: no expiry).
#[inline]
pub fn ttl_to_expiry(ttl_secs: u64) -> u64 {
    if ttl_secs == 0 {
        return 0;
    }
    now_ms() + ttl_secs * 1000
}

/// Returns true if the given slot has expired.
///
/// A slot with expires_at_ms == 0 never expires.
///
/// # Safety
/// Slot pointer must be valid and point to an OCCUPIED slot.
#[inline]
pub unsafe fn is_expired(slot: &Slot) -> bool {
    let expiry = slot.expires_at_ms.load(Ordering::Acquire);
    if expiry == 0 {
        return false; // no TTL set
    }
    now_ms() >= expiry
}

/// Lazily evict a slot if it has expired.
///
/// If expired, atomically transitions state from OCCUPIED → TOMBSTONE
/// so that:
///   - Future GETs skip it
///   - Future SETs can reclaim it
///
/// Returns true if the slot was expired and marked as tombstone.
///
/// # Safety
/// Slot pointer must be valid. State is managed via atomic CAS.
pub unsafe fn lazy_evict_if_expired(slot: &Slot) -> bool {
    if !is_expired(slot) {
        return false;
    }

    // Attempt to transition OCCUPIED → TOMBSTONE atomically.
    // If another thread beat us to it, that's fine — the slot is already dead.
    let _ = slot.state.compare_exchange(
        crate::slot::STATE_OCCUPIED,
        STATE_TOMBSTONE,
        Ordering::AcqRel,
        Ordering::Relaxed,
    );

    true
}

/// Remaining TTL in milliseconds for a slot.
/// Returns None if no TTL is set or slot has already expired.
pub unsafe fn remaining_ttl_ms(slot: &Slot) -> Option<u64> {
    let expiry = slot.expires_at_ms.load(Ordering::Acquire);
    if expiry == 0 {
        return None; // no TTL
    }
    let now = now_ms();
    if now >= expiry {
        return None; // already expired
    }
    Some(expiry - now)
}