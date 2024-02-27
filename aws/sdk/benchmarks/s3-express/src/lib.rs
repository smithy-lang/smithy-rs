/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use criterion::measurement::WallTime;
use criterion::Criterion;
use pprof::criterion::{Output, PProfProfiler};
use std::os::raw::c_int;

const DEFAULT_CONFIDENCE_LEVEL: f64 = 0.99;
const DEFAULT_NUMBER_OF_ITERATIONS: usize = 20;
const DEFAULT_PROF_FREQUENCY: c_int = 10;
const DEFAULT_SAMPLE_SIZE: usize = 10;

/// Configures [`Criterion`] for benchmarks
pub fn criterion_config() -> Criterion<WallTime> {
    match std::env::var("PROF") {
        Ok(prof) if prof == "pprof" => Criterion::default()
            .with_profiler(PProfProfiler::new(
                prof_frequency(),
                Output::Flamegraph(None),
            ))
            .sample_size(sample_size())
            .confidence_level(confidence_level()),
        _ => Criterion::default()
            .sample_size(sample_size())
            .confidence_level(confidence_level()),
    }
}

/// Configures the confidence level for benchmarks
fn confidence_level() -> f64 {
    let confidence_level = std::env::var("CONFIDENCE_LEVEL")
        .map_or(DEFAULT_CONFIDENCE_LEVEL, |s| s.parse::<f64>().unwrap());
    dbg!(confidence_level)
}

/// Configures the number of times operations run for measurement
pub fn number_of_iterations() -> usize {
    let number_of_iterations = std::env::var("NUMBER_OF_ITERATIONS")
        .map_or(DEFAULT_NUMBER_OF_ITERATIONS, |n| {
            n.parse::<usize>().unwrap()
        });
    dbg!(number_of_iterations)
}

/// Configures profiler frequency for flamegraph generation in benchmarks
///
/// If the frequency is too high, criterion may exit with SIGTRAP: trace/breakpoint trap.
/// See https://github.com/tikv/pprof-rs/issues/237
fn prof_frequency() -> c_int {
    let prof_frequency = std::env::var("PROF_FREQUENCY")
        .map_or(DEFAULT_PROF_FREQUENCY, |s| s.parse::<c_int>().unwrap());
    dbg!(prof_frequency)
}

/// Configures the sample size for benchmarks
pub fn sample_size() -> usize {
    let sample_size =
        std::env::var("SAMPLE_SIZE").map_or(DEFAULT_SAMPLE_SIZE, |s| s.parse::<usize>().unwrap());
    dbg!(sample_size)
}
