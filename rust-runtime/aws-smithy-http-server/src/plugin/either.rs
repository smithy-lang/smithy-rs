/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Contains the [`Either`] enum.
//!
//! See [`Either`] documentation for more details.

use pin_project_lite::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::{Layer, Service};

use crate::operation::Operation;

use super::Plugin;

pin_project! {
    /// Combine two different [`Future`]/[`Service`]/[`Layer`]/[`Plugin`] types into a single type.
    ///
    /// # Notes on [`Future`]
    ///
    /// The [`Future::Output`] must be identical.
    ///
    /// # Notes on [`Service`]
    ///
    /// The [`Service::Response`] and [`Service::Error`] must be identical.
    #[derive(Clone, Debug)]
    #[project = EitherProj]
    pub enum Either<A, B> {
        /// One type of backing [`Service`].
        A { #[pin] value: A },
        /// The other type of backing [`Service`].
        B { #[pin] value: B },
    }
}

impl<A, B> Future for Either<A, B>
where
    A: Future,
    B: Future<Output = A::Output>,
{
    type Output = A::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EitherProj::A { value } => value.poll(cx),
            EitherProj::B { value } => value.poll(cx),
        }
    }
}

impl<A, B, Request> Service<Request> for Either<A, B>
where
    A: Service<Request>,
    B: Service<Request, Response = A::Response, Error = A::Error>,
{
    type Response = A::Response;
    type Error = A::Error;
    type Future = Either<A::Future, B::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        use self::Either::*;

        match self {
            A { value } => value.poll_ready(cx),
            B { value } => value.poll_ready(cx),
        }
    }

    fn call(&mut self, request: Request) -> Self::Future {
        use self::Either::*;

        match self {
            A { value } => Either::A {
                value: value.call(request),
            },
            B { value } => Either::B {
                value: value.call(request),
            },
        }
    }
}

impl<S, A, B> Layer<S> for Either<A, B>
where
    A: Layer<S>,
    B: Layer<S>,
{
    type Service = Either<A::Service, B::Service>;

    fn layer(&self, inner: S) -> Self::Service {
        match self {
            Either::A { value } => Either::A {
                value: value.layer(inner),
            },
            Either::B { value } => Either::B {
                value: value.layer(inner),
            },
        }
    }
}

impl<P, Op, S, L, A, B> Plugin<P, Op, S, L> for Either<A, B>
where
    A: Plugin<P, Op, S, L>,
    B: Plugin<P, Op, S, L>,
{
    type Service = Either<A::Service, B::Service>;
    type Layer = Either<A::Layer, B::Layer>;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        match self {
            Either::A { value } => {
                let Operation { inner, layer } = value.map(input);
                Operation {
                    inner: Either::A { value: inner },
                    layer: Either::A { value: layer },
                }
            }
            Either::B { value } => {
                let Operation { inner, layer } = value.map(input);
                Operation {
                    inner: Either::B { value: inner },
                    layer: Either::B { value: layer },
                }
            }
        }
    }
}
