/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::{env, fs::OpenOptions, io::Write, path::Path, time::Duration};

use tokio::time;
use wrk_api_bench::{BenchmarkBuilder, HistoryPeriod, WrkBuilder};

use crate::helpers::PokemonService;

mod helpers;

#[tokio::test]
async fn banchmark() -> Result<(), Box<dyn std::error::Error>> {
    // Benchmarks are expensive, so they run only if the environment
    // variable RUN_BENCHMARKS is present.
    if env::var_os("RUN_BENCHMARKS").is_some() {
        let _program = PokemonService::run();
        // Give PokemonSérvice some time to start up.
        time::sleep(Duration::from_millis(50)).await;

        // The history directory is cached inside GitHub actions under
        // the running use home directory to allow us to recover historical
        // data between runs.
        let history_dir = if env::var_os("GITHUB_ACTIONS").is_some() {
            home::home_dir().unwrap().join(".wrk-api-bench")
        } else {
            Path::new(".").join(".wrk-api-bench")
        };
        let variance_file = history_dir.join("smithy_rs_benchmark_variance.txt");

        let mut wrk = WrkBuilder::default()
            .url(String::from("http://localhost:13734/pokemon-species/pikachu"))
            .history_dir(history_dir)
            .build()?;

        // Run a single benchmark with 8 threads and 64 connections for 60 seconds.
        let benches = vec![BenchmarkBuilder::default()
            .duration(Duration::from_secs(60))
            .threads(8)
            .connections(64)
            .build()?];
        wrk.bench(&benches)?;

        // Calculate variance from last run and write it to disk.
        let variance = wrk.variance(HistoryPeriod::Last)?;
        let mut variance_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(variance_file)
            .unwrap();
        variance_file.write_all(variance.to_github_markdown().as_bytes())?;
    }
    Ok(())
}
