use std::alloc::{GlobalAlloc, Layout, System};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

struct CountingAllocator;

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static DEALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
static CURRENT_LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static BASELINE_LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let size = layout.size();
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
            let live = CURRENT_LIVE_BYTES.fetch_add(size, Ordering::Relaxed) + size;
            record_peak(live);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        CURRENT_LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe {
            System.dealloc(ptr, layout);
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, old_layout, new_size) };
        if !new_ptr.is_null() {
            let old_size = old_layout.size();
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
            if new_size >= old_size {
                let live = CURRENT_LIVE_BYTES.fetch_add(new_size - old_size, Ordering::Relaxed)
                    + new_size
                    - old_size;
                record_peak(live);
            } else {
                CURRENT_LIVE_BYTES.fetch_sub(old_size - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

fn record_peak(live: usize) {
    let mut peak = PEAK_LIVE_BYTES.load(Ordering::Relaxed);
    while live > peak {
        match PEAK_LIVE_BYTES.compare_exchange_weak(
            peak,
            live,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(next_peak) => peak = next_peak,
        }
    }
}

#[derive(Debug)]
struct AllocationSnapshot {
    alloc_count: u64,
    dealloc_count: u64,
    allocated_bytes: u64,
    peak_live_bytes: usize,
}

fn reset_counts() {
    let live = CURRENT_LIVE_BYTES.load(Ordering::Relaxed);
    BASELINE_LIVE_BYTES.store(live, Ordering::Relaxed);
    PEAK_LIVE_BYTES.store(live, Ordering::Relaxed);
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    DEALLOC_COUNT.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
}

fn snapshot() -> AllocationSnapshot {
    let baseline = BASELINE_LIVE_BYTES.load(Ordering::Relaxed);
    let peak = PEAK_LIVE_BYTES.load(Ordering::Relaxed);
    AllocationSnapshot {
        alloc_count: ALLOC_COUNT.load(Ordering::Relaxed),
        dealloc_count: DEALLOC_COUNT.load(Ordering::Relaxed),
        allocated_bytes: ALLOCATED_BYTES.load(Ordering::Relaxed),
        peak_live_bytes: peak.saturating_sub(baseline),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    println!("case,alloc_count,dealloc_count,allocated_bytes,peak_live_bytes");
    for case in pokemon_service_benchmarks::CASES {
        let runner = case.runner();
        runner.run().await;
        thread::sleep(Duration::from_millis(10));

        reset_counts();
        runner.run().await;
        thread::sleep(Duration::from_millis(10));
        let snapshot = snapshot();

        println!(
            "{},{},{},{},{}",
            runner.name,
            snapshot.alloc_count,
            snapshot.dealloc_count,
            snapshot.allocated_bytes,
            snapshot.peak_live_bytes
        );
    }
    ExitCode::SUCCESS
}
