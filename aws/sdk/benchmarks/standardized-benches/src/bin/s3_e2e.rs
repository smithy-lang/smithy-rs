/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use standardized_benches::e2e::benchmark_types::BenchmarkConfig;
use standardized_benches::e2e::s3::{run_benchmark, ActionConfig};
use std::fs;

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    config_path: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let config_str = fs::read_to_string(&args.config_path).expect("config file should be readable");
    let config: BenchmarkConfig<ActionConfig> =
        serde_json::from_str(&config_str).expect("config file should be valid JSON");
    let results = run_benchmark(config).await;
    println!(
        "{}",
        serde_json::to_string_pretty(&results).expect("results should be rendered")
    );
}
