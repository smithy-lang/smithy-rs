/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(missing_docs)]

//! Stalled stream protection.
//!
//! When enabled, upload and download streams that stall (stream no data) for
//! longer than a configured grace period will return an error.

use aws_smithy_types::config_bag::{Storable, StoreReplace};
use std::time::Duration;

/// The default grace period for stalled stream protection.
///
/// When a stream stalls for longer than this grace period, the stream will
/// return an error.
pub const DEFAULT_GRACE_PERIOD: Duration = Duration::from_secs(20);

/// Configuration for stalled stream protection.
///
/// When enabled, download streams that stall out will be cancelled.
#[derive(Clone, Debug)]
pub struct StalledStreamProtectionConfig {
    upload_enabled: bool,
    download_enabled: bool,
    grace_period: Duration,
    /// Minimum acceptable throughput in bytes per second. A nonzero stream that
    /// stays below this floor for longer than `grace_period` is treated the same
    /// as a fully stalled (zero-byte) stream and will be aborted.
    ///
    /// When `None` the runtime uses an internal default (1 byte/s), which
    /// catches only fully-stalled connections.
    minimum_throughput_bytes_per_second: Option<u64>,
}

impl StalledStreamProtectionConfig {
    /// Create a new config that enables stalled stream protection for both uploads and downloads.
    pub fn enabled() -> Builder {
        Builder {
            upload_enabled: Some(true),
            download_enabled: Some(true),
            grace_period: None,
            minimum_throughput_bytes_per_second: None,
        }
    }

    /// Create a new config that disables stalled stream protection.
    pub fn disabled() -> Self {
        Self {
            upload_enabled: false,
            download_enabled: false,
            grace_period: DEFAULT_GRACE_PERIOD,
            minimum_throughput_bytes_per_second: None,
        }
    }

    /// Return whether stalled stream protection is enabled for either uploads or downloads.
    pub fn is_enabled(&self) -> bool {
        self.upload_enabled || self.download_enabled
    }

    /// True if stalled stream protection is enabled for upload streams.
    pub fn upload_enabled(&self) -> bool {
        self.upload_enabled
    }

    /// True if stalled stream protection is enabled for download streams.
    pub fn download_enabled(&self) -> bool {
        self.download_enabled
    }

    /// Return the grace period for stalled stream protection.
    ///
    /// When a stream stalls for longer than this grace period, the stream will
    /// return an error.
    pub fn grace_period(&self) -> Duration {
        self.grace_period
    }

    /// Return the minimum throughput floor in bytes per second, if one was configured.
    ///
    /// A nonzero stream that stays below this floor for longer than the grace period is
    /// aborted. `None` means only fully-stalled (zero-byte) streams are caught.
    pub fn minimum_throughput_bytes_per_second(&self) -> Option<u64> {
        self.minimum_throughput_bytes_per_second
    }
}

#[derive(Clone, Debug)]
pub struct Builder {
    upload_enabled: Option<bool>,
    download_enabled: Option<bool>,
    grace_period: Option<Duration>,
    minimum_throughput_bytes_per_second: Option<u64>,
}

impl Builder {
    /// Set the grace period for stalled stream protection.
    pub fn grace_period(mut self, grace_period: Duration) -> Self {
        self.grace_period = Some(grace_period);
        self
    }

    /// Set the grace period for stalled stream protection.
    pub fn set_grace_period(&mut self, grace_period: Option<Duration>) -> &mut Self {
        self.grace_period = grace_period;
        self
    }

    /// Set the minimum throughput floor in bytes per second.
    ///
    /// A nonzero stream that stays below this value for longer than the grace period will
    /// be aborted and re-issued, catching connections that trickle data without ever
    /// recovering to a useful rate.
    pub fn minimum_throughput_bytes_per_second(mut self, bytes_per_second: u64) -> Self {
        self.minimum_throughput_bytes_per_second = Some(bytes_per_second);
        self
    }

    /// Set the minimum throughput floor in bytes per second.
    pub fn set_minimum_throughput_bytes_per_second(
        &mut self,
        bytes_per_second: Option<u64>,
    ) -> &mut Self {
        self.minimum_throughput_bytes_per_second = bytes_per_second;
        self
    }

    /// Set whether stalled stream protection is enabled for both uploads and downloads.
    pub fn is_enabled(mut self, enabled: bool) -> Self {
        self.set_is_enabled(Some(enabled));
        self
    }

    /// Set whether stalled stream protection is enabled for both uploads and downloads.
    pub fn set_is_enabled(&mut self, enabled: Option<bool>) -> &mut Self {
        self.set_upload_enabled(enabled);
        self.set_download_enabled(enabled);
        self
    }

    /// Set whether stalled stream protection is enabled for upload streams.
    pub fn upload_enabled(mut self, enabled: bool) -> Self {
        self.set_upload_enabled(Some(enabled));
        self
    }

    /// Set whether stalled stream protection is enabled for upload streams.
    pub fn set_upload_enabled(&mut self, enabled: Option<bool>) -> &mut Self {
        self.upload_enabled = enabled;
        self
    }

    /// Set whether stalled stream protection is enabled for download streams.
    pub fn download_enabled(mut self, enabled: bool) -> Self {
        self.set_download_enabled(Some(enabled));
        self
    }

    /// Set whether stalled stream protection is enabled for download streams.
    pub fn set_download_enabled(&mut self, enabled: Option<bool>) -> &mut Self {
        self.download_enabled = enabled;
        self
    }

    /// Build the config.
    pub fn build(self) -> StalledStreamProtectionConfig {
        StalledStreamProtectionConfig {
            upload_enabled: self.upload_enabled.unwrap_or_default(),
            download_enabled: self.download_enabled.unwrap_or_default(),
            grace_period: self.grace_period.unwrap_or(DEFAULT_GRACE_PERIOD),
            minimum_throughput_bytes_per_second: self.minimum_throughput_bytes_per_second,
        }
    }
}

impl From<StalledStreamProtectionConfig> for Builder {
    fn from(config: StalledStreamProtectionConfig) -> Self {
        Builder {
            upload_enabled: Some(config.upload_enabled),
            download_enabled: Some(config.download_enabled),
            grace_period: Some(config.grace_period),
            minimum_throughput_bytes_per_second: config.minimum_throughput_bytes_per_second,
        }
    }
}

impl Storable for StalledStreamProtectionConfig {
    type Storer = StoreReplace<Self>;
}
