/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use standardized_benches::endpoint::run_concurrent_benchmark;
use standardized_benches::endpoint::s3::S3EndpointBenchmark;
use std::sync::Arc;

const THREAD_COUNTS: &[usize] = &[1, 2, 4, 8, 16];
const RUNS_PER_THREAD: usize = 10_000;

#[tokio::main]
async fn main() {
    let scenarios: Vec<(&str, &str, fn() -> S3EndpointBenchmark)> = vec![
        (
            "s3_virtual_addressing",
            "virtual addressing@us-west-2 (cache hit — same params)",
            S3EndpointBenchmark::virtual_addressing,
        ),
        (
            "s3_path_style",
            "path style@us-west-2 (cache hit — same params)",
            S3EndpointBenchmark::path_style,
        ),
        (
            "s3_outposts",
            "outposts (cache hit — same params)",
            S3EndpointBenchmark::outposts,
        ),
    ];

    let mut results = Vec::new();

    for (scenario_id, description, make_bench) in &scenarios {
        for &num_threads in THREAD_COUNTS {
            let bench = Arc::new(make_bench());
            let id: &'static str =
                Box::leak(format!("{scenario_id}_concurrent_{num_threads}t").into_boxed_str());
            let desc: &'static str =
                Box::leak(format!("{description} [{num_threads} threads]").into_boxed_str());

            let b = Arc::clone(&bench);
            let result = run_concurrent_benchmark(
                id,
                desc,
                num_threads,
                RUNS_PER_THREAD,
                Arc::new(move || {
                    let b = Arc::clone(&b);
                    async move { b.resolve().await }
                }),
            )
            .await;

            results.push(result);
        }
    }

    let output = serde_json::json!({
        "product_id": "aws-sdk-rust",
        "benchmark_type": "concurrent_endpoint_resolution",
        "results": results,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("output should be serialized")
    );
}
