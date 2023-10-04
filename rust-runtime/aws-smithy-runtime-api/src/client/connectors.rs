/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy connectors and related code.
//!
//! # What is a connector?
//!
//! When we talk about connectors, we are referring to the [`HttpConnector`] trait, and implementations of
//! that trait. This trait simply takes a HTTP request, and returns a future with the response for that
//! request.
//!
//! This is slightly different from what a connector is in other libraries such as
//! [`hyper`](https://crates.io/crates/hyper). In hyper 0.x, the connector is a
//! [`tower`](https://crates.io/crates/tower) `Service` that takes a `Uri` and returns
//! a future with something that implements `AsyncRead + AsyncWrite`.
//!
//! The [`HttpConnector`](crate::client::connectors::HttpConnector) is designed to be a layer on top of
//! whole HTTP libraries, such as hyper, which allows Smithy clients to be agnostic to the underlying HTTP
//! transport layer. This also makes it easy to write tests with a fake HTTP connector, and several
//! such test connector implementations are availble in [`aws-smithy-runtime`](https://crates.io/crates/aws-smithy-runtime).
//!
//! # Responsibilities of a connector
//!
//! A connector primarily makes HTTP requests, but can also be used to implement connect and read
//! timeouts. The `HyperConnector` in [`aws-smithy-runtime`](https://crates.io/crates/aws-smithy-runtime)
//! is an example where timeouts are implemented as part of the connector.
//!
//! Connectors are also responsible for DNS lookup, TLS, connection reuse, pooling, and eviction.
//! The Smithy clients have no knowledge of such concepts.

use crate::client::orchestrator::{HttpRequest, HttpResponse};
use crate::impl_shared_conversions;
use aws_smithy_async::future::now_or_later::NowOrLater;
use aws_smithy_http::result::ConnectorError;
use pin_project_lite::pin_project;
use std::fmt;
use std::future::Future as StdFuture;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

type BoxFuture = Pin<Box<dyn StdFuture<Output = Result<HttpResponse, ConnectorError>> + Send>>;

pin_project! {
    /// Future for [`HttpConnector::call`].
    pub struct HttpConnectorFuture {
        #[pin]
        inner: NowOrLater<Result<HttpResponse, ConnectorError>, BoxFuture>,
    }
}

impl HttpConnectorFuture {
    /// Create a new `HttpConnectorFuture` with the given future.
    pub fn new<F>(future: F) -> Self
    where
        F: StdFuture<Output = Result<HttpResponse, ConnectorError>> + Send + 'static,
    {
        Self {
            inner: NowOrLater::new(Box::pin(future)),
        }
    }

    /// Create a new `HttpConnectorFuture` with the given boxed future.
    ///
    /// Use this if you already have a boxed future to avoid double boxing it.
    pub fn new_boxed(
        future: Pin<Box<dyn StdFuture<Output = Result<HttpResponse, ConnectorError>> + Send>>,
    ) -> Self {
        Self {
            inner: NowOrLater::new(future),
        }
    }

    /// Create a `HttpConnectorFuture` that is immediately ready with the given result.
    pub fn ready(result: Result<HttpResponse, ConnectorError>) -> Self {
        Self {
            inner: NowOrLater::ready(result),
        }
    }
}

impl StdFuture for HttpConnectorFuture {
    type Output = Result<HttpResponse, ConnectorError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.inner.poll(cx)
    }
}

/// Trait with a `call` function that asynchronously converts a request into a response.
///
/// Ordinarily, a connector would use an underlying HTTP library such as [hyper](https://crates.io/crates/hyper),
/// and any associated HTTPS implementation alongside it to service requests.
///
/// However, it can also be useful to create fake connectors implementing this trait
/// for testing.
pub trait HttpConnector: Send + Sync + fmt::Debug {
    /// Asynchronously converts a request into a response.
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture;
}

/// A shared [`HttpConnector`] implementation.
#[derive(Clone, Debug)]
pub struct SharedHttpConnector(Arc<dyn HttpConnector>);

impl SharedHttpConnector {
    /// Returns a new [`SharedHttpConnector`].
    pub fn new(connection: impl HttpConnector + 'static) -> Self {
        Self(Arc::new(connection))
    }
}

impl HttpConnector for SharedHttpConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        (*self.0).call(request)
    }
}

impl_shared_conversions!(convert SharedHttpConnector from HttpConnector using SharedHttpConnector::new);
