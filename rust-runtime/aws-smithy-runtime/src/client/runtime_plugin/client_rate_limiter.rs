/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::time::SystemTime;

use aws_smithy_runtime_api::client::orchestrator::{BoxError, ConfigBagAccessors};
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::{builder, builder_methods, builder_struct};

const MIN_FILL_RATE: f64 = 0.5;
const MIN_CAPACITY: f64 = 1.0;
const SMOOTH: f64 = 0.8;
/// How much to scale back after receiving a throttling response
const BETA: f64 = 0.7;
/// Controls how aggressively we scale up after being throttled
const SCALE_CONSTANT: f64 = 0.4;

pub struct ClientRateLimiter {
    /// The rate at which token are replenished.
    fill_rate: f64,
    /// The maximum capacity allowed in the token bucket.
    max_capacity: f64,
    /// The current capacity of the token bucket.
    /// The minimum this can be is 1.0
    current_capacity: f64,
    /// The last time the token bucket was refilled.
    last_timestamp: Option<f64>,
    /// The smoothed rate which tokens are being retrieved.
    measured_tx_rate: f64,
    /// The last half second time bucket used.
    last_tx_rate_bucket: f64,
    /// The number of requests seen within the current time bucket.
    request_count: u64,
    /// Boolean indicating if the token bucket is enabled.
    /// The token bucket is initially disabled.
    /// When a throttling error is encountered it is enabled.
    enabled: bool,
    /// The maximum rate when the client was last throttled.
    last_max_rate: f64,
    /// The last time when the client was throttled.
    last_throttle_time: f64,
    time_window: f64,
    calculated_rate: f64,
}

impl ClientRateLimiter {
    pub fn builder() -> Builder {
        Builder::new()
    }

    pub fn acquire(&mut self, cfg: &ConfigBag, amount: f64) -> Result<(), BoxError> {
        if !self.enabled {
            // return early if we haven't encountered a throttling error yet
            return Ok(());
        }

        self.refill(cfg);

        if self.current_capacity < amount {
            Err(BoxError::from("the client rate limiter is out of tokens"))
        } else {
            self.current_capacity -= amount;
            Ok(())
        }
    }

    pub fn update_sending_rate(&mut self, cfg: &ConfigBag, is_throttling_error: bool) {
        self.update_measured_rate(cfg);

        if is_throttling_error {
            let rate_to_use = if self.enabled {
                f64::min(self.measured_tx_rate, self.fill_rate)
            } else {
                self.measured_tx_rate
            };

            // The fill_rate is from the token bucket
            self.last_max_rate = rate_to_use;
            self.calculate_time_window();
            self.last_throttle_time = get_unix_timestamp(cfg);
            self.calculated_rate = cubic_throttle(rate_to_use);
            self.enable_token_bucket();
        } else {
            self.calculate_time_window();
            self.calculated_rate = self.cubic_success(get_unix_timestamp(cfg));
        }

        let new_rate = f64::min(self.calculated_rate, 2.0 * self.measured_tx_rate);
        self.token_bucket_update_rate(cfg, new_rate);
    }

    fn refill(&mut self, cfg: &ConfigBag) {
        let timestamp = get_unix_timestamp(cfg);
        match self.last_timestamp {
            None => self.last_timestamp = Some(timestamp),
            Some(last_timestamp) => {
                let fill_amount = (timestamp - last_timestamp) * self.fill_rate;
                self.current_capacity =
                    f64::min(self.current_capacity + fill_amount, self.max_capacity);
                self.last_timestamp = Some(timestamp);
            }
        }
    }

    fn token_bucket_update_rate(&mut self, cfg: &ConfigBag, new_fill_rate: f64) {
        // Refill based on our current rate before we update to the new fill rate.
        self.refill(cfg);

        self.fill_rate = f64::max(new_fill_rate, MIN_FILL_RATE);
        self.max_capacity = f64::max(new_fill_rate, MIN_CAPACITY);
        // When we scale down we can't have a current capacity that exceeds our max_capacity.
        self.current_capacity = f64::min(self.current_capacity, self.max_capacity);
    }

    fn enable_token_bucket(&mut self) {
        self.enabled = true;
    }

