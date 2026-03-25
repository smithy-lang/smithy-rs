/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use standardized_benches::endpoint::lambda::LambdaEndpointBenchmark;
use standardized_benches::endpoint::{run_benchmark, BenchmarkConfig};

#[tokio::main]
async fn main() {
    let mut results = Vec::new();

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
    for (config, bench) in &benchmarks {
        results.push(run_benchmark(config, || bench.resolve()).await);
    }

    let output = serde_json::json!({
        "product_id": "aws-sdk-rust",
        "results": results,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("output should be serialized")
    );
}
