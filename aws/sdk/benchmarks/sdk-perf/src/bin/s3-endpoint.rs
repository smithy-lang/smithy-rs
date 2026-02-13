/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use sdk_perf::results::Results;
use sdk_perf::s3_endpoint::{
    resolve_s3_accesspoint_endpoint, resolve_s3_outposts_endpoint, resolve_s3express_endpoint,
};
use sdk_perf::test_util::{run_test, TestConfig};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Commit ID for the benchmark run
    #[arg(short, long)]
    commit_id: String,
}

fn main() {
    let args = Args::parse();

    let mut results = Results {
        product_id: "aws-sdk-rust".into(),
        sdk_version: None,
        commit_id: args.commit_id,
        results: Vec::new(),
    };

    let config = TestConfig {
        name: "s3_outposts_endpoint_resolution".into(),
        description: "S3 outposts vanilla test".into(),
        unit: "Microseconds".into(),
        runs: 1000,
    };
    run_test(&config, &mut results, resolve_s3_outposts_endpoint);

    let accesspoint_config = TestConfig {
        name: "s3_accesspoint_endpoint_resolution".into(),
        description: "S3 Access Point endpoint resolution benchmark".into(),
        unit: "Microseconds".into(),
        runs: 1000,
    };
    run_test(
        &accesspoint_config,
        &mut results,
        resolve_s3_accesspoint_endpoint,
    );

    let s3express_config = TestConfig {
        name: "s3express_endpoint_resolution".into(),
        description: "Data Plane with short zone name".into(),
        unit: "Microseconds".into(),
        runs: 1000,
    };
    run_test(&s3express_config, &mut results, resolve_s3express_endpoint);

    let output = serde_json::to_string(&results).unwrap();
    println!("{output:#}");
}
