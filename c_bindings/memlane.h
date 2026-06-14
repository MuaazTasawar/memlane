/**
 * memlane.h — C bindings for the MemLane shared memory cache engine.
 *
 * Link against libmemlane.so (Linux) or memlane.dll (Windows).
 *
 * Quick start:
 *
 *   #include "memlane.h"
 *   #include <stdio.h>
 *
 *   int main() {
 *       MLHandle h = ml_create();
 *       if (!h) { fprintf(stderr, "Failed to create arena\n"); return 1; }
 *
 *       const char* key = "hello";
 *       const char* val = "world";
 *       ml_set(h, (uint8_t*)key, 5, (uint8_t*)val, 5, 0);
 *
 *       uint8_t buf[1024];
 *       int64_t n = ml_get(h, (uint8_t*)key, 5, buf, sizeof(buf));
 *       if (n >= 0) printf("Got: %.*s\n", (int)n, buf);
 *
 *       ml_close(h);
 *       return 0;
 *   }
 *
 * Compile:
 *   gcc main.c -L. -lmemlane -o main
 */

#ifndef MEMLANE_H
#define MEMLANE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Opaque handle to a MemLane arena connection.
 * Returned by ml_create() and ml_open().
 * Must be freed with ml_close().
 */
typedef void* MLHandle;

/**
 * Create a new MemLane shared memory arena.
 * Call this once from the initialising process.
 *
 * @return Handle on success, NULL on failure.
 */
MLHandle ml_create(void);

/**
 * Open an existing MemLane arena created by ml_create().
 * Safe to call from multiple processes simultaneously.
 *
 * @return Handle on success, NULL on failure.
 */
MLHandle ml_open(void);

/**
 * Close a MemLane handle and free associated resources.
 * The shared memory region itself is NOT destroyed — other processes
 * connected to the same arena continue to work.
 *
 * @param handle  A valid handle from ml_create() or ml_open().
 */
void ml_close(MLHandle handle);

/**
 * SET a key-value pair in the arena.
 *
 * @param handle      Valid arena handle.
 * @param key_ptr     Pointer to key bytes (not null-terminated).
 * @param key_len     Length of key in bytes (max 128).
 * @param val_ptr     Pointer to value bytes (not null-terminated).
 * @param val_len     Length of value in bytes (max 1024).
 * @param ttl_secs    Time-to-live in seconds. 0 = no expiry.
 * @return 1 on success, 0 on failure (arena full or invalid args).
 */
int32_t ml_set(
    MLHandle handle,
    const uint8_t* key_ptr, size_t key_len,
    const uint8_t* val_ptr, size_t val_len,
    uint64_t ttl_secs
);

/**
 * GET a value by key, writing it into a caller-allocated buffer.
 *
 * @param handle       Valid arena handle.
 * @param key_ptr      Pointer to key bytes.
 * @param key_len      Length of key in bytes.
 * @param out_buf      Caller-allocated output buffer.
 * @param out_buf_len  Size of output buffer in bytes.
 * @return Number of bytes written on success,
 *         -1 if key not found or expired,
 *         -2 on error (null args, etc.).
 */
int64_t ml_get(
    MLHandle handle,
    const uint8_t* key_ptr, size_t key_len,
    uint8_t* out_buf, size_t out_buf_len
);

/**
 * DEL a key from the arena (marks slot as tombstone).
 *
 * @param handle    Valid arena handle.
 * @param key_ptr   Pointer to key bytes.
 * @param key_len   Length of key in bytes.
 * @return 1 if deleted, 0 if key not found.
 */
int32_t ml_del(
    MLHandle handle,
    const uint8_t* key_ptr, size_t key_len
);

/**
 * EXISTS: check if a key is present without fetching its value.
 *
 * @param handle    Valid arena handle.
 * @param key_ptr   Pointer to key bytes.
 * @param key_len   Length of key in bytes.
 * @return 1 if key exists, 0 otherwise.
 */
int32_t ml_exists(
    MLHandle handle,
    const uint8_t* key_ptr, size_t key_len
);

/**
 * FLUSH: mark all slots as empty.
 *
 * WARNING: Not safe to call while other processes are reading/writing.
 *
 * @param handle  Valid arena handle.
 * @return 1 on success, 0 on failure.
 */
int32_t ml_flush(MLHandle handle);

/**
 * Returns the approximate number of currently occupied slots.
 *
 * @param handle  Valid arena handle.
 * @return Occupied slot count (u64).
 */
uint64_t ml_used_count(MLHandle handle);

/**
 * Returns the total slot capacity of the arena.
 *
 * @param handle  Valid arena handle.
 * @return Total slot count (u64). Currently 65536.
 */
uint64_t ml_capacity(MLHandle handle);

#ifdef __cplusplus
}
#endif

#endif /* MEMLANE_H */