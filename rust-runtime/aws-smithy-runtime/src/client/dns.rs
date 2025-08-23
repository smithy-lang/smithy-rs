/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Built-in DNS resolver implementations.

#[cfg(all(feature = "rt-tokio", not(target_family = "wasm")))]
mod tokio;

#[cfg(all(feature = "rt-tokio", not(target_family = "wasm")))]
pub use self::tokio::TokioDnsResolver;

#[cfg(all(feature = "hickory-dns", not(target_family = "wasm")))]
mod caching;

#[cfg(all(feature = "hickory-dns", not(target_family = "wasm")))]
pub use self::caching::CachingDnsResolver;
