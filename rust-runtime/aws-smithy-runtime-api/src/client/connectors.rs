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
use aws_smithy_async::future::now_or_later::NowOrLater;
use aws_smithy_http::result::ConnectorError;
use std::fmt;
use std::future::Future as StdFuture;
use std::pin::Pin;
use std::sync::Arc;

/// Boxed future used by [`HttpConnectorFuture`].
pub type BoxFuture = Pin<Box<dyn StdFuture<Output = Result<HttpResponse, ConnectorError>> + Send>>;
/// Future for [`HttpConnector::call`].
pub type HttpConnectorFuture = NowOrLater<Result<HttpResponse, ConnectorError>, BoxFuture>;

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
