/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Ready;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures_util::Future;
use http::StatusCode;
use hyper::{Body, Request, Response};
use pin_project_lite::pin_project;
use tower::Layer;
use tower::Service;

use crate::body::BoxBody;

use super::Either;

pub struct CheckHealthLayer;

impl CheckHealthLayer {
    pub fn new() -> Self {
        CheckHealthLayer
    }
}

impl<S> Layer<S> for CheckHealthLayer {
    type Service = CheckHealthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CheckHealthService { inner }
    }
}

#[derive(Clone)]
pub struct CheckHealthService<S> {
    inner: S,
}

pin_project! {
    pub struct CheckHealthFuture<E, F>{
        #[pin]
        inner: Either<PingFuture<E>, F>
    }
}

pin_project! {
    struct PingFuture<E> {
        #[pin]
        inner: Ready<Response<BoxBody>>,
        pd: PhantomData<E>
    }
}

impl<E> PingFuture<E> {
    fn new() -> Self {
        let response = Response::builder()
            .status(StatusCode::OK)
            .body(crate::body::boxed(Body::empty()))
            .expect("Couldn't construct response");

        Self {
            inner: std::future::ready(response),
            pd: PhantomData,
        }
    }
}

impl<E> Future for PingFuture<E> {
    type Output = Result<Response<BoxBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx).map(Ok)
    }
}

impl<E, F> CheckHealthFuture<E, F> {
    fn ping() -> Self {
        Self {
            inner: Either::Left {
                value: PingFuture::new(),
            },
        }
    }

    fn service_call(inner: F) -> Self {
        Self {
            inner: Either::Right { value: inner },
        }
    }
}

impl<E, F: Future<Output = Result<Response<BoxBody>, E>>> Future for CheckHealthFuture<E, F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

impl<S> Service<Request<Body>> for CheckHealthService<S>
where
    S: Service<Request<Body>, Response = Response<BoxBody>> + Clone,
    S::Future: std::marker::Send + 'static,
{
    type Response = S::Response;

    type Error = S::Error;

    type Future = CheckHealthFuture<S::Error, S::Future>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let clone = self.inner.clone();
        let mut service = std::mem::replace(&mut self.inner, clone);

        if req.uri() == "/ping" {
            CheckHealthFuture::ping()
        } else {
            CheckHealthFuture::service_call(service.call(req))
        }
    }
}
