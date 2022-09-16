/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::plugin::Plugin;

/// A [`Service`](tower::Service) that adds a print log.
#[derive(Clone, Debug)]
pub struct PrintService<S> {
    inner: S,
    name: &'static str,
}

impl<R, S> tower::Service<R> for PrintService<S>
where
    S: tower::Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        println!("Hi {}", self.name);
        self.inner.call(req)
    }
}

/// A [`Layer`](tower::Layer) which constructs the [`PrintService`].
#[derive(Debug)]
pub struct PrintLayer {
    name: &'static str,
}
impl<S> tower::Layer<S> for PrintLayer {
    type Service = PrintService<S>;

    fn layer(&self, service: S) -> Self::Service {
        PrintService {
            inner: service,
            name: self.name,
        }
    }
}

/// A [`Plugin`]() for a service builder to add a [`PrintLayer`] over operations.
#[derive(Debug)]
pub struct PrintPlugin;
impl<P, Op, S, L> Plugin<P, Op, S, L> for PrintPlugin
where
    Op: aws_smithy_http_server::operation::OperationShape,
{
    type Service = S;
    type Layer = tower::layer::util::Stack<L, PrintLayer>;

    fn map(
        &self,
        input: aws_smithy_http_server::operation::Operation<S, L>,
    ) -> aws_smithy_http_server::operation::Operation<Self::Service, Self::Layer> {
        input.layer(PrintLayer { name: Op::NAME })
    }
}

/// An extension to service builders to add the `print()` function.
pub trait PrintExt: aws_smithy_http_server::plugin::Pluggable<PrintPlugin> {
    /// Causes all operations to print the operation name when called.
    ///
    /// This works by applying the [`PrintPlugin`].
    fn print(self) -> Self::Output
    where
        Self: Sized,
    {
        self.apply(PrintPlugin)
    }
}

impl<Builder> PrintExt for Builder where Builder: aws_smithy_http_server::plugin::Pluggable<PrintPlugin> {}
