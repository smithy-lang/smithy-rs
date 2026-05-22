/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Tests demonstrating that credential resolution errors are NOT retried at the
//! operation level, even when RetryConfig::standard() is configured.
//!
//! This is the gap: the SDK's retry loop wraps credential errors as
//! `OrchestratorError::Other` which no default classifier recognizes as retryable.

use aws_config::BehaviorVersion;
use aws_credential_types::provider::error::CredentialsError;
use aws_credential_types::provider::future::ProvideCredentials as ProvideCredentialsFuture;
use aws_credential_types::provider::ProvideCredentials;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::retry::RetryConfig;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::config::SharedAsyncSleep;
use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_http_client::test_util::infallible_client_fn;
use aws_smithy_http_client::tls;
use aws_smithy_runtime_api::client::dns::{DnsFuture, ResolveDns, ResolveDnsError};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// A credential provider that fails N times then succeeds.
/// Simulates what happens when STS throttles — the STS client's inner retries
/// are exhausted and the error propagates to the operation level.
#[derive(Clone, Debug)]
struct TransientlyFailingProvider {
    attempt: Arc<AtomicU32>,
    fail_count: u32,
}

impl TransientlyFailingProvider {
    fn new(fail_count: u32) -> Self {
        Self {
            attempt: Arc::new(AtomicU32::new(0)),
            fail_count,
        }
    }

    fn attempts(&self) -> u32 {
        self.attempt.load(Ordering::SeqCst)
    }
}

impl ProvideCredentials for TransientlyFailingProvider {
    fn provide_credentials<'a>(&'a self) -> ProvideCredentialsFuture<'a>
    where
        Self: 'a,
    {
        let attempt = self.attempt.fetch_add(1, Ordering::SeqCst);
        if attempt < self.fail_count {
            ProvideCredentialsFuture::ready(Err(CredentialsError::provider_error(
                "Throttling: Rate exceeded",
            )))
        } else {
            ProvideCredentialsFuture::ready(Ok(Credentials::new(
                "AKID", "SECRET", None, None, "test",
            )))
        }
    }
}

/// A credential provider that fails N times with a DNS error then succeeds.
/// Simulates EAI_AGAIN during STS call inside a credential provider.
#[derive(Clone, Debug)]
struct DnsFailingProvider {
    attempt: Arc<AtomicU32>,
    fail_count: u32,
}

impl DnsFailingProvider {
    fn new(fail_count: u32) -> Self {
        Self {
            attempt: Arc::new(AtomicU32::new(0)),
            fail_count,
        }
    }

    fn attempts(&self) -> u32 {
        self.attempt.load(Ordering::SeqCst)
    }
}

impl ProvideCredentials for DnsFailingProvider {
    fn provide_credentials<'a>(&'a self) -> ProvideCredentialsFuture<'a>
    where
        Self: 'a,
    {
        let attempt = self.attempt.fetch_add(1, Ordering::SeqCst);
        if attempt < self.fail_count {
            ProvideCredentialsFuture::ready(Err(CredentialsError::provider_error(
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "EAI_AGAIN: Temporary failure in name resolution",
                ),
            )))
        } else {
            ProvideCredentialsFuture::ready(Ok(Credentials::new(
                "AKID", "SECRET", None, None, "test",
            )))
        }
    }
}

/// A transient credential failure SHOULD BE retried at the operation level.
#[tokio::test]
#[should_panic]
async fn transient_credential_error_is_retried_by_operation() {
    let http_client = infallible_client_fn(|_req| {
        http_1x::Response::builder()
            .status(200)
            .body("<ListAllMyBucketsResult><Buckets></Buckets></ListAllMyBucketsResult>")
            .unwrap()
    });

    let provider = TransientlyFailingProvider::new(1);

    let config = aws_config::defaults(BehaviorVersion::latest())
        .http_client(http_client)
        .credentials_provider(provider.clone())
        .region(Region::new("us-east-2"))
        .retry_config(RetryConfig::standard().with_max_attempts(3))
        .sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        .load()
        .await;

    let client = aws_sdk_s3::Client::new(&config);
    let result = client.list_buckets().send().await;

    // Desired: operation succeeds because transient credential error was retried
    assert!(result.is_ok());
    assert!(provider.attempts() > 1);
}

