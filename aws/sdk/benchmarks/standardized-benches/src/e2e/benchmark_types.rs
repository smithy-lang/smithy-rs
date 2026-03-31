/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkConfig<T> {
    pub version: u32,
    pub name: String,
    pub description: String,
    pub service: String,
    pub action: String,
    pub action_config: T,
    pub batch: BatchConfig,
    pub warmup: WarmupConfig,
    pub measurement: MeasurementConfig,
    /// Timeout in seconds for waiter operations (e.g. wait_until_bucket_exists).
    /// Defaults to 60 if not specified.
    #[serde(default = "default_waiter_timeout_secs")]
    pub waiter_timeout_secs: u64,
}

fn default_waiter_timeout_secs() -> u64 {
    60
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchConfig {
    pub description: String,
    pub number_of_actions: usize,
    pub sequential_execution: bool,
    pub concurrency: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmupConfig {
    pub batches: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementConfig {
    pub batches: usize,
    pub collect_metrics: bool,
    pub metrics_interval: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStats {
    pub mean: f64,
    pub max: f64,
}

impl From<crate::bench_utils::BasicStats> for ResourceStats {
    fn from(s: crate::bench_utils::BasicStats) -> Self {
        Self {
            mean: s.mean,
            max: s.max,
        }
    }
}
