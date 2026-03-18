/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use sdk_perf::s3_endpoint::S3EndpointBenchmark;
use std::hint::black_box;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Commit ID for the benchmark run
    #[arg(short, long)]
    commit_id: String,
}

struct BenchmarkConfig {
    id: &'static str,
    description: &'static str,
    runs: usize,
}

async fn run_benchmark(config: &BenchmarkConfig, bench: &S3EndpointBenchmark) -> serde_json::Value {
    // warmup
    for _ in 0..5 {
        black_box(bench.resolve().await);
    }

    let mut timings = Vec::with_capacity(config.runs);
    loop {
        let start = Instant::now();
        black_box(bench.resolve().await);
        let elapsed = start.elapsed().as_nanos() as u64;
        timings.push(elapsed);
        if timings.len() >= config.runs {
            break;
        }
    }

    let mut sorted = timings.clone();
    sorted.sort_unstable();
    let n = timings.len();
    let mean = timings.iter().sum::<u64>() / n as u64;
    let variance = timings
        .iter()
        .map(|&x| {
            let diff = x as i64 - mean as i64;
            (diff * diff) as u64
        })
        .sum::<u64>()
        / n as u64;
    let std_dev = (variance as f64).sqrt() as u64;

    serde_json::json!({
        "id": config.id,
        "description": config.description,
        "n": n,
        "mean_ns": mean,
        "p50_ns": sorted[n * 50 / 100],
        "p90_ns": sorted[n * 90 / 100],
        "p95_ns": sorted[n * 95 / 100],
        "p99_ns": sorted[n * 99 / 100],
        "std_dev_ns": std_dev,
    })
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let benchmarks: Vec<(BenchmarkConfig, S3EndpointBenchmark)> = vec![
        (
            BenchmarkConfig {
                id: "s3_outposts_endpoint_resolution",
                description: "S3 outposts vanilla test",
                runs: 10000,
            },
            S3EndpointBenchmark::s3_outposts(),
        ),
        (
            BenchmarkConfig {
                id: "s3_accesspoint_endpoint_resolution",
                description: "S3 Access Point endpoint resolution benchmark",
                runs: 10000,
            },
            S3EndpointBenchmark::s3_accesspoint(),
        ),
        (
            BenchmarkConfig {
                id: "s3express_endpoint_resolution",
                description: "Data Plane with short zone name",
                runs: 10000,
            },
            S3EndpointBenchmark::s3express(),
        ),
        (
            BenchmarkConfig {
                id: "s3_path_style_endpoint_resolution",
                description: "vanilla path style@us-west-2",
                runs: 10000,
            },
            S3EndpointBenchmark::s3_path_style(),
        ),
        (
            BenchmarkConfig {
                id: "s3_virtual_addressing_endpoint_resolution",
                description: "vanilla virtual addressing@us-west-2",
                runs: 10000,
            },
            S3EndpointBenchmark::s3_virtual_addressing(),
        ),
    ];

    let mut results = Vec::new();
    for (config, bench) in &benchmarks {
        results.push(run_benchmark(config, bench).await);
    }

    let output = serde_json::json!({
        "product_id": "aws-sdk-rust",
        "commit_id": args.commit_id,
        "results": results,
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
