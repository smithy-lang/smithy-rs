/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime::client::dns::CachingDnsResolver;
use aws_smithy_runtime_api::client::dns::ResolveDns;
use std::time::Instant;
use tokio::test;

// Note: I couldn't come up with a way to mock the DNS returns to hickory so these
// tests actually hit the network. We should ideally find a better way to do this.

#[test]
async fn test_dns_caching_performance() {
    let resolver = CachingDnsResolver::default();
    let hostname = "example.com";

    // First resolution should hit the network
    let start = Instant::now();
    let first_result = resolver.resolve_dns(hostname).await;
    let first_duration = start.elapsed();

    let first_ips = first_result.unwrap();
    assert!(!first_ips.is_empty());

    // Second resolution should hit the cache
    let start = Instant::now();
    let second_result = resolver.resolve_dns(hostname).await;
    let second_duration = start.elapsed();

    let second_ips = second_result.unwrap();

    // Verify same IPs returned
    assert_eq!(first_ips, second_ips);

    // Cache hit should be faster
    assert!(second_duration < first_duration);
}

#[test]
async fn test_dns_cache_size_limit() {
    let resolver = CachingDnsResolver::builder().cache_size(1).build();

    // Resolve first hostname
    let result1 = resolver.resolve_dns("example.com").await;
    assert!(result1.is_ok());

    // Resolve second hostname (should evict first from cache)
    let result2 = resolver.resolve_dns("aws.com").await;
    assert!(result2.is_ok());

    // Both should still work first should still be in cache (its TTL has not expired),
    // second should have to re-resolve since it was never cached

    let start = Instant::now();
    let result2_again = resolver.resolve_dns("aws.com").await;
    let result2_again_duration = start.elapsed();

    let start = Instant::now();
    let result1_again = resolver.resolve_dns("example.com").await;
    let result1_again_duration = start.elapsed();

    // result1_again should be resolved more quickly than result2_again
    println!("result1_again_duration: {:?}", result1_again_duration);
    println!("result2_again_duration: {:?}", result2_again_duration);
    assert!(result1_again_duration < result2_again_duration);

    assert!(result1_again.is_ok());
    assert!(result2_again.is_ok());
}

#[test]
async fn test_dns_error_handling() {
    let resolver = CachingDnsResolver::default();

    // Try to resolve an invalid hostname
    let result = resolver
        .resolve_dns("invalid.nonexistent.domain.test")
        .await;
    assert!(result.is_err());
}
