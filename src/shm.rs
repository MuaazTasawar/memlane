//! Shared Memory Arena for MemLane.
//!
//! This module creates and manages a POSIX shared memory region (on Linux/macOS)
//! or a Windows file mapping (on Windows) that holds the raw slot array.
//!
//! The layout of the shared memory region is:
//!
//!   [ ArenaHeader (4096 bytes, one OS page) ][ Slot × SLOT_COUNT ]
//!
//! ArenaHeader stores metadata: magic number, version, slot count, used count.
//! The slot array starts at offset 4096 (page-aligned).
//!
//! Multiple processes can open the same named region and get a pointer to the
//! same physical memory — no TCP, no copy, no kernel involvement on reads.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::slot::{Slot, SLOT_SIZE};

/// Magic number written into the header to detect corrupt/wrong regions
const ARENA_MAGIC: u64 = 0x4D454D4C414E4500; // "MEMLANE\0"

/// Version of the arena layout — bump if Slot layout changes
const ARENA_VERSION: u32 = 1;

/// Number of slots in the arena. Must be a power of two for fast modulo.
/// 64K slots × ~1.3KB each ≈ 85MB shared memory region.
pub const SLOT_COUNT: usize = 65536; // 2^16

/// Page size for header alignment
const PAGE_SIZE: usize = 4096;

/// Size of the full shared memory region in bytes
pub const ARENA_SIZE: usize = PAGE_SIZE + SLOT_COUNT * SLOT_SIZE;

/// Name of the shared memory object (visible in /dev/shm on Linux)
pub const SHM_NAME: &str = "/memlane_arena";

/// Arena header stored at offset 0 in the shared memory region.
/// Padded to exactly one OS page (4096 bytes).
#[repr(C)]
pub struct ArenaHeader {
    /// Magic number — must equal ARENA_MAGIC
    pub magic: AtomicU64,
    /// Layout version
    pub version: AtomicU32,
    /// Total number of slots in this arena
    pub slot_count: AtomicU32,
    /// Number of currently occupied slots (approximate, relaxed)
    pub used_count: AtomicU64,
    /// Padding to fill exactly one page
    pub _pad: [u8; PAGE_SIZE - 24],
}

/// The shared memory arena: a header + a flat array of slots.
pub struct Arena {
    /// Raw pointer to the start of the mmap'd region
    pub ptr: *mut u8,
    /// Total mapped size in bytes
    pub size: usize,
    /// OS-level handle (fd on Unix, HANDLE on Windows)
    #[cfg(unix)]
    pub fd: i32,
    #[cfg(windows)]
    pub handle: *mut std::ffi::c_void,
}

