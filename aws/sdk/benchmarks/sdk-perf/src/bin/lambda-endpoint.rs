/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use sdk_perf::lambda_endpoint::LambdaEndpointBenchmark;
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

fn run_benchmark(config: &BenchmarkConfig, bench: &LambdaEndpointBenchmark) -> serde_json::Value {
    for _ in 0..5 {
        bench.resolve();
    }

    let mut timings = Vec::with_capacity(config.runs);
    loop {
        let start = Instant::now();
        bench.resolve();
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

fn main() {
    let args = Args::parse();

    let benchmarks: Vec<(BenchmarkConfig, LambdaEndpointBenchmark)> = vec![
        (
            BenchmarkConfig {
                id: "lambda_standard_endpoint_resolution",
                description: "Lambda standard endpoint resolution",
                runs: 10000,
            },
            LambdaEndpointBenchmark::standard(),
        ),
        (
            BenchmarkConfig {
                id: "lambda_govcloud_fips_dualstack_endpoint_resolution",
                description: "Lambda GovCloud FIPS dual-stack endpoint resolution",
                runs: 10000,
            },
            LambdaEndpointBenchmark::govcloud_fips_dualstack(),
        ),
    ];

    let mut results = Vec::new();
    for (config, bench) in &benchmarks {
        results.push(run_benchmark(config, bench));
    }

    let output = serde_json::json!({
        "product_id": "aws-sdk-rust",
        "commit_id": args.commit_id,
        "results": results,
    });
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
