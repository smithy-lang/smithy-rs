/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

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
