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

pub fn calculate_resource_stats(samples: &[f64]) -> ResourceStats {
    if samples.is_empty() {
        return ResourceStats {
            mean: 0.0,
            max: 0.0,
        };
    }
    ResourceStats {
        mean: samples.iter().sum::<f64>() / samples.len() as f64,
        max: samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    }
}

pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx]
}
