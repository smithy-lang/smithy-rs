/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This code was copied and then modified from https://github.com/hanabu/lambda-web

use futures_util::future::BoxFuture;
use hyper::{
    service::Service as HyperService,
    body::HttpBody as HyperHttpBody,
};
use lambda_http::{Body, Error as LambdaError, IntoResponse, Request, Response, Service};
use std::{
    convert::Infallible,
    error::Error as StdError,
    fmt::Debug,
    marker::PhantomData,
    task::{Context, Poll},
};

type HyperRequest = hyper::Request<hyper::Body>;
type HyperResponse<B> = hyper::Response<B>;

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
    S: HyperService<HyperRequest, Response = HyperResponse<B>, Error = Infallible>
        + Send + Clone + 'static,
    S::Future: Send + 'a,
    B: HyperHttpBody + Send + Debug,
    <B as HyperHttpBody>::Error: StdError + Send + Sync + 'static,
    <B as HyperHttpBody>::Data: Send,
{
    type Error = LambdaError;
    type Response = Response<Body>;
    type Future = MakeRouteLambdaServiceFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    /// Lambda handler function
    /// Parse Lambda event as hyper request,
    /// serialize hyper response to Lambda JSON response
    fn call(&mut self, event: Request) -> Self::Future {
        // Parse request
        let hyper_request = lambda_to_hyper_request(event);

        // Call hyper service when request parsing succeeded
        let svc_call = hyper_request.map(|req| Service::call(&mut self.service.clone(), req));

        let fut = async move {
            match svc_call {
                Ok(svc_fut) => {
                    // Request parsing succeeded
                    let response = svc_fut.await.expect("It should not fail");
                    // Returns as Lambda response
                    hyper_to_lambda_response(response).await
                }
                Err(request_err) => {
                    // Request parsing error
                    Err(request_err.into())
                }
            }
        };
        MakeRouteLambdaServiceFuture::new(Box::pin(fut))
    }
}

opaque_future! {
    /// Response future for [`IntoMakeLambdaService`] services.
    pub type MakeRouteLambdaServiceFuture = BoxFuture<'static, Result<Response<Body>, LambdaError>>;
}

fn lambda_to_hyper_request(event: Request) -> Result<HyperRequest, hyper::Error> {
    println!("Lambda request {:?}", event);
    let (parts, body) = event.into_parts();
    let body = match body {
        lambda_http::Body::Empty => hyper::Body::empty(),
        lambda_http::Body::Text(s) => hyper::Body::from(s),
        lambda_http::Body::Binary(v) => hyper::Body::from(v),
    };
    let req = hyper::Request::from_parts(parts, body);
    println!("Hyper request {:?}", req);
    Ok(req)
}

async fn hyper_to_lambda_response<B>(response: HyperResponse<B>) -> Result<Response<Body>, LambdaError>
where
    B: HyperHttpBody + Debug,
    <B as HyperHttpBody>::Error: StdError + Send + Sync + 'static,
{
    println!("Hyper response {:?}", response);
    // Divide resonse into headers and body
    let (parts, body) = response.into_parts();
    let body = hyper::body::to_bytes(body).await?;
    let res = Response::from_parts(parts, Body::from(body.as_ref()));
    println!("Lambda response {:?}", res);
    Ok(res.into_response())
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
