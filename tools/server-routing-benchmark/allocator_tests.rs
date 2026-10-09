#[cfg(test)]
mod tests {
    use super::allocations::{self, Counting, Measurement};
    use std::alloc::{GlobalAlloc, Layout};
    use std::sync::atomic::Ordering::Relaxed;

    fn reset() {
        assert_eq!(
            allocations::LIVE.load(Relaxed),
            0,
            "previous test leaked bytes"
        );
        allocations::ENABLED.store(false, Relaxed);
        allocations::CALLS.store(0, Relaxed);
        allocations::BYTES.store(0, Relaxed);
        allocations::PEAK.store(0, Relaxed);
    }
    fn layout(size: usize) -> Layout {
        Layout::from_size_align(size, 8).unwrap()
    }
    unsafe fn allocate(size: usize) -> *mut u8 {
        let ptr = unsafe { Counting.alloc(layout(size)) };
        assert!(!ptr.is_null());
        ptr
    }
    unsafe fn resize(ptr: *mut u8, old: usize, new: usize) -> *mut u8 {
        let ptr = unsafe { Counting.realloc(ptr, layout(old), new) };
        assert!(!ptr.is_null());
        ptr
    }
    unsafe fn free(ptr: *mut u8, size: usize) {
        unsafe { Counting.dealloc(ptr, layout(size)) };
    }
    // Expected: calls, requested bytes, baseline, absolute peak, final, delta, growth.
    fn check(m: Measurement, expected: (u64, u64, i64, i64, i64, i64, i64)) {
        assert_eq!(
            (
                m.allocation_calls,
                m.allocated_bytes,
                m.baseline_live_heap_bytes,
                m.peak_live_heap_bytes,
                m.final_live_heap_bytes,
                m.net_heap_delta_bytes,
                m.peak_heap_growth_bytes
            ),
            expected
        );
    }

    #[test]
    fn disabled_window_tracks_live_bytes_without_counting_calls() {
        reset();
        unsafe {
            let regular = allocate(128);
            let zeroed = Counting.alloc_zeroed(layout(64));
            assert!(!zeroed.is_null());
            assert_eq!(allocations::LIVE.load(Relaxed), 192);
            let regular = resize(regular, 128, 256);
            assert_eq!(allocations::LIVE.load(Relaxed), 320);
            free(regular, 256);
            free(zeroed, 64);
        }
        assert_eq!(allocations::LIVE.load(Relaxed), 0);
        assert_eq!(allocations::CALLS.load(Relaxed), 0);
        assert_eq!(allocations::BYTES.load(Relaxed), 0);
        assert_eq!(allocations::PEAK.load(Relaxed), 0);
    }

    #[test]
    fn warmup_free_preserves_absolute_peak_and_signed_delta() {
        reset();
        unsafe {
            let warmup = allocate(128);
            let baseline = allocations::begin_measurement();
            free(warmup, 128);
            let measured = allocate(64);
            check(
                allocations::finish_measurement(baseline),
                (1, 64, 128, 128, 64, -64, 0),
            );
            free(measured, 64);
        }
    }

    #[test]
    fn measured_allocation_above_retained_warmup_records_peak() {
        reset();
        unsafe {
            let warmup = allocate(128);
            let baseline = allocations::begin_measurement();
            let measured = allocate(64);
            free(measured, 64);
            check(
                allocations::finish_measurement(baseline),
                (1, 64, 128, 192, 128, 0, 64),
            );
            free(warmup, 128);
        }
    }

    #[test]
    fn growing_warmup_allocation_counts_full_new_requested_size() {
        reset();
        unsafe {
            let warmup = allocate(128);
            let baseline = allocations::begin_measurement();
            let grown = resize(warmup, 128, 256);
            check(
                allocations::finish_measurement(baseline),
                (1, 256, 128, 256, 256, 128, 128),
            );
            free(grown, 256);
        }
    }

    #[test]
    fn shrinking_warmup_allocation_retains_baseline_peak() {
        reset();
        unsafe {
            let warmup = allocate(128);
            let baseline = allocations::begin_measurement();
            let shrunk = resize(warmup, 128, 64);
            check(
                allocations::finish_measurement(baseline),
                (1, 64, 128, 128, 64, -64, 0),
            );
            free(shrunk, 64);
        }
    }

    #[test]
    fn zeroed_allocation_is_counted_and_remains_zeroed() {
        reset();
        let baseline = allocations::begin_measurement();
        unsafe {
            let ptr = Counting.alloc_zeroed(layout(64));
            assert!(!ptr.is_null());
            assert_eq!(std::slice::from_raw_parts(ptr, 64), &[0u8; 64]);
            check(
                allocations::finish_measurement(baseline),
                (1, 64, 0, 64, 64, 64, 64),
            );
            free(ptr, 64);
        }
    }

    #[test]
    fn repeated_windows_reset_counts_and_capture_new_baseline() {
        reset();
        unsafe {
            let baseline = allocations::begin_measurement();
            let first = allocate(64);
            check(
                allocations::finish_measurement(baseline),
                (1, 64, 0, 64, 64, 64, 64),
            );
            let between = allocate(128);
            let baseline = allocations::begin_measurement();
            let second = allocate(32);
            check(
                allocations::finish_measurement(baseline),
                (1, 32, 192, 224, 224, 32, 32),
            );
            free(first, 64);
            free(between, 128);
            free(second, 32);
        }
    }

    #[test]
    fn finished_snapshot_excludes_later_allocations() {
        reset();
        unsafe {
            let baseline = allocations::begin_measurement();
            let measured = allocate(64);
            let result = allocations::finish_measurement(baseline);
            let after = allocate(512);
            assert_eq!(allocations::LIVE.load(Relaxed), 576);
            check(result, (1, 64, 0, 64, 64, 64, 64));
            assert_eq!(allocations::CALLS.load(Relaxed), 1);
            assert_eq!(allocations::BYTES.load(Relaxed), 64);
            assert_eq!(allocations::PEAK.load(Relaxed), 64);
            free(after, 512);
            free(measured, 64);
        }
    }

    #[test]
    fn freeing_only_warmup_bytes_reports_zero_calls_and_negative_delta() {
        reset();
        unsafe {
            let warmup = allocate(128);
            let baseline = allocations::begin_measurement();
            free(warmup, 128);
            check(
                allocations::finish_measurement(baseline),
                (0, 0, 128, 128, 0, -128, 0),
            );
        }
    }
}
