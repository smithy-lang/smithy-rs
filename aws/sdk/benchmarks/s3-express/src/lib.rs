/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

const DEFAULT_CONFIDENCE_LEVEL: f64 = 0.99;
const DEFAULT_NUMBER_OF_ITERATIONS: usize = 20;
const DEFAULT_SAMPLE_SIZE: usize = 10;

/// Configures the confidence level for benchmarks
pub fn confidence_level() -> f64 {
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

/// Configures the sample size for benchmarks
pub fn sample_size() -> usize {
    let sample_size =
        std::env::var("SAMPLE_SIZE").map_or(DEFAULT_SAMPLE_SIZE, |s| s.parse::<usize>().unwrap());
    dbg!(sample_size)
}
