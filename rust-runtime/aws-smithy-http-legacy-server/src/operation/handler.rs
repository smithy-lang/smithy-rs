/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    convert::Infallible,
    future::Future,
    marker::PhantomData,
    task::{Context, Poll},
};

use futures_util::{future::Map, FutureExt};
use tower::Service;

use super::OperationShape;

/// A utility trait used to provide an even interface for all operation handlers.
///
/// See [`operation`](crate::operation) documentation for more info.
pub trait Handler<Op, Exts>
where
    Op: OperationShape,
{
    type Future: Future<Output = Result<Op::Output, Op::Error>>;

    fn call(&mut self, input: Op::Input, exts: Exts) -> Self::Future;
}

/// A utility trait used to provide an even interface over return types `Result<Ok, Error>`/`Ok`.
trait IntoResult<Ok, Error> {
    fn into_result(self) -> Result<Ok, Error>;
}

// We can convert from `Result<Ok, Error>` to `Result<Ok, Error>`.
impl<Ok, Error> IntoResult<Ok, Error> for Result<Ok, Error> {
    fn into_result(self) -> Result<Ok, Error> {
        self
    }
}

// We can convert from `T` to `Result<T, Infallible>`.
impl<Ok> IntoResult<Ok, Infallible> for Ok {
    fn into_result(self) -> Result<Ok, Infallible> {
        Ok(self)
    }
}

// fn(Input) -> Output
impl<Op, F, Fut> Handler<Op, ()> for F
where
    Op: OperationShape,
    F: Fn(Op::Input) -> Fut,
    Fut: Future,
    Fut::Output: IntoResult<Op::Output, Op::Error>,
{
    type Future = Map<Fut, fn(Fut::Output) -> Result<Op::Output, Op::Error>>;

    fn call(&mut self, input: Op::Input, _exts: ()) -> Self::Future {
        (self)(input).map(IntoResult::into_result)
    }
}

// fn(Input, Ext_i) -> Output
macro_rules! impl_handler {
    ($($var:ident),+) => (
        impl<Op, F, Fut, $($var,)*> Handler<Op, ($($var,)*)> for F
        where
            Op: OperationShape,
            F: Fn(Op::Input, $($var,)*) -> Fut,
            Fut: Future,
            Fut::Output: IntoResult<Op::Output, Op::Error>,
        {
            type Future = Map<Fut, fn(Fut::Output) -> Result<Op::Output, Op::Error>>;

            fn call(&mut self, input: Op::Input, exts: ($($var,)*)) -> Self::Future {
                #[allow(non_snake_case)]
                let ($($var,)*) = exts;
                (self)(input, $($var,)*).map(IntoResult::into_result)
            }
        }
    )
}

impl_handler!(Exts0);
impl_handler!(Exts0, Exts1);
impl_handler!(Exts0, Exts1, Exts2);
impl_handler!(Exts0, Exts1, Exts2, Exts3);
impl_handler!(Exts0, Exts1, Exts2, Exts3, Exts4);
impl_handler!(Exts0, Exts1, Exts2, Exts3, Exts4, Exts5);
impl_handler!(Exts0, Exts1, Exts2, Exts3, Exts4, Exts5, Exts6);
impl_handler!(Exts0, Exts1, Exts2, Exts3, Exts4, Exts5, Exts6, Exts7);
impl_handler!(Exts0, Exts1, Exts2, Exts3, Exts4, Exts5, Exts6, Exts7, Exts8);

/// An extension trait for [`Handler`].
pub trait HandlerExt<Op, Exts>: Handler<Op, Exts>
where
    Op: OperationShape,
{
    /// Convert the [`Handler`] into a [`Service`].
    fn into_service(self) -> IntoService<Op, Self>
    where
        Self: Sized,
    {
        IntoService {
            handler: self,
            _operation: PhantomData,
        }
    }
}

impl<Op, Exts, H> HandlerExt<Op, Exts> for H
where
    Op: OperationShape,
    H: Handler<Op, Exts>,
{
}

/// A [`Service`] provided for every [`Handler`].
pub struct IntoService<Op, H> {
    pub(crate) handler: H,
    pub(crate) _operation: PhantomData<Op>,
}

impl<Op, H> Clone for IntoService<Op, H>
where
    H: Clone,
{
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            _operation: PhantomData,
        }
    }
}

impl<Op, Exts, H> Service<(Op::Input, Exts)> for IntoService<Op, H>
where
    Op: OperationShape,
    H: Handler<Op, Exts>,
{
    type Response = Op::Output;
    type Error = Op::Error;
    type Future = H::Future;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, (input, exts): (Op::Input, Exts)) -> Self::Future {
        self.handler.call(input, exts)
    }
}
