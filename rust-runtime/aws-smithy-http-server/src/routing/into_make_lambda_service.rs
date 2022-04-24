/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This code was heavily inspired by https://github.com/hanabu/lambda-web

use crate::BoxError;
use futures_util::future::BoxFuture;
use http::{uri, Response};
use http_body::Body;
use hyper::Body as HyperBody;
#[allow(unused_imports)]
use lambda_http::{Body as LambdaBody, Request, RequestExt as _};
use std::{
    convert::Infallible,
    error::Error,
    fmt::Debug,
    task::{Context, Poll},
};
use tower::Service;

type HyperRequest = http::Request<HyperBody>;

/// A [`MakeService`] that produces AWS Lambda compliant services.
///
/// [`MakeService`]: tower::make::MakeService
#[derive(Debug, Clone)]
pub struct IntoMakeLambdaService<S> {
    service: S,
}

impl<S> IntoMakeLambdaService<S> {
    pub(super) fn new(service: S) -> Self {
        Self { service }
    }
}

impl<S, B> Service<Request> for IntoMakeLambdaService<S>
where
    S: Service<HyperRequest, Response = Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Body + Send + Debug,
    <B as Body>::Error: Error + Send + Sync + 'static,
    <B as Body>::Data: Send,
{
    type Error = BoxError;
    type Response = Response<LambdaBody>;
    type Future = MakeRouteLambdaServiceFuture;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx).map_err(|err| err.into())
    }

    /// Lambda handler function
    /// Parse Lambda request as hyper request,
    /// serialize hyper response to Lambda response
    fn call(&mut self, event: Request) -> Self::Future {
        // As recommended in https://github.com/tower-rs/tower/issues/547
        let clone = self.service.clone();
        let mut inner = std::mem::replace(&mut self.service, clone);

        let fut = async move {
            // Parse request
            let hyper_request = lambda_to_hyper_request(event)?;

            // Call Hyper service when request parsing succeeded
            let response = inner.call(hyper_request).await?;

            // Return Lambda response after finished calling inner service
            hyper_to_lambda_response(response).await
        };
        MakeRouteLambdaServiceFuture::new(Box::pin(fut))
    }
}

opaque_future! {
    /// Response future for [`IntoMakeLambdaService`] services.
    pub type MakeRouteLambdaServiceFuture = BoxFuture<'static, Result<Response<LambdaBody>, BoxError>>;
}

fn lambda_to_hyper_request(request: Request) -> Result<HyperRequest, BoxError> {
    tracing::debug!("Converting Lambda to Hyper request...");
    // Raw HTTP path without any stage information
    let raw_path = request.raw_http_path();
    let (parts, body) = request.into_parts();
    let mut uri: uri::Uri = parts.uri;
    let mut path = String::from(uri.path());
    if !raw_path.is_empty() && raw_path != path {
        tracing::debug!("Recreating URI from raw HTTP path.");
        path = raw_path;
        let uri_parts: uri::Parts = uri.into();
        let path_and_query = uri_parts.path_and_query.unwrap();
        if let Some(query) = path_and_query.query() {
            path.push('?');
            path.push_str(query);
        }
        uri = uri::Uri::builder()
            .authority(uri_parts.authority.unwrap())
            .scheme(uri_parts.scheme.unwrap())
            .path_and_query(path)
            .build()
            .unwrap();
    }

    let body = match body {
        LambdaBody::Empty => HyperBody::empty(),
        LambdaBody::Text(s) => HyperBody::from(s),
        LambdaBody::Binary(v) => HyperBody::from(v),
    };
    let mut req = http::Request::builder()
        .method(parts.method)
        .uri(uri)
        .body(body)
        .unwrap();
    // There is no builder method that sets headers in batch
    let _ = std::mem::replace(req.headers_mut(), parts.headers);
    tracing::debug!("Hyper request converted successfully.");
    Ok(req)
}

async fn hyper_to_lambda_response<B>(response: Response<B>) -> Result<Response<LambdaBody>, BoxError>
where
    B: Body + Debug,
    <B as Body>::Error: Error + Send + Sync + 'static,
{
    tracing::debug!("Converting Hyper to Lambda response...");
    // Divide response into headers and body
    let (parts, body) = response.into_parts();
    let body = hyper::body::to_bytes(body).await?;
    let res = Response::from_parts(parts, LambdaBody::from(body.as_ref()));
    tracing::debug!("Lambda response converted successfully.");
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traits() {
        use crate::test_helpers::*;

        assert_send::<IntoMakeLambdaService<()>>();
        assert_sync::<IntoMakeLambdaService<()>>();
    }
}
