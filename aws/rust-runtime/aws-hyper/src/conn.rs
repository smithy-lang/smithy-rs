/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use hyper_tls::HttpsConnector;
use hyper::client::{HttpConnector, ResponseFuture};
use smithy_http::body::SdkBody;
use crate::test_connection::TestConnection;
use std::task::{Context, Poll};
use crate::BoxError;
use std::future::{Future, Ready};
use std::pin::Pin;

/// A good base connection type for most use cases
///
/// This supports three options:
/// 1. HTTPS
/// 2. A `TestConnection`
/// 3. Any implementation of the `HttpService` trait

pub enum Standard {
    Https(hyper::Client<HttpsConnector<HttpConnector>, SdkBody>),
    Test(TestConnection<hyper::Body>),
    Dyn(Box<dyn HttpService>)
}

impl Clone for Standard {
    fn clone(&self) -> Self {
        match self {
            Standard::Https(client) => Standard::Https(client.clone()),
            Standard::Test(test_conn) => Standard::Test(test_conn.clone()),
            Standard::Dyn(box_conn) => Standard::Dyn(box_conn.clone())
        }
    }
}

impl Clone for Box<dyn HttpService> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub trait HttpService: HttpServiceClone + Send + Sync {
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BoxError>>;
    fn call(&mut self, req: http::Request<SdkBody>) -> Pin<Box<dyn Future<Output=Result<http::Response<hyper::Body>, BoxError>> + Send>>;
}

pub trait HttpServiceClone {
    fn clone_box(&self) -> Box<dyn HttpService>;
}

impl tower::Service<http::Request<SdkBody>> for Standard {
    type Response = http::Response<hyper::Body>;
    type Error = BoxError;
    type Future = StandardFuture;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self {
            Standard::Https(https) => https.poll_ready(cx).map_err(|err|err.into()),
            Standard::Dyn(conn) => conn.poll_ready(cx),
            Standard::Test(_) => Poll::Ready(Result::Ok(()))
        }
    }

    fn call(&mut self, req: http::Request<SdkBody>) -> Self::Future {
        match self {
            Standard::Https(https) => StandardFuture::Https(https.call(req)),
            Standard::Dyn(conn) => StandardFuture::Dyn(conn.call(req)),
            Standard::Test(conn) => StandardFuture::TestConn(conn.call(req))
        }
    }
}


#[pin_project::pin_project(project = FutProj)]
pub enum StandardFuture {
    Https(#[pin] ResponseFuture),
    TestConn(#[pin] Ready<Result<http::Response<hyper::Body>, BoxError>>),
    Dyn(#[pin] Pin<Box<dyn Future<Output=Result<http::Response<hyper::Body>, BoxError>> + Send>>)
}

impl Future for StandardFuture {
    type Output = Result<http::Response<hyper::Body>, BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            FutProj::TestConn(ready_fut) => ready_fut.poll(cx),
            FutProj::Https(fut) => fut.poll(cx).map_err(|err|err.into()),
            FutProj::Dyn(dyn_fut) => dyn_fut.poll(cx)
        }
    }
}
