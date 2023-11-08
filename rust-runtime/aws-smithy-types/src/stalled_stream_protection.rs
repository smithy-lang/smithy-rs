/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(missing_docs)]

//! Stalled stream protection.
//!
//! When enabled, upload and download streams that stall (stream no data) for
//! longer than a configured grace period will return an error.

use crate::config_bag::{Storable, StoreReplace};
use std::time::Duration;

const DEFAULT_GRACE_PERIOD: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct StalledStreamProtectionConfig {
    is_enabled: bool,
    grace_period: Duration,
}

impl StalledStreamProtectionConfig {
    pub fn new_enabled() -> Builder {
        Builder {
            is_enabled: Some(true),
            grace_period: None,
        }
    }

    /// Create a new config that disables stopped stream protection.
    pub fn new_disabled() -> Self {
        Self {
            is_enabled: false,
            grace_period: DEFAULT_GRACE_PERIOD,
        }
    }

    /// Return whether stopped stream protection is enabled.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    /// Return the grace period for stalled stream protection.
    ///
    /// When a stream stalls for longer than this grace period, the stream will
    /// return an error.
    pub fn grace_period(&self) -> Duration {
        self.grace_period
    }
}

#[derive(Clone, Debug)]
pub struct Builder {
    is_enabled: Option<bool>,
    grace_period: Option<Duration>,
}

impl Builder {
    pub fn grace_period(mut self, grace_period: Duration) -> Self {
        self.grace_period = Some(grace_period);
        self
    }

    pub fn set_grace_period(&mut self, grace_period: Option<Duration>) -> &mut Self {
        self.grace_period = grace_period;
        self
    }

    pub fn is_enabled(mut self, is_enabled: bool) -> Self {
        self.is_enabled = Some(is_enabled);
        self
    }

    pub fn set_is_enabled(&mut self, is_enabled: Option<bool>) -> &mut Self {
        self.is_enabled = is_enabled;
        self
    }

    pub fn build(self) -> StalledStreamProtectionConfig {
        StalledStreamProtectionConfig {
            is_enabled: self.is_enabled.unwrap_or_default(),
            grace_period: self.grace_period.unwrap_or(DEFAULT_GRACE_PERIOD),
        }
    }
}

impl From<StalledStreamProtectionConfig> for Builder {
    fn from(config: StalledStreamProtectionConfig) -> Self {
        Builder {
            is_enabled: Some(config.is_enabled),
            grace_period: Some(config.grace_period),
        }
    }
}

impl Storable for StalledStreamProtectionConfig {
    type Storer = StoreReplace<Self>;
}
