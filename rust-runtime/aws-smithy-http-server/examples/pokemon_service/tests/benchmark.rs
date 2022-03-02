/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// Files here are for running integration tests.
// These tests only have access to your crate's public API.
// See: https://doc.rust-lang.org/book/ch11-03-test-organization.html#integration-tests

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use crate::helpers::{client, PokemonService};
use tokio::time;

#[macro_use]
mod helpers;

#[cfg(feature = "benchmarks")]
#[tokio::test]
async fn banchmark() {
    use wrk_api_bench::{BenchmarkBuilder, HistoryPeriod, WrkBuilder};
    let _program = PokemonService::run();
    // Give PokemonSérvice some time to start up.

    time::sleep(Duration::from_millis(50)).await;

    let history_dir = home::home_dir().unwrap().join(".wrk-api-bench");
    let mut wrk = WrkBuilder::default()
        .url(String::from("http://localhost:13734/pokemon-species/pikachu"))
        .history_dir(history_dir)
        .build()
        .unwrap();
    let benches = vec![BenchmarkBuilder::default()
        .duration(Duration::from_secs(5))
        .build()
        .unwrap()];
    wrk.bench(&benches).unwrap();
    let mut variance = wrk.variance(HistoryPeriod::Last).unwrap();
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(Path::new("/tmp/current_variance.txt"))
        .unwrap();
    file.write_all(variance.to_string().as_bytes()).unwrap();
}
