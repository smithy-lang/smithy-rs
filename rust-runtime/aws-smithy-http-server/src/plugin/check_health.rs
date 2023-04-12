/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Ready;
use std::task::{Context, Poll};

use futures_util::Future;
use http::StatusCode;
use hyper::{Body, Request, Response};
use tower::layer::util::Stack;
use tower::{util::Oneshot, Layer, Service, ServiceExt};

use crate::body;
use crate::body::BoxBody;
use crate::operation::Operation;

use super::{Either, Plugin, PluginPipeline, PluginStack};

/// A [`tower::Layer`] used to apply [`CheckHealthService`].
#[derive(Clone, Debug)]
pub struct CheckHealthLayer<PingHandler> {
    health_check_uri: &'static str,
    ping_handler: PingHandler,
}

impl<H> CheckHealthLayer<H> {
    pub fn new(health_check_uri: &'static str, ping_handler: H) -> Self {
        CheckHealthLayer {
            health_check_uri,
            ping_handler,
        }
    }
}

pub type DefaultHandler<E> = fn(Request<Body>) -> Ready<Result<Response<BoxBody>, E>>;

impl CheckHealthLayer<()> {
    pub fn with_default_handler<E>() -> CheckHealthLayer<DefaultHandler<E>> {
        const DEFAULT_HEALTH_CHECK_URI: &str = "/ping";
        CheckHealthLayer::new(DEFAULT_HEALTH_CHECK_URI, default_ping_handler)
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

/// A middleware [`Service`] responsible for handling health check requests.
#[derive(Clone, Debug)]
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
        if req.uri() == self.layer.health_check_uri {
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

#[derive(Debug)]
pub struct CheckHealthPlugin<H> {
    layer: CheckHealthLayer<H>,
}

impl<H> CheckHealthPlugin<H> {
    fn new(health_check_uri: &'static str, ping_handler: H) -> Self {
        CheckHealthPlugin {
            layer: CheckHealthLayer::new(health_check_uri, ping_handler),
        }
    }
}

impl<Protocol, Op, S, L, H> Plugin<Protocol, Op, S, L> for CheckHealthPlugin<H>
where
    H: Clone,
{
    type Service = S;

    type Layer = Stack<L, CheckHealthLayer<H>>;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        input.layer(self.layer.clone())
    }
}

/// An extension trait for applying [`CheckHealthLayer`] to all operations in a service.
pub trait CheckHealthExt<H, CurrentPlugins> {
    /// Applies a [`CheckHealthLayer`] to all operations.
    fn check_health(
        self,
        health_check_uri: &'static str,
        ping_handler: H,
    ) -> PluginPipeline<PluginStack<CheckHealthPlugin<H>, CurrentPlugins>>;
}

impl<H, CurrentPlugins> CheckHealthExt<H, CurrentPlugins> for PluginPipeline<CurrentPlugins> {
    fn check_health(
        self,
        health_check_uri: &'static str,
        ping_handler: H,
    ) -> PluginPipeline<PluginStack<CheckHealthPlugin<H>, CurrentPlugins>> {
        self.push(CheckHealthPlugin::new(health_check_uri, ping_handler))
    }
}
