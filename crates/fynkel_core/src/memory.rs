//! Native memory for the instrumentation (blueprint §15.4, M3-16): the
//! Rust heap through a counting allocator and the process's resident set.
//!
//! [`CountingAllocator`] wraps the system allocator and keeps the live byte
//! count and its peak in two atomics; a final crate (a cdylib, a binary)
//! installs it with `#[global_allocator]` — `fynkel_native` (so the Flutter
//! host's dylib counts), the standalone host and the CLI (so `fynkel test
//! --headless` counts). A crate that does not install it reads
//! [`heap_bytes`] as 0: the counter is bumped only by the installed
//! allocator. Each bump is one relaxed atomic add — nothing a frame notices.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

static ALLOCATED: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// The system allocator with a live byte count.
pub struct CountingAllocator;

// SAFETY: forwards every call to `System` unchanged; the counters are a
// side effect that never touches the returned memory.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            add(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            add(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        ALLOCATED.fetch_sub(layout.size() as u64, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            ALLOCATED.fetch_sub(layout.size() as u64, Ordering::Relaxed);
            add(new_size);
        }
        p
    }
}

fn add(size: usize) {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    let now = ALLOCATED.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
    // A stale peak is fine (Relaxed): it is a statistic, not a bound.
    if now > PEAK.load(Ordering::Relaxed) {
        PEAK.store(now, Ordering::Relaxed);
    }
}

/// Bytes the Rust heap holds right now, 0 when no [`CountingAllocator`]
/// is installed in this binary.
pub fn heap_bytes() -> u64 {
    ALLOCATED.load(Ordering::Relaxed)
}

/// The largest [`heap_bytes`] seen since the process started.
pub fn heap_peak_bytes() -> u64 {
    PEAK.load(Ordering::Relaxed)
}

/// Allocations (and reallocations) since the process started — the leak
/// and allocation tests count per cycle from it.
pub fn allocation_count() -> u64 {
    ALLOCATIONS.load(Ordering::Relaxed)
}

/// The process's resident set in bytes (Mach `task_info` on macOS and
/// iOS; 0 elsewhere). Everything the process touched — the Dart heap and
/// the GPU driver's mappings included on the Flutter host — which is why
/// the heap count exists next to it.
pub fn resident_bytes() -> u64 {
    #[cfg(target_vendor = "apple")]
    {
        apple::resident_bytes()
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        0
    }
}

#[cfg(target_vendor = "apple")]
mod apple {
    //! `task_info(mach_task_self(), MACH_TASK_BASIC_INFO)` declared by
    //! hand: the one libc call this needs is not worth a dependency.
    use std::ffi::c_int;

    #[repr(C)]
    struct TimeValue {
        seconds: c_int,
        microseconds: c_int,
    }

    /// `mach_task_basic_info_data_t`.
    #[repr(C)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: TimeValue,
        system_time: TimeValue,
        policy: c_int,
        suspend_count: c_int,
    }

    const MACH_TASK_BASIC_INFO: c_int = 20;
    const MACH_TASK_BASIC_INFO_COUNT: u32 =
        (std::mem::size_of::<MachTaskBasicInfo>() / std::mem::size_of::<u32>()) as u32;

    extern "C" {
        fn mach_task_self() -> u32;
        fn task_info(
            target: u32,
            flavor: c_int,
            info: *mut MachTaskBasicInfo,
            count: *mut u32,
        ) -> c_int;
    }

    pub fn resident_bytes() -> u64 {
        let mut info = std::mem::MaybeUninit::<MachTaskBasicInfo>::uninit();
        let mut count = MACH_TASK_BASIC_INFO_COUNT;
        // SAFETY: the buffer is the size the flavour writes and `count`
        // tells the kernel so; the task port is our own.
        let rc = unsafe {
            task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                info.as_mut_ptr(),
                &mut count,
            )
        };
        if rc != 0 {
            return 0;
        }
        // SAFETY: a zero return means the kernel filled the struct.
        unsafe { info.assume_init() }.resident_size
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resident_set_is_reported_on_apple() {
        let rss = super::resident_bytes();
        if cfg!(target_vendor = "apple") {
            assert!(rss > 1 << 20, "resident set {rss} bytes");
        } else {
            assert_eq!(rss, 0);
        }
    }
}
