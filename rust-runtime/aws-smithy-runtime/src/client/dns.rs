/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Built-in DNS resolver implementations.

#[cfg(all(feature = "rt-tokio", not(target_family = "wasm")))]
mod tokio {
    use aws_smithy_runtime_api::client::dns::{DnsFuture, DnsResolver};
    use std::io::{Error as IoError, ErrorKind as IoErrorKind};
    use std::net::ToSocketAddrs;

    /// DNS resolver that uses `tokio::spawn_blocking` to resolve DNS using the standard library.
    ///
    /// This implementation isn't available for WASM targets.
    #[non_exhaustive]
    #[derive(Debug, Default)]
    pub struct TokioDnsResolver;

    impl TokioDnsResolver {
        /// Creates a new Tokio DNS resolver
        pub fn new() -> Self {
            Self
        }
    }

    impl DnsResolver for TokioDnsResolver {
        fn resolve_dns(&self, name: String) -> DnsFuture {
            DnsFuture::new(async move {
                let result = tokio::task::spawn_blocking(move || (name, 0).to_socket_addrs()).await;
                match result {
                    Err(join_failure) => Err(IoError::new(IoErrorKind::Other, join_failure).into()),
                    Ok(Ok(dns_result)) => {
                        Ok(dns_result.into_iter().map(|addr| addr.ip()).collect())
                    }
                    Ok(Err(dns_failure)) => Err(dns_failure.into()),
                }
            })
        }
    }
}

#[cfg(feature = "rt-tokio")]
pub use self::tokio::TokioDnsResolver;
