// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use serde_json::{json, Value};
#[cfg(feature = "allocations")]
use std::alloc::{GlobalAlloc, Layout, System};
#[cfg(feature = "allocations")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "allocations")]
static COUNT: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "allocations")]
static BYTES: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "allocations")]
static LIVE: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "allocations")]
static PEAK: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "allocations")]
struct Counting;
#[cfg(feature = "allocations")]
#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[cfg(feature = "allocations")]
fn add(size: usize) {
    COUNT.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(size as u64, Ordering::Relaxed);
    let live = LIVE.fetch_add(size as u64, Ordering::Relaxed) + size as u64;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

#[cfg(feature = "allocations")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            add(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc_zeroed(layout);
        if !p.is_null() {
            add(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        System.dealloc(p, layout);
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let out = System.realloc(p, layout, size);
        if !out.is_null() {
            LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
            add(size);
        }
        out
    }
}

pub fn reset_peak() {
    #[cfg(feature = "allocations")]
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

pub fn snapshot() -> Value {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) }, 0);
    let seconds = |v: libc::timeval| v.tv_sec as f64 + v.tv_usec as f64 / 1_000_000.0;
    let data = json!({
        "user_seconds": seconds(usage.ru_utime),
        "system_seconds": seconds(usage.ru_stime),
        "lifetime_max_rss_kib": usage.ru_maxrss,
    });
    #[cfg(feature = "allocations")]
    let data = {
        let mut data = data;
        data["allocations"] = json!(COUNT.load(Ordering::Relaxed));
        data["allocated_bytes"] = json!(BYTES.load(Ordering::Relaxed));
        data["requested_live_heap_bytes"] = json!(LIVE.load(Ordering::Relaxed));
        data["requested_peak_heap_bytes"] = json!(PEAK.load(Ordering::Relaxed));
        data
    };
    data
}

#[cfg(all(test, feature = "allocations"))]
mod tests {
    #[test]
    fn large_live_allocation_is_visible_and_released() {
        use super::*;
        reset_peak();
        let baseline = LIVE.load(Ordering::Relaxed);
        let allocation = vec![42u8; 1024 * 1024];
        assert_eq!(std::hint::black_box(allocation.as_slice())[17], 42);
        let during = LIVE.load(Ordering::Relaxed);
        assert!(during >= baseline + 1024 * 1024);
        assert!(PEAK.load(Ordering::Relaxed) >= during);
        drop(allocation);
        assert!(LIVE.load(Ordering::Relaxed) <= during - 1024 * 1024);
    }
}
