// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use serde_json::{json, Value};
#[cfg(feature = "allocations")]
use std::alloc::{GlobalAlloc, Layout, System};
#[cfg(feature = "allocations")]
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::runtime::{Builder, Runtime};

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

pub fn runtime_builder(workers: usize) -> Builder {
    let mut builder = Builder::new_multi_thread();
    builder.worker_threads(workers).enable_all();
    #[cfg(feature = "runtime-diagnostics")]
    {
        use tokio::runtime::{HistogramConfiguration, LogHistogram};
        // Log buckets retain useful resolution across microsecond polls and
        // multi-second scheduler stalls. Persist their actual ranges in JSON.
        builder
            .enable_metrics_poll_time_histogram()
            .metrics_poll_time_histogram_configuration(HistogramConfiguration::log(
                LogHistogram::builder().max_value(std::time::Duration::from_secs(60)),
            ))
            .enable_metrics_schedule_latency_histogram()
            .metrics_schedule_latency_histogram_configuration(HistogramConfiguration::log(
                LogHistogram::builder().max_value(std::time::Duration::from_secs(60)),
            ));
    }
    builder
}

#[cfg_attr(feature = "dial9", allow(dead_code))]
pub fn runtime(workers: usize) -> Runtime {
    runtime_builder(workers).build().unwrap()
}

// Use an OS thread so a blocked Tokio worker cannot also block observation.
// Only diagnostic binaries enable this sampler and Tokio's histogram clocks.
#[cfg(feature = "runtime-diagnostics")]
pub struct RuntimeMonitor {
    before: Value,
    stop: std::sync::mpsc::Sender<()>,
    thread: std::thread::JoinHandle<(Value, Vec<Value>, f64)>,
}

#[cfg(feature = "runtime-diagnostics")]
fn runtime_counters(metrics: &tokio::runtime::RuntimeMetrics) -> Value {
    let histogram = |schedule: bool| {
        let buckets = if schedule {
            metrics.schedule_latency_histogram_num_buckets()
        } else {
            metrics.poll_time_histogram_num_buckets()
        };
        (0..buckets)
            .map(|bucket| {
                let range = if schedule {
                    metrics.schedule_latency_histogram_bucket_range(bucket)
                } else {
                    metrics.poll_time_histogram_bucket_range(bucket)
                };
                let counts: Vec<_> = (0..metrics.num_workers())
                    .map(|worker| {
                        if schedule {
                            metrics.schedule_latency_histogram_bucket_count(worker, bucket)
                        } else {
                            metrics.poll_time_histogram_bucket_count(worker, bucket)
                        }
                    })
                    .collect();
                json!({"start_ns": range.start.as_nanos() as u64,
                       "end_ns": range.end.as_nanos() as u64, "worker_counts": counts})
            })
            .collect::<Vec<_>>()
    };
    let workers: Vec<_> = (0..metrics.num_workers())
        .map(|worker| {
            json!({
                "busy_ns": metrics.worker_total_busy_duration(worker).as_nanos() as u64,
                "polls": metrics.worker_poll_count(worker),
                "parks": metrics.worker_park_count(worker),
                "local_schedules": metrics.worker_local_schedule_count(worker),
                "queue_overflows": metrics.worker_overflow_count(worker),
                "stolen_tasks": metrics.worker_steal_count(worker),
            })
        })
        .collect();
    json!({"workers": workers, "poll_time": histogram(false),
           "schedule_latency": histogram(true)})
}

#[cfg(feature = "runtime-diagnostics")]
fn runtime_gauges(metrics: &tokio::runtime::RuntimeMetrics, elapsed: f64) -> Value {
    let workers: Vec<_> = (0..metrics.num_workers())
        .map(|worker| {
            json!({"local_queue_depth": metrics.worker_local_queue_depth(worker),
                   "mean_poll_ns_ewma": metrics.worker_mean_poll_time(worker).as_nanos() as u64})
        })
        .collect();
    json!({"elapsed_seconds": elapsed,
           "alive_tasks": metrics.num_alive_tasks(),
           "global_queue_depth": metrics.global_queue_depth(),
           "blocking_queue_depth": metrics.blocking_queue_depth(),
           "blocking_threads": metrics.num_blocking_threads(), "workers": workers})
}

#[cfg(feature = "runtime-diagnostics")]
impl RuntimeMonitor {
    pub fn start(metrics: tokio::runtime::RuntimeMetrics) -> Self {
        let origin = std::time::Instant::now();
        let before = runtime_counters(&metrics);
        let initial = runtime_gauges(&metrics, origin.elapsed().as_secs_f64());
        let (stop, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut samples = vec![initial];
            while receiver
                .recv_timeout(std::time::Duration::from_millis(100))
                .is_err_and(|error| error == std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                samples.push(runtime_gauges(&metrics, origin.elapsed().as_secs_f64()));
            }
            let after = runtime_counters(&metrics);
            let elapsed = origin.elapsed().as_secs_f64();
            samples.push(runtime_gauges(&metrics, elapsed));
            (after, samples, elapsed)
        });
        Self {
            before,
            stop,
            thread,
        }
    }

    pub fn finish(self) -> Value {
        self.stop.send(()).unwrap();
        let (after, samples, elapsed) = self.thread.join().unwrap();
        json!({"before": self.before, "after": after, "samples": samples,
               "elapsed_seconds": elapsed, "sample_interval_seconds": 0.1})
    }
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

#[cfg(all(test, feature = "runtime-diagnostics"))]
mod runtime_tests {
    #[test]
    fn blocked_worker_exposes_backlog_and_schedule_latency() {
        use super::*;
        use std::{sync::mpsc, time::Duration};
        let runtime = runtime(1);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        // Deliberately block a worker to reproduce scheduler starvation.
        // Synchronization ensures queued tasks cannot run before observation.
        let blocker = runtime.spawn(async move {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let queued: Vec<_> = (0..32).map(|_| runtime.spawn(async {})).collect();
        let monitor = RuntimeMonitor::start(runtime.metrics());
        std::thread::sleep(Duration::from_millis(20));
        release_tx.send(()).unwrap();
        runtime.block_on(async {
            blocker.await.unwrap();
            for task in queued {
                task.await.unwrap();
            }
        });
        // Tokio publishes worker counters in batches, including when parking.
        std::thread::sleep(Duration::from_millis(20));
        let result = monitor.finish();
        assert!(result["samples"][0]["global_queue_depth"].as_u64().unwrap() >= 32);
        assert!(result["samples"][0]["alive_tasks"].as_u64().unwrap() >= 33);
        assert!(
            result["after"]["workers"][0]["busy_ns"].as_u64().unwrap()
                > result["before"]["workers"][0]["busy_ns"].as_u64().unwrap()
        );
        for histogram in ["poll_time", "schedule_latency"] {
            let total = |snapshot: &Value| {
                snapshot[histogram]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|b| b["worker_counts"][0].as_u64().unwrap())
                    .sum::<u64>()
            };
            assert!(total(&result["after"]) > total(&result["before"]));
        }
        // Queued tasks spent at least 20 ms waiting; retain a distribution
        // above 1 ms rather than only asserting that instrumentation exists.
        let long_waits: u64 = result["after"]["schedule_latency"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["start_ns"].as_u64().unwrap() >= 1_000_000)
            .map(|b| b["worker_counts"][0].as_u64().unwrap())
            .sum();
        assert!(long_waits >= 32);
    }
}
