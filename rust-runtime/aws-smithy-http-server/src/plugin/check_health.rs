/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Ready;
use std::task::{Context, Poll};

use futures_util::Future;
use http::StatusCode;
use hyper::{Body, Request, Response};
use tower::{util::Oneshot, Layer, Service, ServiceExt};

use crate::body;
use crate::body::BoxBody;

use super::Either;

#[derive(Clone)]
pub struct CheckHealthLayer<H> {
    ping_handler: H,
}

impl<H> CheckHealthLayer<H> {
    pub fn new(ping_handler: H) -> Self {
        CheckHealthLayer { ping_handler }
    }
}

pub type DefaultHandler<E> = fn(Request<Body>) -> Ready<Result<Response<BoxBody>, E>>;

impl CheckHealthLayer<()> {
    pub fn with_default_handler<E>() -> CheckHealthLayer<DefaultHandler<E>> {
        CheckHealthLayer::new(default_ping_handler)
    }
}

impl<S, H: Clone> Layer<S> for CheckHealthLayer<H> {
    type Service = CheckHealthService<H, S>;

    fn layer(&self, inner: S) -> Self::Service {
        CheckHealthService {
            inner,
            layer: self.clone(),
        }
    }
}

#[derive(Clone)]
pub struct CheckHealthService<H, S> {
    inner: S,
    layer: CheckHealthLayer<H>,
}

impl<H, HandlerFuture, S> Service<Request<Body>> for CheckHealthService<H, S>
where
    S: Service<Request<Body>, Response = Response<BoxBody>> + Clone,
    S::Future: std::marker::Send + 'static,
    HandlerFuture: Future<Output = Result<Response<BoxBody>, S::Error>>,
    H: Fn(Request<Body>) -> HandlerFuture,
{
    type Response = S::Response;

    type Error = S::Error;

    type Future = Either<HandlerFuture, Oneshot<S, Request<Body>>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // The check that the service is ready is done by `Oneshot` below.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        if req.uri() == "/ping" {
            Either::Left {
                value: (self.layer.ping_handler)(req),
            }
        } else {
            let clone = self.inner.clone();
            let service = std::mem::replace(&mut self.inner, clone);

            Either::Right {
                value: service.oneshot(req),
            }
        }
    }
}

/// A handler that returns `200 OK` with an empty body.
fn default_ping_handler<E>(_req: Request<Body>) -> Ready<Result<Response<BoxBody>, E>> {
    let response = Response::builder()
        .status(StatusCode::OK)
        .body(body::boxed(Body::empty()))
        .expect("Couldn't construct response");

    std::future::ready(Ok::<_, E>(response))
}
