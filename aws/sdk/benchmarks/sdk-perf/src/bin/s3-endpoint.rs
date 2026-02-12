/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::config::endpoint::{DefaultResolver, Params, ResolveEndpoint};
use clap::Parser;
use sdk_perf::results::Results;
use sdk_perf::test_util::{run_test, TestConfig};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Commit ID for the benchmark run
    #[arg(short, long)]
    commit_id: String,
}

fn resolve_s3_outposts_endpoint() {
    let resolver = DefaultResolver::new();

    let params = Params::builder()
        .set_region(Some("us-west-2".to_owned()))
        .set_bucket(Some(
            "arn:aws:s3-outposts:us-west-2:123456789012:outpost/op-01234567890123456/accesspoint/reports".to_owned()
        ))
        .set_key(Some("key".to_owned()))
        .build()
        .expect("valid params");

    let _ = resolver.resolve_endpoint(&params);
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
        description: "S3 Outposts endpoint resolution benchmark".into(),
        unit: "Microseconds".into(),
        runs: 10,
    };

    run_test(&config, &mut results, || {
        for _ in 0..1000 {
            resolve_s3_outposts_endpoint();
        }
    });

    let output = serde_json::to_string(&results).unwrap();
    println!("{output:#}");
}
