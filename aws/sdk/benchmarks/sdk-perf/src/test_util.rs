/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::results::{Result, Results};
use std::time::SystemTime;

pub struct TestConfig {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub runs: u8,
}

pub fn run_test<F: Fn()>(config: &TestConfig, results: &mut Results, func: F) {
    let mut result = Result {
        name: config.name.clone(),
        description: config.description.clone(),
        publish_to_cloudwatch: Some(true),
        dimensions: None,
        date: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        measurements: Vec::new(),
        unit: config.unit.clone(),
    };

    // warmup the function
    for _i in 0..5 {
        func()
    }

    for _i in 0..config.runs {
        let start = SystemTime::now();
        func();
        let time = start.elapsed().unwrap().as_micros() as f64;
        result.measurements.push(time);
    }

    results.results.push(result);
}
