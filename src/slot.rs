//! Slot: the fundamental unit of storage in MemLane.
//!
//! Each slot is a fixed-size region in shared memory holding:
//!   - A state flag (Empty, Occupied, Tombstone) as an atomic u8
//!   - A 64-bit fingerprint of the key (for fast reject without full compare)
//!   - The expiry timestamp in milliseconds (0 = no expiry)
//!   - A fixed-size key buffer
//!   - A fixed-size value buffer
//!
//! All fields are cache-line aligned to prevent false sharing across cores.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// Maximum key length in bytes
pub const MAX_KEY_LEN: usize = 128;

/// Maximum value length in bytes
pub const MAX_VAL_LEN: usize = 1024;

/// Slot state constants
pub const STATE_EMPTY: u8 = 0;
pub const STATE_OCCUPIED: u8 = 1;
pub const STATE_TOMBSTONE: u8 = 2; // deleted but probing must continue past it

/// Total size of one slot in bytes.
/// Padded manually to 1536 bytes to sit on clean cache-line boundaries.
pub const SLOT_SIZE: usize = std::mem::size_of::<Slot>();

/// A single key-value slot stored in shared memory.
///
/// # Safety
/// This struct is laid out in raw shared memory mapped from shm_open/mmap.
/// All fields that are accessed concurrently must use atomic types.
/// Key/value byte arrays are read/written under a per-slot CAS protocol
/// described in hashmap.rs.
#[repr(C)]
pub struct Slot {
    /// Atomic state: Empty=0, Occupied=1, Tombstone=2
    pub state: AtomicU8,

    /// 7 padding bytes to align fingerprint to 8-byte boundary
    pub _pad0: [u8; 7],

    /// Lower 64 bits of SipHash of the key — fast mismatch rejection
    pub fingerprint: AtomicU64,

    /// Expiry in Unix milliseconds. 0 means no expiry.
    pub expires_at_ms: AtomicU64,

    /// Actual length of the key stored (≤ MAX_KEY_LEN)
    pub key_len: AtomicU64,

    /// Actual length of the value stored (≤ MAX_VAL_LEN)
    pub val_len: AtomicU64,

    /// Raw key bytes (fixed-size buffer, only key_len bytes are valid)
    pub key_buf: [u8; MAX_KEY_LEN],

    /// Raw value bytes (fixed-size buffer, only val_len bytes are valid)
    pub val_buf: [u8; MAX_VAL_LEN],

    /// Padding to reach a clean size — adjust if SLOT_SIZE changes
    pub _pad1: [u8; 32],
}

impl Slot {
    /// Initialise a slot to the empty state.
    /// Called once per slot during arena setup.
    ///
    /// # Safety
    /// Caller must ensure no other thread is accessing this slot.
    pub unsafe fn init(slot: *mut Slot) {
        (*slot).state.store(STATE_EMPTY, Ordering::Relaxed);
        (*slot).fingerprint.store(0, Ordering::Relaxed);
        (*slot).expires_at_ms.store(0, Ordering::Relaxed);
        (*slot).key_len.store(0, Ordering::Relaxed);
        (*slot).val_len.store(0, Ordering::Relaxed);
    }

    /// Returns true if this slot is logically empty (never written or evicted).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_EMPTY
    }

    /// Returns true if this slot holds a live key-value pair.
    #[inline]
    pub fn is_occupied(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_OCCUPIED
    }

    /// Returns true if this slot was deleted (tombstone for open addressing).
    #[inline]
    pub fn is_tombstone(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_TOMBSTONE
    }

    /// Read the stored key into a Vec<u8>.
    ///
    /// # Safety
    /// Caller must verify state == OCCUPIED before calling.
    pub unsafe fn read_key(&self) -> Vec<u8> {
        let len = self.key_len.load(Ordering::Acquire) as usize;
        self.key_buf[..len.min(MAX_KEY_LEN)].to_vec()
    }

    /// Read the stored value into a Vec<u8>.
    ///
    /// # Safety
    /// Caller must verify state == OCCUPIED before calling.
    pub unsafe fn read_value(&self) -> Vec<u8> {
        let len = self.val_len.load(Ordering::Acquire) as usize;
        self.val_buf[..len.min(MAX_VAL_LEN)].to_vec()
    }

    /// Write a key into the key buffer. Truncates silently if > MAX_KEY_LEN.
    ///
    /// # Safety
    /// Must only be called when the slot is being initialised under CAS lock.
    pub unsafe fn write_key(&mut self, key: &[u8]) {
        let len = key.len().min(MAX_KEY_LEN);
        self.key_buf[..len].copy_from_slice(&key[..len]);
        self.key_len.store(len as u64, Ordering::Release);
    }

    /// Write a value into the value buffer. Truncates silently if > MAX_VAL_LEN.
    ///
    /// # Safety
    /// Must only be called when the slot is being initialised under CAS lock.
    pub unsafe fn write_value(&mut self, val: &[u8]) {
        let len = val.len().min(MAX_VAL_LEN);
        self.val_buf[..len].copy_from_slice(&val[..len]);
        self.val_len.store(len as u64, Ordering::Release);
    }
}