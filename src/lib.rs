//! MemLane — Zero-TCP Shared Memory Cache Engine
//!
//! This is the crate root. It re-exports the public API and exposes
//! C-compatible FFI symbols so other languages can link against MemLane
//! as a native shared library.
//!
//! # Usage (Rust)
//! ```rust
//! use memlane::client::MemLaneClient;
//!
//! let client = MemLaneClient::create().expect("Failed to create arena");
//! client.set(b"hello", b"world", 0).unwrap();
//! let val = client.get(b"hello").unwrap();
//! assert_eq!(val, Some(b"world".to_vec()));
//! ```
//!
//! # Usage (C)
//! Link against libmemlane.so and include c_bindings/memlane.h.

pub mod shm;
pub mod slot;
pub mod ttl;
pub mod hashmap;
pub mod client;
pub mod server;

pub use client::MemLaneClient;

// ── C FFI Layer ───────────────────────────────────────────────────────────────
//
// All FFI functions are prefixed `ml_` and use only C-compatible types.
// Strings are passed as ptr + len (no null termination assumed).
// The client handle is an opaque pointer to a heap-allocated MemLaneClient.

use std::ffi::c_void;

/// Opaque handle returned to C callers.
/// Internally a Box<MemLaneClient> cast to *mut c_void.
pub type MLHandle = *mut c_void;

/// Create a new MemLane arena and return an opaque handle.
/// Returns NULL on failure.
///
/// # Safety
/// The returned handle must be freed with `ml_close`.
#[no_mangle]
pub unsafe extern "C" fn ml_create() -> MLHandle {
    match MemLaneClient::create() {
        Ok(client) => Box::into_raw(Box::new(client)) as MLHandle,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Open an existing MemLane arena and return an opaque handle.
/// Returns NULL on failure.
///
/// # Safety
/// The returned handle must be freed with `ml_close`.
#[no_mangle]
pub unsafe extern "C" fn ml_open() -> MLHandle {
    match MemLaneClient::open() {
        Ok(client) => Box::into_raw(Box::new(client)) as MLHandle,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Close and free a MemLane handle.
///
/// # Safety
/// `handle` must be a valid pointer returned by `ml_create` or `ml_open`.
/// After this call, `handle` is invalid and must not be used.
#[no_mangle]
pub unsafe extern "C" fn ml_close(handle: MLHandle) {
    if !handle.is_null() {
        drop(Box::from_raw(handle as *mut MemLaneClient));
    }
}

/// SET a key-value pair. Returns 1 on success, 0 on failure (arena full).
///
/// # Safety
/// - `handle` must be valid
/// - `key_ptr` must point to `key_len` readable bytes
/// - `val_ptr` must point to `val_len` readable bytes
/// - `ttl_secs` = 0 means no expiry
#[no_mangle]
pub unsafe extern "C" fn ml_set(
    handle: MLHandle,
    key_ptr: *const u8,
    key_len: usize,
    val_ptr: *const u8,
    val_len: usize,
    ttl_secs: u64,
) -> i32 {
    if handle.is_null() || key_ptr.is_null() || val_ptr.is_null() {
        return 0;
    }
    let client = &*(handle as *const MemLaneClient);
    let key = std::slice::from_raw_parts(key_ptr, key_len);
    let val = std::slice::from_raw_parts(val_ptr, val_len);
    match client.set(key, val, ttl_secs) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// GET a value by key. Writes result into `out_buf` (caller-allocated).
/// Returns number of bytes written, or -1 if key not found, or -2 on error.
///
/// # Safety
/// - `handle` must be valid
/// - `key_ptr` must point to `key_len` readable bytes
/// - `out_buf` must point to at least `out_buf_len` writable bytes
#[no_mangle]
pub unsafe extern "C" fn ml_get(
    handle: MLHandle,
    key_ptr: *const u8,
    key_len: usize,
    out_buf: *mut u8,
    out_buf_len: usize,
) -> i64 {
    if handle.is_null() || key_ptr.is_null() || out_buf.is_null() {
        return -2;
    }
    let client = &*(handle as *const MemLaneClient);
    let key = std::slice::from_raw_parts(key_ptr, key_len);

    match client.get(key) {
        Ok(Some(val)) => {
            let copy_len = val.len().min(out_buf_len);
            std::ptr::copy_nonoverlapping(val.as_ptr(), out_buf, copy_len);
            copy_len as i64
        }
        Ok(None) => -1,
        Err(_) => -2,
    }
}

/// DEL a key. Returns 1 if deleted, 0 if not found.
///
/// # Safety
/// - `handle` must be valid
/// - `key_ptr` must point to `key_len` readable bytes
#[no_mangle]
pub unsafe extern "C" fn ml_del(
    handle: MLHandle,
    key_ptr: *const u8,
    key_len: usize,
) -> i32 {
    if handle.is_null() || key_ptr.is_null() {
        return 0;
    }
    let client = &*(handle as *const MemLaneClient);
    let key = std::slice::from_raw_parts(key_ptr, key_len);
    match client.del(key) {
        Ok(true) => 1,
        _ => 0,
    }
}

/// EXISTS: returns 1 if key exists, 0 otherwise.
///
/// # Safety
/// - `handle` must be valid
/// - `key_ptr` must point to `key_len` readable bytes
#[no_mangle]
pub unsafe extern "C" fn ml_exists(
    handle: MLHandle,
    key_ptr: *const u8,
    key_len: usize,
) -> i32 {
    if handle.is_null() || key_ptr.is_null() {
        return 0;
    }
    let client = &*(handle as *const MemLaneClient);
    let key = std::slice::from_raw_parts(key_ptr, key_len);
    match client.exists(key) {
        Ok(true) => 1,
        _ => 0,
    }
}

/// FLUSH: delete all keys. Returns 1 on success.
///
/// # Safety
/// `handle` must be valid. Not safe to call while other processes are active.
#[no_mangle]
pub unsafe extern "C" fn ml_flush(handle: MLHandle) -> i32 {
    if handle.is_null() {
        return 0;
    }
    let client = &*(handle as *const MemLaneClient);
    match client.flush() {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// Returns the number of currently occupied slots (approximate).
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn ml_used_count(handle: MLHandle) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let client = &*(handle as *const MemLaneClient);
    client.used_count()
}

/// Returns total slot capacity of the arena.
///
/// # Safety
/// `handle` must be valid.
#[no_mangle]
pub unsafe extern "C" fn ml_capacity(handle: MLHandle) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let client = &*(handle as *const MemLaneClient);
    client.capacity() as u64
}