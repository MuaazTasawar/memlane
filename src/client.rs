//! MemLaneClient — the primary Rust API for MemLane.
//!
//! Wraps the Arena and exposes safe (well, safer) methods over the
//! underlying unsafe hashmap operations. This is what Rust callers
//! link against directly — in-process, zero-copy, zero-TCP.
//!
//! # Example
//! ```rust
//! let client = MemLaneClient::create().unwrap();
//! client.set(b"user:1001", b"alice", 3600).unwrap(); // TTL = 1 hour
//! let val = client.get(b"user:1001").unwrap();
//! println!("{:?}", val); // Some([97, 108, 105, 99, 101])
//! ```

use crate::hashmap::{self, DelResult, GetResult, SetResult};
use crate::shm::{Arena, create_arena, open_arena};

/// Error type for MemLane client operations
#[derive(Debug, thiserror::Error)]
pub enum MemLaneError {
    #[error("Arena initialisation failed: {0}")]
    ArenaInit(String),

    #[error("Arena is full — all {0} slots are occupied")]
    ArenaFull(usize),

    #[error("Key too long: {0} bytes (max 128)")]
    KeyTooLong(usize),

    #[error("Value too long: {0} bytes (max 1024)")]
    ValueTooLong(usize),
}

pub type Result<T> = std::result::Result<T, MemLaneError>;

/// A client connected to a MemLane shared memory arena.
///
/// Multiple `MemLaneClient` instances in different processes (or threads)
/// can share the same arena safely — all synchronisation is done via
/// atomics inside the slot array.
pub struct MemLaneClient {
    arena: Arena,
}

impl MemLaneClient {
    /// Create a new shared memory arena and return a connected client.
    ///
    /// Call this once from the "server" or initialising process.
    /// Other processes should call `open()` instead.
    pub fn create() -> Result<Self> {
        let arena = create_arena().map_err(MemLaneError::ArenaInit)?;
        Ok(Self { arena })
    }

    /// Connect to an existing shared memory arena.
    ///
    /// The arena must have been created by a prior call to `create()`.
    pub fn open() -> Result<Self> {
        let arena = open_arena().map_err(MemLaneError::ArenaInit)?;
        Ok(Self { arena })
    }

    // ── Core operations ───────────────────────────────────────────────────────

    /// GET: retrieve the value for a key.
    ///
    /// Returns `Ok(Some(value))` if found, `Ok(None)` if not found or expired.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if key.len() > crate::slot::MAX_KEY_LEN {
            return Err(MemLaneError::KeyTooLong(key.len()));
        }
        let result = unsafe { hashmap::get(&self.arena, key) };
        match result {
            GetResult::Found(val) => Ok(Some(val)),
            GetResult::NotFound | GetResult::Expired => Ok(None),
        }
    }

    /// SET: insert or update a key-value pair.
    ///
    /// `ttl_secs = 0` means no expiry.
    /// Returns `Ok(true)` if inserted, `Ok(false)` if updated in place.
    pub fn set(&self, key: &[u8], value: &[u8], ttl_secs: u64) -> Result<bool> {
        if key.len() > crate::slot::MAX_KEY_LEN {
            return Err(MemLaneError::KeyTooLong(key.len()));
        }
        if value.len() > crate::slot::MAX_VAL_LEN {
            return Err(MemLaneError::ValueTooLong(value.len()));
        }
        let result = unsafe { hashmap::set(&self.arena, key, value, ttl_secs) };
        match result {
            SetResult::Inserted => Ok(true),
            SetResult::Updated => Ok(false),
            SetResult::Full => Err(MemLaneError::ArenaFull(crate::shm::SLOT_COUNT)),
        }
    }

    /// DEL: delete a key. Returns `Ok(true)` if deleted, `Ok(false)` if not found.
    pub fn del(&self, key: &[u8]) -> Result<bool> {
        if key.len() > crate::slot::MAX_KEY_LEN {
            return Err(MemLaneError::KeyTooLong(key.len()));
        }
        let result = unsafe { hashmap::del(&self.arena, key) };
        Ok(matches!(result, DelResult::Deleted))
    }

    /// EXISTS: check if a key exists without fetching the value.
    pub fn exists(&self, key: &[u8]) -> Result<bool> {
        if key.len() > crate::slot::MAX_KEY_LEN {
            return Err(MemLaneError::KeyTooLong(key.len()));
        }
        Ok(unsafe { hashmap::exists(&self.arena, key) })
    }

    /// FLUSH: delete all keys in the arena.
    ///
    /// ⚠ Not safe to call while other processes are actively reading/writing.
    pub fn flush(&self) -> Result<()> {
        unsafe { hashmap::flush_all(&self.arena) };
        Ok(())
    }

    // ── Metadata ──────────────────────────────────────────────────────────────

    /// Returns the approximate number of occupied slots.
    pub fn used_count(&self) -> u64 {
        hashmap::used_count(&self.arena)
    }

    /// Returns total slot capacity.
    pub fn capacity(&self) -> usize {
        hashmap::capacity(&self.arena)
    }

    /// Returns fill ratio as a float between 0.0 and 1.0.
    pub fn fill_ratio(&self) -> f64 {
        self.used_count() as f64 / self.capacity() as f64
    }

    // ── Batch operations ──────────────────────────────────────────────────────

    /// MGET: retrieve multiple keys at once.
    ///
    /// Returns a Vec of Option<Vec<u8>> in the same order as input keys.
    /// Much faster than N individual GETs from a TCP client since there
    /// is zero network overhead between calls.
    pub fn mget(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>> {
        keys.iter().map(|k| self.get(k)).collect()
    }

    /// MSET: insert multiple key-value pairs at once.
    ///
    /// `ttl_secs` applies to all keys. Returns count of successful inserts.
    pub fn mset(&self, pairs: &[(&[u8], &[u8])], ttl_secs: u64) -> Result<usize> {
        let mut count = 0;
        for (key, val) in pairs {
            self.set(key, val, ttl_secs)?;
            count += 1;
        }
        Ok(count)
    }

    /// INCR: increment a numeric value stored as ASCII digits.
    ///
    /// If the key doesn't exist, initialises it to 1.
    /// Returns the new value as u64.
    pub fn incr(&self, key: &[u8]) -> Result<u64> {
        let current = match self.get(key)? {
            Some(bytes) => {
                let s = String::from_utf8(bytes).unwrap_or_else(|_| "0".to_string());
                s.parse::<u64>().unwrap_or(0)
            }
            None => 0,
        };
        let next = current + 1;
        let next_str = next.to_string();
        self.set(key, next_str.as_bytes(), 0)?;
        Ok(next)
    }

    /// DECR: decrement a numeric value stored as ASCII digits.
    ///
    /// Saturates at 0 (does not go negative).
    pub fn decr(&self, key: &[u8]) -> Result<u64> {
        let current = match self.get(key)? {
            Some(bytes) => {
                let s = String::from_utf8(bytes).unwrap_or_else(|_| "0".to_string());
                s.parse::<u64>().unwrap_or(0)
            }
            None => 0,
        };
        let next = current.saturating_sub(1);
        let next_str = next.to_string();
        self.set(key, next_str.as_bytes(), 0)?;
        Ok(next)
    }
}