/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::config_bag::{Storable, StoreReplace};
use aws_smithy_types::retry::ErrorKind;
use std::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::trace;

const DEFAULT_CAPACITY: usize = 500;
const DEFAULT_RETRY_COST: u32 = 5;
const DEFAULT_RETRY_TIMEOUT_COST: u32 = DEFAULT_RETRY_COST * 2;
const PERMIT_REGENERATION_AMOUNT: usize = 1;
const DEFAULT_SUCCESS_REWARD: f64 = 0.0;

/// Token bucket used for standard and adaptive retry.
#[derive(Clone, Debug)]
pub struct TokenBucket {
    semaphore: Arc<Semaphore>,
    max_permits: usize,
    timeout_retry_cost: u32,
    retry_cost: u32,
    success_reward: f64,
    fractional_tokens: AtomicF64,
}

pub struct AtomicF64 {
    storage: AtomicU64,
}
impl AtomicF64 {
    pub fn new(value: f64) -> Self {
        let as_u64 = value.to_bits();
        Self { storage: AtomicU64::new(as_u64) }
    }
    pub fn store(&self, value: f64) {
        let as_u64 = value.to_bits();
        self.storage.store(as_u64, Ordering::Relaxed)
    }
    pub fn load(&self) -> f64 {
        let as_u64 = self.storage.load(Ordering::Relaxed);
        f64::from_bits(as_u64)
    }
}

impl fmt::Debug for AtomicF64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use debug_struct, debug_tuple, or write! for formatting
        f.debug_struct("AtomicF64")
            .field("value", &self.load())
            .finish()
    }
}

impl Clone for AtomicF64 {
    fn clone(&self) -> Self {
        // Manually clone each field
        AtomicF64 {
            storage: AtomicU64::new(self.storage.load(Ordering::Relaxed)),
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
            fractional_tokens: AtomicF64::new(0.0),
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
            semaphore: Arc::new(Semaphore::new(Semaphore::MAX_PERMITS)),
            max_permits: Semaphore::MAX_PERMITS,
            timeout_retry_cost: 0,
            retry_cost: 0,
            success_reward: 0.0,
            fractional_tokens: AtomicF64::new(0.0),
        }
    }

    /// Creates a builder for constructing a `TokenBucket`.
    pub fn builder() -> TokenBucketBuilder {
        TokenBucketBuilder::default()
    }

    pub(crate) fn acquire(&self, err: &ErrorKind) -> Option<OwnedSemaphorePermit> {
        let retry_cost = if err == &ErrorKind::TransientError {
            self.timeout_retry_cost
        } else {
            self.retry_cost
        };

        self.semaphore
            .clone()
            .try_acquire_many_owned(retry_cost)
            .ok()
    }

    pub(crate) fn regenerate_a_token(&self) {
        self.add_tokens(PERMIT_REGENERATION_AMOUNT);
    }

    pub(crate) fn reward_success(&self) {
        // Verify that fractional tokens have not become corrupted
        if !self.fractional_tokens.load().is_finite() {
            tracing::error!("Fractional tokens corrupted to: {}", self.fractional_tokens.load());
            // If corrupted, reset to the number of permits the bucket was created with
            self.fractional_tokens.store(self.max_permits as f64);
            return;
        }

        if self.success_reward > 0.0 {
            self.fractional_tokens.store(self.fractional_tokens.load() + self.success_reward);
        }

        let full_tokens_accumulated = self.fractional_tokens.load().floor();
        if full_tokens_accumulated >= 1.0 {
            self.add_tokens(full_tokens_accumulated as usize);
            self.fractional_tokens.store(self.fractional_tokens.load() - full_tokens_accumulated);
        }
    }

    fn add_tokens(&self, amount: usize) {
        let available = self.semaphore.available_permits();
        if available >= self.max_permits {
            return;
        }
        let tokens_to_add = amount.min(self.max_permits - available);
        trace!("adding {tokens_to_add} back into the bucket");
        self.semaphore.add_permits(tokens_to_add);
    }

    #[cfg(all(test, any(feature = "test-util", feature = "legacy-test-util")))]
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
    success_reward: Option<f64>,
}