/// Contrast: HTTP-level transient errors ARE retried.
#[tokio::test]
async fn http_error_is_retried_by_operation() {
    let attempt = Arc::new(AtomicU32::new(0));
    let attempt_clone = attempt.clone();

    let http_client = infallible_client_fn(move |_req| {
        let n = attempt_clone.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            http_1x::Response::builder()
                .status(500)
                .body("InternalServerError")
                .unwrap()
        } else {
            http_1x::Response::builder()
                .status(200)
                .body("<ListAllMyBucketsResult><Buckets></Buckets></ListAllMyBucketsResult>")
                .unwrap()
        }
    });

    let config = aws_config::defaults(BehaviorVersion::latest())
        .http_client(http_client)
        .credentials_provider(Credentials::for_tests())
        .region(Region::new("us-east-2"))
        .retry_config(RetryConfig::standard().with_max_attempts(3))
        .sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        .load()
        .await;

    let client = aws_sdk_s3::Client::new(&config);
    let result = client.list_buckets().send().await;

    assert!(result.is_ok());
    assert_eq!(2, attempt.load(Ordering::SeqCst)); // Called twice: 500 then 200
}

/// A DNS resolver that fails N times then succeeds.
#[derive(Clone, Debug)]
struct TransientDnsFailure {
    attempt: Arc<AtomicU32>,
    fail_count: u32,
}

impl TransientDnsFailure {
    fn new(fail_count: u32) -> Self {
        Self {
            attempt: Arc::new(AtomicU32::new(0)),
            fail_count,
        }
    }

    fn attempts(&self) -> u32 {
        self.attempt.load(Ordering::SeqCst)
    }
}

impl ResolveDns for TransientDnsFailure {
    fn resolve_dns<'a>(&'a self, _name: &'a str) -> DnsFuture<'a> {
        let n = self.attempt.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_count {
            DnsFuture::ready(Err(ResolveDnsError::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "EAI_AGAIN: Temporary failure in name resolution",
            ))))
        } else {
            DnsFuture::ready(Ok(vec![IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))]))
        }
    }
}

/// DNS failure on the S3 HTTP call IS retried (ConnectorError::Io → transient).
#[tokio::test]
async fn dns_failure_on_s3_call_is_retried() {
    let resolver = TransientDnsFailure::new(1);

    let http_client = aws_smithy_http_client::Builder::new()
        .tls_provider(tls::Provider::Rustls(
            tls::rustls_provider::CryptoMode::Ring,
        ))
        .build_with_resolver(resolver.clone());

    let config = aws_config::defaults(BehaviorVersion::latest())
        .http_client(http_client)
        .credentials_provider(Credentials::for_tests())
        .region(Region::new("us-east-2"))
        .endpoint_url("http://fake-s3.example.com:1234")
        .retry_config(RetryConfig::standard().with_max_attempts(3))
        .sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        .load()
        .await;

    let client = aws_sdk_s3::Client::new(&config);
    let _ = client.list_buckets().send().await;

    assert!(resolver.attempts() > 1); // DNS failure was retried
}

/// Transient DNS failure during credential resolution SHOULD BE retried at the operation level.
/// Currently the operation fails without retrying identity resolution.
#[tokio::test]
#[should_panic]
async fn transient_dns_failure_during_credential_resolution_is_retried() {
    // Provider that fails once with a DNS error (simulating EAI_AGAIN during STS call),
    // then succeeds on the next attempt.
    let provider = DnsFailingProvider::new(1);

    let http_client = infallible_client_fn(|_req| {
        http_1x::Response::builder()
            .status(200)
            .body("<ListAllMyBucketsResult><Buckets></Buckets></ListAllMyBucketsResult>")
            .unwrap()
    });

    let config = aws_config::defaults(BehaviorVersion::latest())
        .http_client(http_client)
        .credentials_provider(provider.clone())
        .region(Region::new("us-east-2"))
        .retry_config(RetryConfig::standard().with_max_attempts(3))
        .sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
        .load()
        .await;

    let client = aws_sdk_s3::Client::new(&config);
    let result = client.list_buckets().send().await;

    // Desired: the transient DNS credential failure is retried and the operation succeeds
    assert!(result.is_ok());
    assert_eq!(2, provider.attempts());
}