    fn update_measured_rate(&mut self, cfg: &ConfigBag) {
        let t = get_unix_timestamp(cfg);
        let time_bucket = (t.floor() * 2.0) / 2.0;
        self.request_count += 1;

        if time_bucket > self.last_tx_rate_bucket {
            let current_rate = self.request_count as f64 / (time_bucket - self.last_tx_rate_bucket);
            self.measured_tx_rate =
                (current_rate * SMOOTH) + (self.measured_tx_rate * (1.0 - SMOOTH));
            self.request_count = 0;
            self.last_tx_rate_bucket = time_bucket.floor();
        }
    }

    fn calculate_time_window(&mut self) {
        // This is broken out into a separate calculation because it only
        // gets updated when @last_max_rate changes so it can be cached.
        let base = (self.last_max_rate * (1.0 - BETA)) / SCALE_CONSTANT;
        self.time_window = base.powf(1.0 / 3.0);
    }

    fn cubic_success(&self, timestamp: f64) -> f64 {
        let dt = timestamp - self.last_throttle_time;
        let dt_sub_time_window = dt - self.time_window;
        (SCALE_CONSTANT * dt_sub_time_window.powi(3)) + self.last_max_rate
    }
}

fn cubic_throttle(rate_to_use: f64) -> f64 {
    rate_to_use * BETA
}