impl TokenBucketBuilder {
    /// Creates a new `TokenBucketBuilder` with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum bucket capacity for the builder.
    pub fn capacity(mut self, capacity: usize) -> Self {
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
    pub fn success_reward(mut self, reward: f64) -> Self {
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
            // fractional_tokens: Arc::new(Mutex::new(0.0)),
            fractional_tokens: AtomicF64::new(0.0),
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
        assert_eq!(bucket.max_permits, Semaphore::MAX_PERMITS);

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
            assert_eq!(
                tokio::sync::Semaphore::MAX_PERMITS,
                bucket.semaphore.available_permits()
            );
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
    fn test_atomicf64_f64_to_bits_conversion_correctness() {
        // This is the core functionality
        let test_values = vec![
            0.0, -0.0, 1.0, -1.0, 
            f64::INFINITY, f64::NEG_INFINITY, f64::NAN,
            f64::MIN, f64::MAX, f64::MIN_POSITIVE, f64::EPSILON,
            std::f64::consts::PI, std::f64::consts::E,
            // Test values that could expose bit manipulation bugs
            1.23456789e-308, // Very small normal number
            1.23456789e308,  // Very large number
            2.2250738585072014e-308, // Near MIN_POSITIVE
        ];
        
        for &expected in &test_values {
            let atomic = AtomicF64::new(expected);
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

    #[test]
    fn test_atomicf64_store_load_preserves_exact_bits() {
        let atomic = AtomicF64::new(0.0);
        
        // Test that store/load cycle preserves EXACT bit patterns
        // This would catch bugs in the to_bits/from_bits conversion
        let critical_bit_patterns = vec![
            0x0000000000000000u64, // +0.0
            0x8000000000000000u64, // -0.0  
            0x7FF0000000000000u64, // +infinity
            0xFFF0000000000000u64, // -infinity
            0x7FF8000000000000u64, // Quiet NaN
            0x7FF4000000000000u64, // Signaling NaN
            0x0000000000000001u64, // Smallest positive subnormal
            0x000FFFFFFFFFFFFFu64, // Largest subnormal
            0x0010000000000000u64, // Smallest positive normal (MIN_POSITIVE)
        ];
        
        for &expected_bits in &critical_bit_patterns {
            let expected_f64 = f64::from_bits(expected_bits);
            atomic.store(expected_f64);
            let loaded_f64 = atomic.load();
            let actual_bits = loaded_f64.to_bits();
            
            assert_eq!(expected_bits, actual_bits);
        }
    }

    #[test] 
    fn test_atomicf64_concurrent_store_load_safety() {
        use std::sync::Arc;
        use std::thread;
        
        let atomic = Arc::new(AtomicF64::new(0.0));
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
            
            // More importantly, verify the reading is a valid f64 
            // (not corrupted bits that happen to parse as valid)
            assert!(reading.is_finite() || reading == 0.0,
                "Corrupted reading detected: {}");
        }
    }

    #[test]
    fn test_atomicf64_stress_concurrent_access() {
        use std::sync::{Arc, Barrier};
        use std::thread;
        
        let expected_values = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let atomic = Arc::new(AtomicF64::new(0.0));
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
                    let value = i as f64;
                    atomic_clone.store(value);
                    let loaded = atomic_clone.load();
                    // Verify no corruption occurred
                    assert!(loaded >= 0.0 && loaded <= 9.0);
                    assert!(expected_values.contains(&loaded), 
                        "Got unexpected value: {}, expected one of {:?}", loaded, expected_values);
                }
            });
            handles.push(handle);
        }
        
        for handle in handles {
            handle.join().unwrap();
        }
    }


    #[test]
    fn test_atomicf64_relaxed_ordering_semantics() {
        use std::sync::Arc;
        use std::thread;
        use std::sync::atomic::{AtomicBool, Ordering};
        
        // Test that Relaxed ordering doesn't cause obvious problems
        // (This is hard to test definitively, but we can check basic operation)
        let atomic = Arc::new(AtomicF64::new(1.0));
        let flag = Arc::new(AtomicBool::new(false));
        
        let atomic_clone = Arc::clone(&atomic);
        let flag_clone = Arc::clone(&flag);
        
        let writer = thread::spawn(move || {
            atomic_clone.store(42.0);
            flag_clone.store(true, Ordering::Release);
        });
        
        let atomic_reader = Arc::clone(&atomic);
        let flag_reader = Arc::clone(&flag);
        
        let reader = thread::spawn(move || {
            // Spin until flag is set
            while !flag_reader.load(Ordering::Acquire) {
                std::hint::spin_loop();
            }
            atomic_reader.load()
        });
        
        writer.join().expect("Writer panicked");
        let final_value = reader.join().expect("Reader panicked");
        
        // Due to relaxed ordering on the AtomicF64, we might see the old or new value
        assert!(final_value == 1.0 || final_value == 42.0,
            "Unexpected value: {}", final_value);
    }

    #[test]
    fn test_atomicf64_integration_with_token_bucket_usage() {
        let atomic = AtomicF64::new(0.0);
        let success_reward = 0.3;
        let iterations = 5;
        
        // Accumulate fractional tokens
        for i in 1..=iterations {
            let current = atomic.load();
            atomic.store(current + success_reward);
        }
        
        let accumulated = atomic.load();
        let expected_total = iterations as f64 * success_reward; // 1.5
        
        // Test the floor() operation pattern
        let full_tokens = accumulated.floor();
        atomic.store(accumulated - full_tokens);
        let remaining = atomic.load();
        
        // These assertions should be general:
        assert_eq!(full_tokens, expected_total.floor()); // Could be 1.0, 2.0, 3.0, etc.
        assert!(remaining >= 0.0 && remaining < 1.0);
        assert_eq!(remaining, expected_total - expected_total.floor());
    }


    #[test]
    fn test_atomicf64_clone_creates_independent_copy() {
        let original = AtomicF64::new(123.456);
        let cloned = original.clone();
        
        // Verify they start with the same value
        assert_eq!(original.load(), cloned.load());
        
        // Verify they're independent - modifying one doesn't affect the other
        original.store(999.0);
        assert_eq!(cloned.load(), 123.456, "Clone should be unaffected by original changes");
        assert_eq!(original.load(), 999.0, "Original should have new value");
    }
}
