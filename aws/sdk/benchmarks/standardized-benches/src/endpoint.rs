/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use serde::Serialize;
use std::hint::black_box;
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
