/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This code was copied and then modified from https://github.com/hanabu/lambda-web

use crate::BoxError;
use aws_lambda_events::encodings::Body;
use futures_util::future::BoxFuture;
use http::Response;
use http_body::Body as HttpBody;
use std::{
    convert::Infallible,
    error::Error,
    fmt::Debug,
    marker::PhantomData,
    task::{Context, Poll},
};
use tower::Service;

type Request = http::Request<Body>;
type HyperRequest = http::Request<hyper::Body>;

#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct IntoMakeLambdaService<'a, S> {
    service: S,
    _marker: PhantomData<&'a ()>,
}

impl<'a, S> IntoMakeLambdaService<'a, S> {
    pub(super) fn new(service: S) -> Self {
        Self {
            service,
            _marker: PhantomData,
        }
    }
}

impl<'a, S, B> Service<Request> for IntoMakeLambdaService<'a, S>
where
    S: Service<HyperRequest, Response = Response<B>, Error = Infallible> + Send + Clone + 'static,
    S::Future: Send + 'a,
    B: HttpBody + Send + Debug,
    <B as HttpBody>::Error: Error + Send + Sync + 'static,
    <B as HttpBody>::Data: Send,
{
    type Error = BoxError;
    type Response = Response<Body>;
    type Future = MakeRouteLambdaServiceFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    /// Lambda handler function
    /// Parse Lambda request as hyper request,
    /// serialize hyper response to Lambda response
    fn call(&mut self, event: Request) -> Self::Future {
        // Parse request
        let hyper_request = lambda_to_hyper_request(event);

        // Call hyper service when request parsing succeeded
        let svc_call = Service::call(&mut self.service.clone(), hyper_request);

        let fut = async move {
           // Request parsing succeeded
           let response = svc_call.await.expect("It should not fail");
           // Returns as Lambda response
           hyper_to_lambda_response(response).await
        };
        MakeRouteLambdaServiceFuture::new(Box::pin(fut))
    }
}

opaque_future! {
    /// Response future for [`IntoMakeLambdaService`] services.
    pub type MakeRouteLambdaServiceFuture = BoxFuture<'static, Result<Response<Body>, BoxError>>;
}

fn lambda_to_hyper_request(request: Request) -> HyperRequest {
    tracing::debug!("Converting Lambda to Hyper request...");
    let (parts, body) = request.into_parts();
    let body = match body {
        Body::Empty => hyper::Body::empty(),
        Body::Text(s) => hyper::Body::from(s),
        Body::Binary(v) => hyper::Body::from(v),
    };
    let req = hyper::Request::from_parts(parts, body);
    tracing::debug!("Hyper request converted successfully.");
    req
}

async fn hyper_to_lambda_response<B>(response: Response<B>) -> Result<Response<Body>, BoxError>
where
    B: HttpBody + Debug,
    <B as HttpBody>::Error: Error + Send + Sync + 'static,
{
    tracing::debug!("Converting Hyper to Lambda response...");
    // Divide response into headers and body
    let (parts, body) = response.into_parts();
    let body = hyper::body::to_bytes(body).await?;
    let res = Response::from_parts(parts, Body::from(body.as_ref()));
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
