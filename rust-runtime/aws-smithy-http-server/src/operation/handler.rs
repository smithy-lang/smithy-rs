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

use futures_util::{
    future::{Map, MapErr},
    FutureExt, TryFutureExt,
};
use tower::Service;

use super::{OperationError, OperationShape};

/// A utility trait used to provide an even interface for all handlers.
pub trait Handler<Op, Exts>
where
    Op: OperationShape,
{
    type Future: Future<Output = Result<Op::Output, Op::Error>>;

    fn call(&mut self, input: Op::Input, exts: Exts) -> Self::Future;
}

/// A utility trait used to provide an even interface over return types `Result<Ok, Error>`/`Ok`.
trait ToResult<Ok, Error> {
    fn into_result(self) -> Result<Ok, Error>;
}

// We can convert from `Result<Ok, Error>` to `Result<Ok, Error>`.
impl<Ok, Error> ToResult<Ok, Error> for Result<Ok, Error> {
    fn into_result(self) -> Result<Ok, Error> {
        self
    }
}

// We can convert from `Ok` to `Result<Ok, Error>`.
impl<Ok> ToResult<Ok, Infallible> for Ok {
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
    Fut::Output: ToResult<Op::Output, Op::Error>,
{
    type Future = Map<Fut, fn(Fut::Output) -> Result<Op::Output, Op::Error>>;

    fn call(&mut self, input: Op::Input, _exts: ()) -> Self::Future {
        (self)(input).map(ToResult::into_result)
    }
}

// fn(Input, Ext0) -> Output
impl<Op, F, Fut, Ext0> Handler<Op, (Ext0,)> for F
where
    Op: OperationShape,
    F: Fn(Op::Input, Ext0) -> Fut,
    Fut: Future,
    Fut::Output: ToResult<Op::Output, Op::Error>,
{
    type Future = Map<Fut, fn(Fut::Output) -> Result<Op::Output, Op::Error>>;

    fn call(&mut self, input: Op::Input, exts: (Ext0,)) -> Self::Future {
        (self)(input, exts.0).map(ToResult::into_result)
    }
}

// fn(Input, Ext0, Ext1) -> Output
impl<Op, F, Fut, Ext0, Ext1> Handler<Op, (Ext0, Ext1)> for F
where
    Op: OperationShape,
    F: Fn(Op::Input, Ext0, Ext1) -> Fut,
    Fut: Future,
    Fut::Output: ToResult<Op::Output, Op::Error>,
{
    type Future = Map<Fut, fn(Fut::Output) -> Result<Op::Output, Op::Error>>;

    fn call(&mut self, input: Op::Input, exts: (Ext0, Ext1)) -> Self::Future {
        (self)(input, exts.0, exts.1).map(ToResult::into_result)
    }
}

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
    handler: H,
    _operation: PhantomData<Op>,
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
    type Error = OperationError<Op::Error, Infallible>;
    type Future = MapErr<H::Future, fn(Op::Error) -> Self::Error>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, (input, exts): (Op::Input, Exts)) -> Self::Future {
        self.handler.call(input, exts).map_err(OperationError::Model)
    }
}
