/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use sdk_perf::ddb_benchmark::{run_benchmark, BenchmarkConfig};
use std::fs;

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    config: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let config_str = fs::read_to_string(&args.config).expect("Failed to read config file");
    let config: BenchmarkConfig =
        serde_json::from_str(&config_str).expect("Failed to parse config");

    let results = run_benchmark(config).await;
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}
