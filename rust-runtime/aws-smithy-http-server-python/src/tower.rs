/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::time::Duration;

#[pyo3::pyclass]
#[derive(Debug, Clone, Default)]
pub struct TowerLayersConfig {
    pub timeout: Option<TimeoutLayerConfig>,
    pub instrument: Option<InstrumentLayerConfig>,
}

#[pyo3::pyclass]
#[derive(Debug, Clone, Default)]
pub struct InstrumentLayerConfig;

/// [tower_http::timeout::TimeoutLayer] configuration wrapper.
#[pyo3::pyclass]
#[derive(Debug, Clone)]
pub struct TimeoutLayerConfig {
    pub timeout: Duration,
}

impl Default for TimeoutLayerConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300),
        }
    }
}
