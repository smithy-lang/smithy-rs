/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use serde::Serialize;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

pub mod lambda;
pub mod s3;

pub struct BenchmarkConfig {
    pub id: &'static str,
    pub description: &'static str,
    pub runs: usize,
}

#[derive(Serialize)]
pub struct BenchmarkResult {
    pub id: &'static str,
    pub description: &'static str,
    pub n: usize,
    pub outliers_removed: usize,
    pub mean_ns: f64,
    pub median_ns: f64,
    pub std_dev_ns: f64,
    pub p90_ns: u64,
    pub p99_ns: u64,
}

#[derive(Serialize)]
pub struct ConcurrentBenchmarkResult {
    pub id: &'static str,
    pub description: &'static str,
    pub threads: usize,
    pub runs_per_thread: usize,
    pub total_ops: usize,
    pub wall_clock_ms: f64,
    pub throughput_ops_per_sec: f64,
    pub per_thread: Vec<BenchmarkResult>,
}

pub async fn run_benchmark<F, Fut>(config: &BenchmarkConfig, f: F) -> BenchmarkResult
where
    F: Fn() -> Fut,
    Fut: std::future::Future,
{
    use crate::bench_utils::{basic_stats, filter_outliers, median, percentile};

    for _ in 0..5 {
        black_box(f().await);
    }

    let mut timings = Vec::with_capacity(config.runs);
    for _ in 0..config.runs {
        let start = Instant::now();
        black_box(f().await);
        timings.push(start.elapsed().as_nanos() as u64);
    }

    timings.sort_unstable();
    let raw_n = timings.len();
    let p90 = percentile(&timings, 0.9);
    let p99 = percentile(&timings, 0.99);

    let timings_f64: Vec<f64> = timings.iter().map(|&x| x as f64).collect();
    let filtered = filter_outliers(&timings_f64);
    let n = filtered.len();
    let stats = basic_stats(&filtered);

    BenchmarkResult {
        id: config.id,
        description: config.description,
        n,
        outliers_removed: raw_n - n,
        mean_ns: stats.mean,
        median_ns: median(&filtered),
        std_dev_ns: stats.std_dev,
        p90_ns: p90,
        p99_ns: p99,
    }
}

/// Run a benchmark concurrently across `num_threads` threads, all sharing the
/// same closure via `Arc`. Each thread runs `runs_per_thread` iterations.
/// Returns per-thread latency stats plus aggregate throughput.
pub async fn run_concurrent_benchmark<F, Fut>(
    id: &'static str,
    description: &'static str,
    num_threads: usize,
    runs_per_thread: usize,
    f: Arc<F>,
) -> ConcurrentBenchmarkResult
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    use crate::bench_utils::{basic_stats, filter_outliers, median, percentile};

    // Warmup
    for _ in 0..5 {
        black_box(f().await);
    }

    let barrier = Arc::new(tokio::sync::Barrier::new(num_threads));
    let wall_start = Instant::now();

    let mut handles = Vec::with_capacity(num_threads);
    for thread_idx in 0..num_threads {
        let f = Arc::clone(&f);
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            // All threads start together
            barrier.wait().await;

            let mut timings = Vec::with_capacity(runs_per_thread);
            for _ in 0..runs_per_thread {
                let start = Instant::now();
                black_box(f().await);
                timings.push(start.elapsed().as_nanos() as u64);
            }

            timings.sort_unstable();
            let raw_n = timings.len();
            let p90 = percentile(&timings, 0.9);
            let p99 = percentile(&timings, 0.99);

            let timings_f64: Vec<f64> = timings.iter().map(|&x| x as f64).collect();
            let filtered = filter_outliers(&timings_f64);
            let n = filtered.len();
            let stats = basic_stats(&filtered);

            // Leak a thread-specific id string so we get &'static str
            let thread_id: &'static str =
                Box::leak(format!("{id}_thread_{thread_idx}").into_boxed_str());

            BenchmarkResult {
                id: thread_id,
                description,
                n,
                outliers_removed: raw_n - n,
                mean_ns: stats.mean,
                median_ns: median(&filtered),
                std_dev_ns: stats.std_dev,
                p90_ns: p90,
                p99_ns: p99,
            }
        }));
    }

    let mut per_thread = Vec::with_capacity(num_threads);
    for handle in handles {
        per_thread.push(handle.await.expect("thread panicked"));
    }

    let wall_elapsed = wall_start.elapsed();
    let total_ops = num_threads * runs_per_thread;
    let wall_clock_ms = wall_elapsed.as_secs_f64() * 1000.0;
    let throughput_ops_per_sec = total_ops as f64 / wall_elapsed.as_secs_f64();

    ConcurrentBenchmarkResult {
        id,
        description,
        threads: num_threads,
        runs_per_thread,
        total_ops,
        wall_clock_ms,
        throughput_ops_per_sec,
        per_thread,
    }
}
