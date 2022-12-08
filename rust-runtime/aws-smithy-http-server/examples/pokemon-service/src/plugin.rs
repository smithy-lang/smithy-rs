/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Provides an example [`Plugin`] implementation - [`PrintPlugin`].

use aws_smithy_http_server::{
    operation::{Operation, OperationShape},
    plugin::{Plugin, PluginPipeline, PluginStack},
};
use tower::{layer::util::Stack, Layer, Service};

use std::task::{Context, Poll};

/// A [`Service`] that prints a given string.
#[derive(Clone, Debug)]
pub struct PrintService<S> {
    inner: S,
    name: &'static str,
}

impl<R, S> Service<R> for PrintService<S>
where
    S: Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        println!("Hi {}", self.name);
        self.inner.call(req)
    }
}

/// A [`Layer`] which constructs the [`PrintService`].
#[derive(Debug)]
pub struct PrintLayer {
    name: &'static str,
}
impl<S> Layer<S> for PrintLayer {
    type Service = PrintService<S>;

    fn layer(&self, service: S) -> Self::Service {
        PrintService {
            inner: service,
            name: self.name,
        }
    }
}

/// A [`Plugin`] for a service builder to add a [`PrintLayer`] over operations.
#[derive(Debug)]
pub struct PrintPlugin;

impl<P, Op, S, L> Plugin<P, Op, S, L> for PrintPlugin
where
    Op: OperationShape,
{
    type Service = S;
    type Layer = Stack<L, PrintLayer>;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        input.layer(PrintLayer { name: Op::NAME })
    }
}

/// This provides a [`print`](PrintExt::print) method on [`PluginPipeline`].
pub trait PrintExt<ExistingPlugins> {
    /// Causes all operations to print the operation name when called.
    ///
    /// This works by applying the [`PrintPlugin`].
    fn print(self) -> PluginPipeline<PluginStack<PrintPlugin, ExistingPlugins>>;
}

impl<ExistingPlugins> PrintExt<ExistingPlugins> for PluginPipeline<ExistingPlugins> {
    fn print(self) -> PluginPipeline<PluginStack<PrintPlugin, ExistingPlugins>> {
        self.push(PrintPlugin)
    }
}