// Safety: Arena wraps a shared memory region. We manage synchronisation
// ourselves via atomics inside Slot. Sending across threads is intentional.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
    /// Returns a reference to the ArenaHeader at the start of the region.
    pub fn header(&self) -> &ArenaHeader {
        unsafe { &*(self.ptr as *const ArenaHeader) }
    }

    /// Returns a raw pointer to slot at the given index.
    ///
    /// # Safety
    /// index must be < SLOT_COUNT.
    pub unsafe fn slot_ptr(&self, index: usize) -> *mut Slot {
        let base = self.ptr.add(PAGE_SIZE);
        (base as *mut Slot).add(index)
    }

    /// Returns a reference to slot at the given index.
    ///
    /// # Safety
    /// index must be < SLOT_COUNT.
    pub unsafe fn slot(&self, index: usize) -> &Slot {
        &*self.slot_ptr(index)
    }

    /// Returns a mutable reference to slot at the given index.
    ///
    /// # Safety
    /// index must be < SLOT_COUNT and caller must hold logical ownership.
    pub unsafe fn slot_mut(&self, index: usize) -> &mut Slot {
        &mut *self.slot_ptr(index)
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.size);
            libc::close(self.fd);
        }

        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Memory::{UnmapViewOfFile, MEMORY_MAPPED_VIEW_ADDRESS};
            let addr = MEMORY_MAPPED_VIEW_ADDRESS { Value: self.ptr as *mut _ };
            UnmapViewOfFile(addr);
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// Create a new shared memory arena (called by the first process).
///
/// Opens the named shm object, sizes it to ARENA_SIZE, maps it, writes
/// the header, and initialises all slots to STATE_EMPTY.
///
/// # Errors
/// Returns an error string if any OS call fails.
#[cfg(unix)]
pub fn create_arena() -> Result<Arena, String> {
    use libc::{
        ftruncate, mmap, shm_open, MAP_SHARED, PROT_READ, PROT_WRITE,
        O_CREAT, O_RDWR, S_IRUSR, S_IWUSR, MAP_FAILED,
    };
    use std::ffi::CString;

    let name = CString::new(SHM_NAME).unwrap();

    let fd = unsafe {
        shm_open(
            name.as_ptr(),
            O_CREAT | O_RDWR,
            (S_IRUSR | S_IWUSR) as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(format!("shm_open failed: errno={}", unsafe { *libc::__errno_location() }));
    }

    if unsafe { ftruncate(fd, ARENA_SIZE as libc::off_t) } < 0 {
        return Err(format!("ftruncate failed"));
    }

    let ptr = unsafe {
        mmap(
            std::ptr::null_mut(),
            ARENA_SIZE,
            PROT_READ | PROT_WRITE,
            MAP_SHARED,
            fd,
            0,
        )
    };

    if ptr == MAP_FAILED {
        return Err(format!("mmap failed"));
    }

    let ptr = ptr as *mut u8;

    // Initialise header
    let header = unsafe { &*(ptr as *mut ArenaHeader) };
    header.magic.store(ARENA_MAGIC, Ordering::Release);
    header.version.store(ARENA_VERSION, Ordering::Release);
    header.slot_count.store(SLOT_COUNT as u32, Ordering::Release);
    header.used_count.store(0, Ordering::Release);

    // Initialise all slots to empty
    for i in 0..SLOT_COUNT {
        let slot_ptr = unsafe { (ptr.add(PAGE_SIZE) as *mut Slot).add(i) };
        unsafe { Slot::init(slot_ptr) };
    }

    Ok(Arena { ptr, size: ARENA_SIZE, fd })
}

/// Open an existing shared memory arena (called by subsequent processes).
///
/// # Errors
/// Returns an error string if the region doesn't exist or header is invalid.
#[cfg(unix)]
pub fn open_arena() -> Result<Arena, String> {
    use libc::{mmap, shm_open, MAP_SHARED, PROT_READ, PROT_WRITE, O_RDWR, MAP_FAILED};
    use std::ffi::CString;

    let name = CString::new(SHM_NAME).unwrap();

    let fd = unsafe { shm_open(name.as_ptr(), O_RDWR, 0) };
    if fd < 0 {
        return Err(format!("shm_open (open) failed — has create_arena() been called?"));
    }

    let ptr = unsafe {
        mmap(
            std::ptr::null_mut(),
            ARENA_SIZE,
            PROT_READ | PROT_WRITE,
            MAP_SHARED,
            fd,
            0,
        )
    };

    if ptr == MAP_FAILED {
        return Err("mmap failed on open".to_string());
    }

    let ptr = ptr as *mut u8;

    // Validate header
    let header = unsafe { &*(ptr as *const ArenaHeader) };
    let magic = header.magic.load(Ordering::Acquire);
    if magic != ARENA_MAGIC {
        return Err(format!("Invalid arena magic: {:#x}", magic));
    }

    Ok(Arena { ptr, size: ARENA_SIZE, fd })
}

/// Windows implementation stub — maps a named file mapping object.
/// Full Windows implementation uses CreateFileMappingW + MapViewOfFile.
#[cfg(windows)]
pub fn create_arena() -> Result<Arena, String> {
    Err("Windows shared memory support coming in Phase 2 extension. Run under WSL for now.".to_string())
}

#[cfg(windows)]
pub fn open_arena() -> Result<Arena, String> {
    Err("Windows shared memory support coming in Phase 2 extension. Run under WSL for now.".to_string())
}