/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::body::SdkBody;
use aws_smithy_http::result::ConnectorError;
use aws_smithy_runtime_api::client::connectors::{
    HttpConnector, HttpConnectorFuture, SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use std::fmt;
use std::sync::Arc;

/// Create a [`SharedHttpConnector`] from `Fn(http:Request) -> http::Response`
///
/// # Examples
///
/// ```rust
/// use aws_smithy_runtime::client::connectors::test_util::infallible_connection_fn;
/// let connector = infallible_connection_fn(|_req| http::Response::builder().status(200).body("OK!").unwrap());
/// ```
pub fn infallible_connection_fn<B>(
    f: impl Fn(http::Request<SdkBody>) -> http::Response<B> + Send + Sync + 'static,
) -> SharedHttpConnector
where
    B: Into<SdkBody>,
{
    SharedHttpConnector::new(InfallibleConnectorFn::new(f))
}

#[derive(Clone)]
struct InfallibleConnectorFn {
    #[allow(clippy::type_complexity)]
    response: Arc<
        dyn Fn(http::Request<SdkBody>) -> Result<http::Response<SdkBody>, ConnectorError>
            + Send
            + Sync,
    >,
}

impl fmt::Debug for InfallibleConnectorFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InfallibleConnectorFn").finish()
    }
}

impl InfallibleConnectorFn {
    fn new<B: Into<SdkBody>>(
        f: impl Fn(http::Request<SdkBody>) -> http::Response<B> + Send + Sync + 'static,
    ) -> Self {
        Self {
            response: Arc::new(move |request| Ok(f(request).map(|b| b.into()))),
        }
    }
}

impl HttpConnector for InfallibleConnectorFn {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        HttpConnectorFuture::ready((self.response)(request))
    }
}