fn get_unix_timestamp(cfg: &ConfigBag) -> f64 {
    let request_time = cfg.request_time().unwrap();
    request_time
        .now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

builder!(
    set_fill_rate, fill_rate, f64, "The rate at which token are replenished.",
    set_max_capacity, max_capacity, f64, "The maximum capacity allowed in the token bucket.",
    set_current_capacity, current_capacity, f64, "The current capacity of the token bucket. The minimum this can be is 1.0",
    set_last_timestamp, last_timestamp, f64, "The last time the token bucket was refilled.",
    set_measured_tx_rate, measured_tx_rate, f64, "The smoothed rate which tokens are being retrieved.",
    set_last_tx_rate_bucket, last_tx_rate_bucket, f64, "The last half second time bucket used.",
    set_request_count, request_count, u64, "The number of requests seen within the current time bucket.",
    set_enabled, enabled, bool, "Boolean indicating if the token bucket is enabled. The token bucket is initially disabled. When a throttling error is encountered it is enabled.",
    set_last_max_rate, last_max_rate, f64, "The maximum rate when the client was last throttled.",
    set_last_throttle_time, last_throttle_time, f64, "The last time when the client was throttled.",
    set_time_window, time_window, f64, "The time window used to calculate the cubic success rate.",
    set_calculated_rate, calculated_rate, f64, "The calculated rate used to update the sending rate."
);

impl Builder {
    pub fn build(self) -> ClientRateLimiter {
        ClientRateLimiter {
            fill_rate: self.fill_rate.unwrap_or_default(),
            max_capacity: self.max_capacity.unwrap_or(f64::MAX),
            current_capacity: self.current_capacity.unwrap_or_default(),
            last_timestamp: self.last_timestamp,
            enabled: self.enabled.unwrap_or_default(),
            measured_tx_rate: self.measured_tx_rate.unwrap_or_default(),
            last_tx_rate_bucket: self.last_tx_rate_bucket.unwrap_or_default(),
            request_count: self.request_count.unwrap_or_default(),
            last_max_rate: self.last_max_rate.unwrap_or_default(),
            last_throttle_time: self.last_throttle_time.unwrap_or_default(),
            time_window: self.time_window.unwrap_or_default(),
            calculated_rate: self.calculated_rate.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{get_unix_timestamp, ClientRateLimiter};
    use crate::client::runtime_plugin::client_rate_limiter::cubic_throttle;
    use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
    use aws_smithy_async::test_util::instant_time_and_sleep;
    use aws_smithy_async::time::SharedTimeSource;
    use aws_smithy_runtime_api::client::orchestrator::ConfigBagAccessors;
    use aws_smithy_types::config_bag::ConfigBag;
    use std::time::{Duration, SystemTime};

    #[test]
    fn it_sets_the_time_window_correctly() {
        let mut rate_limiter = ClientRateLimiter::builder().last_max_rate(10.0).build();

        rate_limiter.calculate_time_window();
        assert_eq!(rate_limiter.time_window, 1.9574338205844317);
    }

    #[test]
    fn should_match_beta_decrease() {
        let new_rate = cubic_throttle(10.0);
        assert_eq!(new_rate, 7.0);

        let mut rate_limiter = ClientRateLimiter::builder()
            .last_max_rate(10.0)
            .last_throttle_time(1.0)
            .build();

        rate_limiter.calculate_time_window();
        let new_rate = rate_limiter.cubic_success(1.0);
        assert_eq!(new_rate, 7.0);
    }

    #[tokio::test]
    async fn throttling_is_enabled_once_throttling_error_is_received() {
        let mut cfg = ConfigBag::base();
        let (time_source, sleep_impl) = instant_time_and_sleep(SystemTime::UNIX_EPOCH);
        cfg.interceptor_state()
            .set_request_time(SharedTimeSource::new(time_source));
        cfg.interceptor_state()
            .set_sleep_impl(Some(SharedAsyncSleep::new(sleep_impl.clone())));
        let now = get_unix_timestamp(&cfg);
        let mut rate_limiter = ClientRateLimiter::builder()
            .last_tx_rate_bucket((now).floor())
            .last_throttle_time(now)
            .build();

        assert!(
            !rate_limiter.enabled,
            "rate_limiter should be disabled by default"
        );
        rate_limiter.update_sending_rate(&cfg, true);
        assert!(
            rate_limiter.enabled,
            "rate_limiter should be enabled after throttling error"
        );
    }

    const ONE_SECOND: Duration = Duration::from_secs(1);

    #[tokio::test]
    async fn test_calculated_rate_with_successes() {
        let mut cfg = ConfigBag::base();
        let (time_source, sleep_impl) = instant_time_and_sleep(SystemTime::UNIX_EPOCH);
        sleep_impl.sleep(Duration::from_secs(5)).await;
        cfg.interceptor_state()
            .set_request_time(SharedTimeSource::new(time_source));
        cfg.interceptor_state()
            .set_sleep_impl(Some(SharedAsyncSleep::new(sleep_impl.clone())));
        let now = get_unix_timestamp(&cfg);
        let mut rate_limiter = ClientRateLimiter::builder()
            .last_tx_rate_bucket((now).floor())
            .last_throttle_time(now)
            .enabled(true)
            .last_max_rate(10.0)
            .build();

        struct Attempt {
            throttled: bool,
            total_duration: Duration,
            expected_calculated_rate: f64,
        }

        let attempts = [
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(5),
                expected_calculated_rate: 7.0,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(6),
                expected_calculated_rate: 9.64893600966,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(7),
                expected_calculated_rate: 10.000030849917364,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(8),
                expected_calculated_rate: 10.453284520772092,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(9),
                expected_calculated_rate: 13.408697022224185,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(10),
                expected_calculated_rate: 21.26626835427364,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(11),
                expected_calculated_rate: 36.425998516920465,
            },
        ];

        for attempt in attempts {
            rate_limiter.update_sending_rate(&cfg, attempt.throttled);
            assert_eq!(
                attempt.expected_calculated_rate,
                rate_limiter.calculated_rate
            );
            assert_eq!(attempt.total_duration, sleep_impl.total_duration());
            sleep_impl.sleep(ONE_SECOND).await;
        }
    }

    #[tokio::test]
    async fn test_calculated_rate_with_throttles() {
        let mut cfg = ConfigBag::base();
        let (time_source, sleep_impl) = instant_time_and_sleep(SystemTime::UNIX_EPOCH);
        sleep_impl.sleep(Duration::from_secs(5)).await;
        cfg.interceptor_state()
            .set_request_time(SharedTimeSource::new(time_source));
        cfg.interceptor_state()
            .set_sleep_impl(Some(SharedAsyncSleep::new(sleep_impl.clone())));
        let now = get_unix_timestamp(&cfg);
        let mut rate_limiter = ClientRateLimiter::builder()
            .last_tx_rate_bucket((now).floor())
            .last_throttle_time(now)
            .enabled(true)
            .last_max_rate(10.0)
            .build();

        struct Attempt {
            throttled: bool,
            total_duration: Duration,
            expected_calculated_rate: f64,
        }

        let attempts = [
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(5),
                expected_calculated_rate: 7.0,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(6),
                expected_calculated_rate: 9.64893600966,
            },
            Attempt {
                throttled: true,
                total_duration: Duration::from_secs(7),
                expected_calculated_rate: 6.754255206761999,
            },
            Attempt {
                throttled: true,
                total_duration: Duration::from_secs(8),
                expected_calculated_rate: 4.727978644733399,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(9),
                expected_calculated_rate: 6.606547753887045,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(10),
                expected_calculated_rate: 6.763279816944947,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(11),
                expected_calculated_rate: 7.598174833907107,
            },
            Attempt {
                throttled: false,
                total_duration: Duration::from_secs(12),
                expected_calculated_rate: 11.511232804773524,
            },
        ];

        for attempt in attempts {
            rate_limiter.update_sending_rate(&cfg, attempt.throttled);
            assert_eq!(
                attempt.expected_calculated_rate,
                rate_limiter.calculated_rate
            );
            assert_eq!(attempt.total_duration, sleep_impl.total_duration());
            sleep_impl.sleep(ONE_SECOND).await;
        }
    }

    #[tokio::test]
    async fn test_client_sending_rates() {
        let mut cfg = ConfigBag::base();
        let (time_source, sleep_impl) = instant_time_and_sleep(SystemTime::UNIX_EPOCH);
        cfg.interceptor_state()
            .set_request_time(SharedTimeSource::new(time_source));
        cfg.interceptor_state()
            .set_sleep_impl(Some(SharedAsyncSleep::new(sleep_impl.clone())));
        let now = get_unix_timestamp(&cfg);
        let mut rate_limiter = ClientRateLimiter::builder()
            .last_tx_rate_bucket(now.floor())
            .build();

        struct Attempt {
            throttled: bool,
            timestamp: Duration,
            expected_measured_tx_rate: f64,
            expected_new_token_bucket_rate: f64,
        }

        let attempts = [
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(0.2),
                expected_measured_tx_rate: 0.000000,
                expected_new_token_bucket_rate: 0.500000,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(0.4),
                expected_measured_tx_rate: 0.000000,
                expected_new_token_bucket_rate: 0.500000,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(0.6),
                expected_measured_tx_rate: 4.800000,
                expected_new_token_bucket_rate: 0.500000,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(0.8),
                expected_measured_tx_rate: 4.800000,
                expected_new_token_bucket_rate: 0.500000,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(1.0),
                expected_measured_tx_rate: 4.160000,
                expected_new_token_bucket_rate: 0.500000,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(1.2),
                expected_measured_tx_rate: 4.160000,
                expected_new_token_bucket_rate: 0.691200,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(1.4),
                expected_measured_tx_rate: 4.160000,
                expected_new_token_bucket_rate: 1.097600,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(1.6),
                expected_measured_tx_rate: 5.632000,
                expected_new_token_bucket_rate: 1.638400,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(1.8),
                expected_measured_tx_rate: 5.632000,
                expected_new_token_bucket_rate: 2.332800,
            },
            Attempt {
                throttled: true,
                timestamp: Duration::from_secs_f64(2.0),
                expected_measured_tx_rate: 4.326400,
                expected_new_token_bucket_rate: 3.028480,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(2.2),
                expected_measured_tx_rate: 4.326400,
                expected_new_token_bucket_rate: 3.486639,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(2.4),
                expected_measured_tx_rate: 4.326400,
                expected_new_token_bucket_rate: 3.821874,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(2.6),
                expected_measured_tx_rate: 5.665280,
                expected_new_token_bucket_rate: 4.053386,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(2.8),
                expected_measured_tx_rate: 5.665280,
                expected_new_token_bucket_rate: 4.200373,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(3.0),
                expected_measured_tx_rate: 4.333056,
                expected_new_token_bucket_rate: 4.282037,
            },
            Attempt {
                throttled: true,
                timestamp: Duration::from_secs_f64(3.2),
                expected_measured_tx_rate: 4.333056,
                expected_new_token_bucket_rate: 2.997426,
            },
            Attempt {
                throttled: false,
                timestamp: Duration::from_secs_f64(3.4),
                expected_measured_tx_rate: 4.333056,
                expected_new_token_bucket_rate: 3.452226,
            },
        ];

        let two_hundred_milliseconds = Duration::from_millis(200);
        for attempt in attempts {
            sleep_impl.sleep(two_hundred_milliseconds).await;
            rate_limiter.update_sending_rate(&cfg, attempt.throttled);
            assert_eq!(
                attempt.expected_measured_tx_rate,
                rate_limiter.measured_tx_rate,
            );
            assert_eq!(attempt.timestamp, sleep_impl.total_duration());
            assert_eq!(
                attempt.expected_new_token_bucket_rate,
                rate_limiter.fill_rate
            );
        }
    }

    #[tokio::test]
    async fn test_client_sending_rates_with_throttles() {
        use std::default::Default;

        #[derive(Default)]
        struct Attempt {
            throttled: bool,
            timestamp: f64,
            expected_measured_tx_rate: f64,
            expected_fill_rate: f64,
        }

        let attempts = [
            Attempt {
                timestamp: Duration::from_millis(200).as_secs_f64(),
                expected_fill_rate: 0.5,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(400).as_secs_f64(),
                expected_fill_rate: 0.5,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(600).as_secs_f64(),
                expected_measured_tx_rate: 4.8,
                expected_fill_rate: 0.5,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(800).as_secs_f64(),
                expected_measured_tx_rate: 4.8,
                expected_fill_rate: 0.5,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(1000).as_secs_f64(),
                expected_measured_tx_rate: 4.16,
                expected_fill_rate: 0.5,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(1200).as_secs_f64(),
                expected_measured_tx_rate: 4.16,
                expected_fill_rate: 0.6912,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(1400).as_secs_f64(),
                expected_measured_tx_rate: 4.16,
                expected_fill_rate: 1.0976,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(1600).as_secs_f64(),
                expected_measured_tx_rate: 5.632,
                expected_fill_rate: 1.6384,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(1800).as_secs_f64(),
                expected_measured_tx_rate: 5.632,
                expected_fill_rate: 2.3328,
                ..Default::default()
            },
            Attempt {
                throttled: true,
                timestamp: Duration::from_millis(2000).as_secs_f64(),
                expected_measured_tx_rate: 4.3264,
                expected_fill_rate: 3.02848,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(2200).as_secs_f64(),
                expected_measured_tx_rate: 4.3264,
                expected_fill_rate: 3.4866391734702598,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(2400).as_secs_f64(),
                expected_measured_tx_rate: 4.3264,
                expected_fill_rate: 3.8218744160402554,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(2600).as_secs_f64(),
                expected_measured_tx_rate: 5.665280,
                expected_fill_rate: 4.053385727709987,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(2800).as_secs_f64(),
                expected_measured_tx_rate: 5.665280,
                expected_fill_rate: 4.200373108479455,
                ..Default::default()
            },
            Attempt {
                timestamp: Duration::from_millis(3000).as_secs_f64(),
                expected_measured_tx_rate: 4.333056,
                expected_fill_rate: 4.282036558348658,
                ..Default::default()
            },
            Attempt {
                throttled: true,
                timestamp: Duration::from_millis(3200).as_secs_f64(),
                expected_measured_tx_rate: 4.333056,
                expected_fill_rate: 2.99742559084406,
            },
            Attempt {
                timestamp: Duration::from_millis(3400).as_secs_f64(),
                expected_measured_tx_rate: 4.333056,
                expected_fill_rate: 3.4522263943863463,
                ..Default::default()
            },
        ];

        let mut cfg = ConfigBag::base();
        let (time_source, sleep_impl) = instant_time_and_sleep(SystemTime::UNIX_EPOCH);
        cfg.interceptor_state()
            .set_request_time(SharedTimeSource::new(time_source));
        cfg.interceptor_state()
            .set_sleep_impl(Some(SharedAsyncSleep::new(sleep_impl.clone())));
        let now = get_unix_timestamp(&cfg);
        let mut rate_limiter = ClientRateLimiter::builder()
            .last_tx_rate_bucket(now.floor())
            .last_throttle_time(now)
            .build();

        for attempt in attempts {
            sleep_impl
                .sleep(Duration::from_secs_f64(attempt.timestamp))
                .await;
            rate_limiter.update_sending_rate(&cfg, attempt.throttled);
            assert_eq!(
                attempt.expected_measured_tx_rate,
                rate_limiter.measured_tx_rate
            );
            assert_eq!(attempt.expected_fill_rate, rate_limiter.fill_rate);
        }
    }
}
