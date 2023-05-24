/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::interceptors::InterceptorContext;
use aws_smithy_runtime_api::client::orchestrator::{BoxError, HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::retries::{RetryStrategy, ShouldAttempt};
use aws_smithy_runtime_api::config_bag::ConfigBag;
use std::time::{Duration, UNIX_EPOCH};
use tokio::time::Instant;

const MIN_FILL_RATE: f64 = 0.5;
const MIN_CAPACITY: f64 = 1.0;
const REQUEST_COST: f64 = 1.0;
const SMOOTH: f64 = 0.8;
/// How much to scale back after receiving a throttling response
const BETA: f64 = 0.7;
/// Controls how aggressively we scale up after being throttled
const SCALE_CONSTANT: f64 = 0.4;

/// Manage retries for a service
///
/// An implementation of the `adaptive` AWS retry strategy. A `Strategy` is scoped to a client.
/// For an individual request, call [`Adaptive::new_request_policy()`](Adaptive::new_request_policy)
#[derive(Debug)]
pub struct AdaptiveRetryStrategy {
    /// The rate at which token are replenished.
    fill_rate: f64,
    /// The maximum capacity allowed in the token bucket.
    max_capacity: f64,
    /// The current capacity of the token bucket.
    /// The minimum this can be is 1.0
    current_capacity: f64,
    /// The last time the token bucket was refilled.
    last_timestamp: Option<Instant>,
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
    last_throttle_time: Instant,
}

impl AdaptiveRetryStrategy {
    pub async fn acquire(&mut self, amount: u64) {
        if !self.enabled {
            // return early if we haven't encountered a throttling error yet
            return;
        }

        self.refill();

        let amount = amount as f64;
        if amount > self.current_capacity {
            let sleep_duration =
                Duration::from_secs_f64((amount - self.current_capacity) / self.fill_rate);
            sleep(sleep_duration).await;
        }

        self.current_capacity -= amount;
    }

    fn refill(&mut self) {
        if let Some(last_timestamp) = self.last_timestamp {
            let fill_amount = last_timestamp.elapsed().as_secs_f64() * self.fill_rate;
            self.current_capacity =
                f64::min(self.max_capacity, self.current_capacity + fill_amount);
        }

        self.last_timestamp = Some(Instant::now());
    }

    /// We need to adjust the fill rate (and, as a result, the max capacity) of the token bucket
    /// based on responses received from a service.
    fn update_fill_rate(&mut self, fill_rate: f64) {
        // Refill based on our current rate before we update to the new fill rate.
        self.refill();

        self.fill_rate = f64::max(fill_rate, MIN_FILL_RATE);
        self.max_capacity = f64::max(fill_rate, MIN_CAPACITY);
        // When we scale down we can't have a current capacity that exceeds our max_capacity.
        self.current_capacity = f64::min(self.current_capacity, self.max_capacity);
    }

    /// We need to update the token bucket's fill rate when we receive a response from the service.
    /// The update amount will depend on whether or not we see a throttling response from a service.
    /// This process relies on being able to measure the current request rate.
    /// The request rate is measured using an exponentially smoothed average, with the rate being
    /// updated in half second buckets.
    fn update_measured_rate(&mut self) {
        // Get f64 seconds since UNIX_EPOCH
        let t = UNIX_EPOCH.elapsed().unwrap().as_secs_f64();
        let time_bucket = f64::floor(t * 2.0) / 2.0;
        self.request_count = self.request_count.saturating_sub(1);

        if time_bucket > self.last_tx_rate_bucket {
            let current_rate = self.request_count as f64 / (time_bucket - self.last_tx_rate_bucket);
            self.measured_tx_rate =
                (current_rate * SMOOTH) + (self.measured_tx_rate * (1.0 - SMOOTH));
            self.request_count = 0;
            self.last_tx_rate_bucket = time_bucket;
        }
    }

    // Called to enable the token bucket in reaction to encountering a throttling error.
    fn enable(&mut self) {
        self.enabled = true;
    }

    fn update_client_sending_rate(&mut self, is_throttling_error: bool) {
        self.update_measured_rate();
        let calculated_rate = if is_throttling_error {
            self.calculate_client_sending_rate_for_throttling_error()
        } else {
            self.calculate_client_sending_rate_for_success()
        };

        let new_rate = f64::min(calculated_rate, 2.0 * self.measured_tx_rate);
        self.update_fill_rate(new_rate);
    }

    fn calculate_client_sending_rate_for_throttling_error(&mut self) -> f64 {
        self.last_max_rate = if self.enabled {
            f64::min(self.measured_tx_rate, self.fill_rate)
        } else {
            self.measured_tx_rate
        };

        self.calculate_time_window();
        self.last_throttle_time = Instant::now();
        self.enabled = true;
        self.cubic_throttle(self.last_max_rate)
    }

    fn calculate_client_sending_rate_for_success(&mut self) -> f64 {
        self.calculate_time_window();
        let delta_t = self.last_throttle_time.elapsed();
        self.cubic_success(delta_t)
    }

    // TODO this is memoizable
    fn calculate_time_window(&self) -> f64 {
        ((self.last_max_rate * (1.0 - BETA)) / SCALE_CONSTANT).powf(1.0 / 3.0)
    }

    fn cubic_success(&self, delta_t: Duration) -> f64 {
        SCALE_CONSTANT * (delta_t.as_secs_f64() - self.calculate_time_window()).powi(3)
            + self.last_max_rate
    }

    fn cubic_throttle(&self, rate_to_use: f64) -> f64 {
        rate_to_use * BETA
    }
}

impl RetryStrategy for AdaptiveRetryStrategy {
    fn should_attempt_initial_request(
        &self,
        cfg: &mut ConfigBag,
    ) -> Result<ShouldAttempt, BoxError> {
        todo!()
    }

    fn should_attempt_retry(
        &self,
        context: &InterceptorContext<HttpRequest, HttpResponse>,
        cfg: &mut ConfigBag,
    ) -> Result<ShouldAttempt, BoxError> {
        todo!()
    }
}

#[cfg(test)]
mod test {
    use super::AdaptiveRetryStrategy;
    use std::time::Duration;
    use tokio::time::{self, Instant};

    #[tokio::test(start_paused = true)]
    async fn test_calculated_rate_with_successes() {
        let start_time = Instant::now();
        time::advance(Duration::from_secs(5)).await;

        let mut retry_strategy = AdaptiveRetryStrategy::builder()
            .last_max_rate(10.0)
            .last_throttle_time(Instant::now())
            .build();

        retry_strategy.update_client_sending_rate(false);
        assert_eq!(7.0, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(5), start_time.elapsed());

        let one_second = Duration::from_secs(1);

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(9.64893600966, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(6), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(10.000030849917364, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(7), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(10.453284520772092, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(8), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(13.408697022224185, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(9), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(21.26626835427364, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(10), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(36.425998516920465, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(11), start_time.elapsed());
    }

    #[tokio::test(start_paused = true)]
    async fn test_calculated_rate_with_throttles() {
        let start_time = Instant::now();
        time::advance(Duration::from_secs(5)).await;

        let mut retry_strategy = AdaptiveRetryStrategy::builder()
            .last_max_rate(10.0)
            .last_throttle_time(Instant::now())
            .build();

        retry_strategy.update_client_sending_rate(false);
        assert_eq!(7.0, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(5), start_time.elapsed());

        let one_second = Duration::from_secs(1);

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(9.64893600966, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(6), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(true);
        assert_eq!(6.754255206761999, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(7), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(4.727978644733399, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(8), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(6.606547753887045, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(9), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(6.763279816944947, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(10), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(7.598174833907107, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(11), start_time.elapsed());

        time::advance(one_second).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(11.511232804773524, retry_strategy.fill_rate,);
        assert_eq!(Duration::from_secs(11), start_time.elapsed());
    }

    #[tokio::test(start_paused = true)]
    async fn test_client_sending_rates() {
        let start_time = Instant::now();
        let two_hundred_milliseconds = Duration::from_millis(200);
        let mut retry_strategy = AdaptiveRetryStrategy::builder().build();

        time::advance(two_hundred_milliseconds).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(0.0, retry_strategy.measured_tx_rate,);
        assert_eq!(Duration::from_millis(200), start_time.elapsed());

        time::advance(two_hundred_milliseconds).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(0.0, retry_strategy.measured_tx_rate,);
        assert_eq!(Duration::from_millis(400), start_time.elapsed());

        time::advance(two_hundred_milliseconds).await;
        retry_strategy.update_client_sending_rate(false);
        assert_eq!(4.8, retry_strategy.measured_tx_rate,);
        assert_eq!(Duration::from_millis(600), start_time.elapsed());
    }
}
