/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Interfaces for resolving DNS

use crate::box_error::BoxError;
use crate::client::orchestrator::BoxFuture;
use crate::impl_shared_conversions;
use aws_smithy_async::future::now_or_later::NowOrLater;
use std::fmt;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// New-type for the future returned by the [`DnsResolver`] trait.
pub struct DnsFuture(NowOrLater<Result<Vec<IpAddr>, BoxError>, BoxFuture<Vec<IpAddr>>>);
impl DnsFuture {
    /// Create a new `DnsFuture`
    pub fn new(
        future: impl Future<Output = Result<Vec<IpAddr>, BoxError>> + Send + 'static,
    ) -> Self {
        Self(NowOrLater::new(Box::pin(future)))
    }

    /// Create a `DnsFuture` that is immediately ready
    pub fn ready(result: Result<Vec<IpAddr>, BoxError>) -> Self {
        Self(NowOrLater::ready(result))
    }
}
impl Future for DnsFuture {
    type Output = Result<Vec<IpAddr>, BoxError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.as_mut();
        let inner = Pin::new(&mut this.0);
        Future::poll(inner, cx)
    }
}

/// Trait for resolving domain names
pub trait DnsResolver: fmt::Debug + Send + Sync {
    /// Asynchronously resolve the given domain name
    fn resolve_dns(&self, name: String) -> DnsFuture;
}

/// Shared DNS resolver
#[derive(Clone, Debug)]
pub struct SharedDnsResolver(Arc<dyn DnsResolver>);

impl SharedDnsResolver {
    /// Create a new `SharedDnsResolver`.
    pub fn new(resolver: impl DnsResolver + 'static) -> Self {
        Self(Arc::new(resolver))
    }
}

impl DnsResolver for SharedDnsResolver {
    fn resolve_dns(&self, name: String) -> DnsFuture {
        self.0.resolve_dns(name)
    }
}

impl_shared_conversions!(convert SharedDnsResolver from DnsResolver using SharedDnsResolver::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_send() {
        fn is_send<T: Send>() {}
        is_send::<DnsFuture>();
    }
}
