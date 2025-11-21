/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::config_bag::{Storable, StoreReplace};
use aws_smithy_types::retry::ErrorKind;
use std::fmt;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const DEFAULT_CAPACITY: usize = 500;
// On a 32 bit architecture, the value of Semaphore::MAX_PERMITS is 536,870,911.
// Therefore, we will enforce a value lower than that to ensure behavior is
// identical across platforms.
// This also allows room for slight bucket overfill in the case where a bucket
// is at maximum capacity and another thread drops a permit it was holding.
const MAXIMUM_CAPACITY: usize = 500_000_000;
const DEFAULT_RETRY_COST: u32 = 5;
const DEFAULT_RETRY_TIMEOUT_COST: u32 = DEFAULT_RETRY_COST * 2;
const PERMIT_REGENERATION_AMOUNT: usize = 1;
const DEFAULT_SUCCESS_REWARD: f32 = 0.0;

/// Token bucket used for standard and adaptive retry.
#[derive(Clone, Debug)]
pub struct TokenBucket {
    semaphore: Arc<Semaphore>,
    max_permits: usize,
    timeout_retry_cost: u32,
    retry_cost: u32,
    success_reward: f32,
    fractional_tokens: Arc<AtomicF32>,
}

struct AtomicF32 {
    storage: AtomicU32,
}
impl AtomicF32 {
    fn new(value: f32) -> Self {
        let as_u32 = value.to_bits();
        Self {
            storage: AtomicU32::new(as_u32),
        }
    }
    fn store(&self, value: f32) {
        let as_u32 = value.to_bits();
        self.storage.store(as_u32, Ordering::Relaxed)
    }
    fn load(&self) -> f32 {
        let as_u32 = self.storage.load(Ordering::Relaxed);
        f32::from_bits(as_u32)
    }
}

impl fmt::Debug for AtomicF32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use debug_struct, debug_tuple, or write! for formatting
        f.debug_struct("AtomicF32")
            .field("value", &self.load())
            .finish()
    }
}

impl Clone for AtomicF32 {
    fn clone(&self) -> Self {
        // Manually clone each field
        AtomicF32 {
            storage: AtomicU32::new(self.storage.load(Ordering::Relaxed)),
        }
    }
}

impl Storable for TokenBucket {
    type Storer = StoreReplace<Self>;
}

impl Default for TokenBucket {
    fn default() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(DEFAULT_CAPACITY)),
            max_permits: DEFAULT_CAPACITY,
            timeout_retry_cost: DEFAULT_RETRY_TIMEOUT_COST,
            retry_cost: DEFAULT_RETRY_COST,
            success_reward: DEFAULT_SUCCESS_REWARD,
            fractional_tokens: Arc::new(AtomicF32::new(0.0)),
        }
    }
}

