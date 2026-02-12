/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use sdk_perf::ddb_serde::{deserialize, serialize};
use sdk_perf::results::Results;
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

    // Note: in the below microseconds should be case insensitive, but due to a
    // bug in the perf test evaluation it is currently not. Can get rid of the
    // capitalization when that is fixed.
    let deserialize_config = TestConfig {
        name: "deserialize.ddb".into(),
        description: "Deserializing a DDB response.".into(),
        unit: "Microseconds".into(),
        runs: 10,
    };

    let serialize_config = TestConfig {
        name: "serialize.ddb".into(),
        description: "Serializing a DDB request.".into(),
        unit: "Microseconds".into(),
        runs: 10,
    };

    run_test(&deserialize_config, &mut results, deserialize);
    run_test(&serialize_config, &mut results, serialize);

    let output = serde_json::to_string(&results).unwrap();
    println!("{output:#}");
}
