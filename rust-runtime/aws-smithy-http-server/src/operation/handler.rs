/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    convert::Infallible,
    future::Future,
    task::{Context, Poll},
};

use futures_util::{future::Map, FutureExt};
use tower::Service;

/// A utility trait used to provide an even interface for all operation handlers.
///
/// See [`operation`](crate::operation) documentation for more info.
pub trait Handler<Input>: Clone + Send + 'static {
    type Future: Future;

    fn call(self, request: Input) -> Self::Future;
}

macro_rules! impl_handler {
    ($($ty: ident),*) => {
        impl<$($ty,)* F, Fut> Handler<($($ty,)*)> for F
        where
            F: Fn($($ty),*) -> Fut,
            F: Clone + Send + 'static,
            Fut: Future,
        {
            type Future = Fut;

            #[allow(non_snake_case)]
            fn call(self, ($($ty,)*): ($($ty,)*)) -> Self::Future {
                self($($ty),*)
            }
        }
    };
}

impl_handler!(T0);
impl_handler!(T0, T2);
impl_handler!(T0, T2, T3);
impl_handler!(T0, T2, T3, T4);
impl_handler!(T0, T2, T3, T4, T5);
impl_handler!(T0, T2, T3, T4, T5, T6);
impl_handler!(T0, T2, T3, T4, T5, T6, T7);
impl_handler!(T0, T2, T3, T4, T5, T6, T7, T8);
impl_handler!(T0, T2, T3, T4, T5, T6, T7, T8, T9);

/// A [`Service`] provided for every [`Handler`].
#[derive(Debug, Clone)]
pub struct IntoService<H> {
    handler: H,
}

impl<Input, H> Service<Input> for IntoService<H>
where
    H: Handler<Input>,
    H::Future: Future,
{
    type Response = <H::Future as Future>::Output;
    type Error = Infallible;
    type Future = Map<H::Future, fn(Self::Response) -> Result<Self::Response, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Input) -> Self::Future {
        self.handler.clone().call(request).map(Ok)
    }
}

impl<H> IntoService<H> {
    pub fn new(handler: H) -> Self {
        Self { handler }
    }
}