impl TokenBucket {
    /// Creates a new `TokenBucket` with the given initial quota.
    pub fn new(initial_quota: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(initial_quota)),
            max_permits: initial_quota,
            ..Default::default()
        }
    }

    /// A token bucket with unlimited capacity that allows retries at no cost.
    pub fn unlimited() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(MAXIMUM_CAPACITY)),
            max_permits: MAXIMUM_CAPACITY,
            timeout_retry_cost: 0,
            retry_cost: 0,
            success_reward: 0.0,
            fractional_tokens: Arc::new(AtomicF32::new(0.0)),
        }
    }

    /// Creates a builder for constructing a `TokenBucket`.
    pub fn builder() -> TokenBucketBuilder {
        TokenBucketBuilder::default()
    }

    pub(crate) fn acquire(&self, err: &ErrorKind) -> Option<OwnedSemaphorePermit> {
        // We have to handle the case where the number of permits in the semaphore exceeds the intended
        // max. This can occur when the bucket is already at max capacity (success reward > 0) and then an
        // OwnedSemaphorePermit gets dropped (destroyed), automatically returning its permits to the
        // semaphore and causing it to exceed max_permits.
        let available_permits = self.semaphore.available_permits();
        if available_permits > self.max_permits {
            self.semaphore
                .forget_permits(available_permits - self.max_permits);
        }

        let retry_cost = if err == &ErrorKind::TransientError {
            self.timeout_retry_cost
        } else {
            self.retry_cost
        };

        let result = self.semaphore
            .clone()
            .try_acquire_many_owned(retry_cost)
            .ok();

        result
    }

    pub(crate) fn success_reward(&self) -> f32 {
        self.success_reward
    }

    pub(crate) fn success_reward(&self) -> f32 {
        self.success_reward
    }

    pub(crate) fn regenerate_a_token(&self) {
        self.add_permits(PERMIT_REGENERATION_AMOUNT);
    }

    pub(crate) fn reward_success(&self) {
        let mut calc_fractional_tokens = self.fractional_tokens.load();
        // Verify that fractional tokens have not become corrupted - if they have, reset to zero
        if !calc_fractional_tokens.is_finite() {
            tracing::error!(
                "Fractional tokens corrupted to: {}, resetting to 0.0",
                calc_fractional_tokens
            );
            self.fractional_tokens.store(0.0);
            return;
        }

        if self.success_reward > 0.0 {
            calc_fractional_tokens += self.success_reward;
        }

        let full_tokens_accumulated = calc_fractional_tokens.floor();
        if full_tokens_accumulated >= 1.0 {
            self.add_permits(full_tokens_accumulated as usize);
            calc_fractional_tokens -= full_tokens_accumulated;
        }
        // Always store the updated fractional tokens back, even if no conversion happened
        self.fractional_tokens.store(calc_fractional_tokens);
    }

    pub(crate) fn add_permits(&self, amount: usize) {
        let available = self.semaphore.available_permits();
        if available >= self.max_permits {
            return;
        }
        self.semaphore
            .add_permits(amount.min(self.max_permits - available));
    }

    #[cfg(any(test, feature = "test-util", feature = "legacy-test-util"))]
    pub(crate) fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }
}

/// Builder for constructing a `TokenBucket`.
#[derive(Clone, Debug, Default)]
pub struct TokenBucketBuilder {
    capacity: Option<usize>,
    retry_cost: Option<u32>,
    timeout_retry_cost: Option<u32>,
    success_reward: Option<f32>,
}

impl TokenBucketBuilder {
    /// Creates a new `TokenBucketBuilder` with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum bucket capacity for the builder.
    pub fn capacity(mut self, mut capacity: usize) -> Self {
        if capacity > MAXIMUM_CAPACITY {
            capacity = MAXIMUM_CAPACITY;
        }
        self.capacity = Some(capacity);
        self
    }

    /// Sets the specified retry cost for the builder.
    pub fn retry_cost(mut self, retry_cost: u32) -> Self {
        self.retry_cost = Some(retry_cost);
        self
    }

    /// Sets the specified timeout retry cost for the builder.
    pub fn timeout_retry_cost(mut self, timeout_retry_cost: u32) -> Self {
        self.timeout_retry_cost = Some(timeout_retry_cost);
        self
    }

    /// Sets the reward for any successful request for the builder.
    pub fn success_reward(mut self, reward: f32) -> Self {
        self.success_reward = Some(reward);
        self
    }

