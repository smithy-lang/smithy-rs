/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use serde::Serialize;
use std::hint::black_box;
use std::time::Instant;

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

fn median(sorted: &[u64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) as f64 / 2.0
    } else {
        sorted[n / 2] as f64
    }
}

/// Filter outliers using IQR method (1.5 × IQR fence).
fn filter_outliers(sorted: &[u64]) -> Vec<u64> {
    let n = sorted.len();
    let q1 = sorted[n / 4] as f64;
    let q3 = sorted[3 * n / 4] as f64;
    let iqr = q3 - q1;
    let lo = q1 - 1.5 * iqr;
    let hi = q3 + 1.5 * iqr;
    sorted
        .iter()
        .copied()
        .filter(|&x| (x as f64) >= lo && (x as f64) <= hi)
        .collect()
}

pub async fn run_benchmark<F, Fut>(config: &BenchmarkConfig, f: F) -> BenchmarkResult
where
    F: Fn() -> Fut,
    Fut: std::future::Future,
{
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
    let p90 = timings[raw_n * 90 / 100];
    let p99 = timings[raw_n * 99 / 100];

    let filtered = filter_outliers(&timings);
    let n = filtered.len();
    let mean = filtered.iter().map(|&x| x as f64).sum::<f64>() / n as f64;
    let std_dev = (filtered
        .iter()
        .map(|&x| {
            let d = x as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / n as f64)
        .sqrt();

    BenchmarkResult {
        id: config.id,
        description: config.description,
        n,
        outliers_removed: raw_n - n,
        mean_ns: mean,
        median_ns: median(&filtered),
        std_dev_ns: std_dev,
        p90_ns: p90,
        p99_ns: p99,
    }
}
