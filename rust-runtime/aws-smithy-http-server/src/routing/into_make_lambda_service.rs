/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

// This code was copied and then modified from https://github.com/hanabu/lambda-web

use crate::{runtime_error::RuntimeErrorKind};
use lambda_http::{Body, Error as LambdaError, Request, Response};
use std::{
    convert::Infallible,
    fmt::Debug,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use tower::Service;

type HyperRequest = hyper::Request<hyper::Body>;
type HyperResponse<B> = hyper::Response<B>;

#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct IntoMakeLambdaService<'a, S> {
    service: S,
    _phantom: PhantomData<&'a ()>,
}

impl<'a, S> IntoMakeLambdaService<'a, S>{
    pub(super) fn new(service: S) -> Self {
        Self {
            service,
            _phantom: PhantomData,
        }
    }
}

impl<'a, S, B> Service<Request> for IntoMakeLambdaService<'a, S>
where
    S: hyper::service::Service<HyperRequest, Response = HyperResponse<B>, Error = Infallible>
        + 'static,
    B: hyper::body::HttpBody + Debug,
    <B as hyper::body::HttpBody>::Error: std::error::Error + Send + Sync + 'static,
{
    type Error = RuntimeErrorKind;
    type Response = Response<Body>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

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
        let svc_call = hyper_request.map(|req| self.service.call(req));

        let fut = async move {
            match svc_call {
                Ok(svc_fut) => {
                    // Request parsing succeeded
                    match empty_response() {
                        Ok(response) => Ok(response),
                        Err(response_err) => Err(RuntimeErrorKind::InternalFailure(crate::Error::new(response_err)))
                    }
                    // match svc_fut {
                    //     Ok(response) => {
                    //         // Returns as Lambda response
                    //         hyper_to_lambda_response(response).await
                    //     }
                    //     Err(response_err) => {
                    //         // Some hyper error -> 500 Internal Server Error
                    //         Err(RuntimeErrorKind::InternalFailure(crate::Error::new(response_err)))
                    //     }
                    // }
                }
                Err(request_err) => {
                    // Request parsing error
                    Err(RuntimeErrorKind::Serialization(crate::Error::new(request_err)))
                }
            }
        };
        Box::pin(fut)
    }
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

fn empty_response() -> Result<Response<Body>, http::Error> {
    Response::builder().body(Body::Empty)
}

async fn hyper_to_lambda_response<B>(response: HyperResponse<B>) -> Result<Response<Body>, LambdaError>
where
    B: hyper::body::HttpBody + Debug,
    <B as hyper::body::HttpBody>::Error: std::error::Error + Send + Sync + 'static,
{
    println!("Hyper response {:?}", response);
    // Divide resonse into headers and body
    let (parts, body) = response.into_parts();
    let body = hyper::body::to_bytes(body).await?;
    let res = Response::from_parts(parts, Body::from(body.as_ref()));
    println!("Lambda response {:?}", res);
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
