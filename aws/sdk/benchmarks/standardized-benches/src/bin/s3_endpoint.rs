/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use standardized_benches::endpoint::s3::S3EndpointBenchmark;
use standardized_benches::endpoint::{run_benchmark, BenchmarkConfig};

#[tokio::main]
async fn main() {
    let mut results = Vec::new();

    let benchmarks: Vec<(BenchmarkConfig, S3EndpointBenchmark)> = vec![
        (
            BenchmarkConfig {
                id: "s3_outposts_endpoint_resolution",
                description: "S3 outposts vanilla test",
                runs: 10000,
            },
            S3EndpointBenchmark::outposts(),
        ),
        (
            BenchmarkConfig {
                id: "s3_accesspoint_endpoint_resolution",
                description: "S3 Access Point endpoint resolution benchmark",
                runs: 10000,
            },
            S3EndpointBenchmark::accesspoint(),
        ),
        (
            BenchmarkConfig {
                id: "s3express_endpoint_resolution",
                description: "Data Plane with short zone name",
                runs: 10000,
            },
            S3EndpointBenchmark::express(),
        ),
        (
            BenchmarkConfig {
                id: "s3_path_style_endpoint_resolution",
                description: "vanilla path style@us-west-2",
                runs: 10000,
            },
            S3EndpointBenchmark::path_style(),
        ),
        (
            BenchmarkConfig {
                id: "s3_virtual_addressing_endpoint_resolution",
                description: "vanilla virtual addressing@us-west-2",
                runs: 10000,
            },
            S3EndpointBenchmark::virtual_addressing(),
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