    /// Builds a `TokenBucket`.
    pub fn build(self) -> TokenBucket {
        TokenBucket {
            semaphore: Arc::new(Semaphore::new(self.capacity.unwrap_or(DEFAULT_CAPACITY))),
            max_permits: self.capacity.unwrap_or(DEFAULT_CAPACITY),
            retry_cost: self.retry_cost.unwrap_or(DEFAULT_RETRY_COST),
            timeout_retry_cost: self
                .timeout_retry_cost
                .unwrap_or(DEFAULT_RETRY_TIMEOUT_COST),
            success_reward: self.success_reward.unwrap_or(DEFAULT_SUCCESS_REWARD),
            fractional_tokens: Arc::new(AtomicF32::new(0.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unlimited_token_bucket() {
        let bucket = TokenBucket::unlimited();

        // Should always acquire permits regardless of error type
        assert!(bucket.acquire(&ErrorKind::ThrottlingError).is_some());
        assert!(bucket.acquire(&ErrorKind::TransientError).is_some());

        // Should have maximum capacity
        assert_eq!(bucket.max_permits, MAXIMUM_CAPACITY);

        // Should have zero retry costs
        assert_eq!(bucket.retry_cost, 0);
        assert_eq!(bucket.timeout_retry_cost, 0);

        // The loop count is arbitrary; should obtain permits without limit
        let mut permits = Vec::new();
        for _ in 0..100 {
            let permit = bucket.acquire(&ErrorKind::ThrottlingError);
            assert!(permit.is_some());
            permits.push(permit);
            // Available permits should stay constant
            assert_eq!(MAXIMUM_CAPACITY, bucket.semaphore.available_permits());
        }
    }

    #[test]
    fn test_bounded_permits_exhaustion() {
        let bucket = TokenBucket::new(10);
        let mut permits = Vec::new();

        for _ in 0..100 {
            let permit = bucket.acquire(&ErrorKind::ThrottlingError);
            if let Some(p) = permit {
                permits.push(p);
            } else {
                break;
            }
        }

        assert_eq!(permits.len(), 2); // 10 capacity / 5 retry cost = 2 permits

        // Verify next acquisition fails
        assert!(bucket.acquire(&ErrorKind::ThrottlingError).is_none());
    }

    #[test]
    fn test_fractional_tokens_accumulate_and_convert() {
        let bucket = TokenBucket::builder()
            .capacity(10)
            .success_reward(0.4)
            .build();

        // acquire 10 tokens to bring capacity below max so we can test accumulation
        let _hold_permit = bucket.acquire(&ErrorKind::TransientError);
        assert_eq!(bucket.semaphore.available_permits(), 0);

        // First success: 0.4 fractional tokens
        bucket.reward_success();
        assert_eq!(bucket.semaphore.available_permits(), 0);

        // Second success: 0.8 fractional tokens
        bucket.reward_success();
        assert_eq!(bucket.semaphore.available_permits(), 0);

        // Third success: 1.2 fractional tokens -> 1 full token added
        bucket.reward_success();
        assert_eq!(bucket.semaphore.available_permits(), 1);
    }

    #[test]
    fn test_fractional_tokens_respect_max_capacity() {
        let bucket = TokenBucket::builder()
            .capacity(10)
            .success_reward(2.0)
            .build();

        for _ in 0..20 {
            bucket.reward_success();
        }

        assert!(bucket.semaphore.available_permits() == 10);
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn test_builder_with_custom_values() {
        let bucket = TokenBucket::builder()
            .capacity(100)
            .retry_cost(10)
            .timeout_retry_cost(20)
            .success_reward(0.5)
            .build();

        assert_eq!(bucket.max_permits, 100);
        assert_eq!(bucket.retry_cost, 10);
        assert_eq!(bucket.timeout_retry_cost, 20);
        assert_eq!(bucket.success_reward, 0.5);
    }

    #[test]
    fn test_atomicf32_f32_to_bits_conversion_correctness() {
        // This is the core functionality
        let test_values = vec![
            0.0,
            -0.0,
            1.0,
            -1.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            f32::MIN,
            f32::MAX,
            f32::MIN_POSITIVE,
            f32::EPSILON,
            std::f32::consts::PI,
            std::f32::consts::E,
            // Test values that could expose bit manipulation bugs
            1.23456789e-38, // Very small normal number
            1.23456789e38,  // Very large number (within f32 range)
            1.1754944e-38,  // Near MIN_POSITIVE for f32
        ];

        for &expected in &test_values {
            let atomic = AtomicF32::new(expected);
            let actual = atomic.load();

            // For NaN, we can't use == but must check bit patterns
            if expected.is_nan() {
                assert!(actual.is_nan(), "Expected NaN, got {}", actual);
                // Different NaN bit patterns should be preserved exactly
                assert_eq!(expected.to_bits(), actual.to_bits());
            } else {
                assert_eq!(expected.to_bits(), actual.to_bits());
            }
        }
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn test_atomicf32_store_load_preserves_exact_bits() {
        let atomic = AtomicF32::new(0.0);

        // Test that store/load cycle preserves EXACT bit patterns
        // This would catch bugs in the to_bits/from_bits conversion
        let critical_bit_patterns = vec![
            0x00000000u32, // +0.0
            0x80000000u32, // -0.0
            0x7F800000u32, // +infinity
            0xFF800000u32, // -infinity
            0x7FC00000u32, // Quiet NaN
            0x7FA00000u32, // Signaling NaN
            0x00000001u32, // Smallest positive subnormal
            0x007FFFFFu32, // Largest subnormal
            0x00800000u32, // Smallest positive normal (MIN_POSITIVE)
        ];

        for &expected_bits in &critical_bit_patterns {
            let expected_f32 = f32::from_bits(expected_bits);
            atomic.store(expected_f32);
            let loaded_f32 = atomic.load();
            let actual_bits = loaded_f32.to_bits();

            assert_eq!(expected_bits, actual_bits);
        }
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn test_atomicf32_concurrent_store_load_safety() {
        use std::sync::Arc;
        use std::thread;

        let atomic = Arc::new(AtomicF32::new(0.0));
        let test_values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let mut handles = Vec::new();

        // Start multiple threads that continuously write different values
        for &value in &test_values {
            let atomic_clone = Arc::clone(&atomic);
            let handle = thread::spawn(move || {
                for _ in 0..1000 {
                    atomic_clone.store(value);
                }
            });
            handles.push(handle);
        }

        // Start a reader thread that continuously reads
        let atomic_reader = Arc::clone(&atomic);
        let reader_handle = thread::spawn(move || {
            let mut readings = Vec::new();
            for _ in 0..5000 {
                let value = atomic_reader.load();
                readings.push(value);
            }
            readings
        });

        // Wait for all writers to complete
        for handle in handles {
            handle.join().expect("Writer thread panicked");
        }

        let readings = reader_handle.join().expect("Reader thread panicked");

        // Verify that all read values are valid (one of the written values)
        // This tests that there's no data corruption from concurrent access
        for &reading in &readings {
            assert!(test_values.contains(&reading) || reading == 0.0);

            // More importantly, verify the reading is a valid f32
            // (not corrupted bits that happen to parse as valid)
            assert!(
                reading.is_finite() || reading == 0.0,
                "Corrupted reading detected"
            );
        }
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn test_atomicf32_stress_concurrent_access() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let expected_values = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let atomic = Arc::new(AtomicF32::new(0.0));
        let barrier = Arc::new(Barrier::new(10)); // Synchronize all threads
        let mut handles = Vec::new();

        // Launch threads that all start simultaneously
        for i in 0..10 {
            let atomic_clone = Arc::clone(&atomic);
            let barrier_clone = Arc::clone(&barrier);
            let handle = thread::spawn(move || {
                barrier_clone.wait(); // All threads start at same time

                // Tight loop increases chance of race conditions
                for _ in 0..10000 {
                    let value = i as f32;
                    atomic_clone.store(value);
                    let loaded = atomic_clone.load();
                    // Verify no corruption occurred
                    assert!(loaded >= 0.0 && loaded <= 9.0);
                    assert!(
                        expected_values.contains(&loaded),
                        "Got unexpected value: {}, expected one of {:?}",
                        loaded,
                        expected_values
                    );
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_atomicf32_integration_with_token_bucket_usage() {
        let atomic = AtomicF32::new(0.0);
        let success_reward = 0.3;
        let iterations = 5;

        // Accumulate fractional tokens
        for _ in 1..=iterations {
            let current = atomic.load();
            atomic.store(current + success_reward);
        }

        let accumulated = atomic.load();
        let expected_total = iterations as f32 * success_reward; // 1.5

        // Test the floor() operation pattern
        let full_tokens = accumulated.floor();
        atomic.store(accumulated - full_tokens);
        let remaining = atomic.load();

        // These assertions should be general:
        assert_eq!(full_tokens, expected_total.floor()); // Could be 1.0, 2.0, 3.0, etc.
        assert!(remaining >= 0.0 && remaining < 1.0);
        assert_eq!(remaining, expected_total - expected_total.floor());
    }

    #[cfg(any(feature = "test-util", feature = "legacy-test-util"))]
    #[test]
    fn test_atomicf32_clone_creates_independent_copy() {
        let original = AtomicF32::new(123.456);
        let cloned = original.clone();

        // Verify they start with the same value
        assert_eq!(original.load(), cloned.load());

        // Verify they're independent - modifying one doesn't affect the other
        original.store(999.0);
        assert_eq!(
            cloned.load(),
            123.456,
            "Clone should be unaffected by original changes"
        );
        assert_eq!(original.load(), 999.0, "Original should have new value");
    }
}
