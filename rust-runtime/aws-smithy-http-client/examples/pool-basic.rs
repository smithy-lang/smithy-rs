/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Basic use of the connection pool: a single shared pool and a client.
//!
//! `SharedPool` is the configuration surface (TLS, limits, idle eviction,
//! events); `Client` is a lightweight handle that implements `HttpClient`
//! and can be passed to an SDK client config. With no partitions declared,
//! the pool uses a single anonymous partition on the current runtime.

use aws_smithy_http_client::pool::{Client, SharedPool};
use aws_smithy_http_client::tls::{self, rustls_provider::CryptoMode};
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Configure the pool once.
    let pool = SharedPool::builder()
        .tls_provider(tls::Provider::Rustls(CryptoMode::AwsLc))
        .max_connections(125)
        .pool_idle_timeout(Duration::from_secs(20))
        .build_https();

    // A `Client` is a cheap handle over the pool; it implements `HttpClient`,
    // so it can be handed to an SDK client config via `.http_client(...)`.
    let _client = Client::new(&pool);

    // Cloning the pool is cheap (an `Arc` bump); additional clients can share it.
    let _another = Client::new(&pool.clone());
}
