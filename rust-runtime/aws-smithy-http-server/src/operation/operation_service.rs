/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    marker::PhantomData,
    task::{Context, Poll},
};

use tower::Service;

use super::{OperationError, OperationShape};

/// A utility trait used to provide an even interface for all operation services.
///
/// This serves to take [`Service`]s of the form `Service<(Op::Input, Ext0, Ext1, ...)>` to the canonical
/// representation of `Service<(Input, (Ext0, Ext1, ...))>` inline with
/// [`IntoService`](super::IntoService).
///
/// See [`operation`](crate::operation) documentation for more info.
pub trait OperationService<Op, Exts, PollError>:
    Service<Self::Normalized, Response = Op::Output, Error = OperationError<Op::Error, PollError>>
where
    Op: OperationShape,
{
    type Normalized;

    // Normalize the request type.
    fn normalize(input: Op::Input, exts: Exts) -> Self::Normalized;
}

// `Service<Op::Input>`
impl<Op, S, PollError> OperationService<Op, (), PollError> for S
where
    Op: OperationShape,
    S: Service<Op::Input, Response = Op::Output, Error = OperationError<Op::Error, PollError>>,
{
    type Normalized = Op::Input;

    fn normalize(input: Op::Input, _exts: ()) -> Self::Normalized {
        input
    }
}

// `Service<(Op::Input, Ext0)>`
impl<Op, Ext0, S, PollError> OperationService<Op, (Ext0,), PollError> for S
where
    Op: OperationShape,
    S: Service<(Op::Input, Ext0), Response = Op::Output, Error = OperationError<Op::Error, PollError>>,
{
    type Normalized = (Op::Input, Ext0);

    fn normalize(input: Op::Input, exts: (Ext0,)) -> Self::Normalized {
        (input, exts.0)
    }
}

// `Service<(Op::Input, Ext0, Ext1)>`
impl<Op, Ext0, Ext1, S, PollError> OperationService<Op, (Ext0, Ext1), PollError> for S
where
    Op: OperationShape,
    S: Service<(Op::Input, Ext0, Ext1), Response = Op::Output, Error = OperationError<Op::Error, PollError>>,
{
    type Normalized = (Op::Input, Ext0, Ext1);

    fn normalize(input: Op::Input, exts: (Ext0, Ext1)) -> Self::Normalized {
        (input, exts.0, exts.1)
    }
}

/// An extension trait of [`OperationService`].
pub trait OperationServiceExt<Op, Exts, PollError>: OperationService<Op, Exts, PollError>
where
    Op: OperationShape,
{
    /// Convert the [`OperationService`] into a canonicalized [`Service`].
    fn canonicalize(self) -> Normalize<Op, Self, PollError>
    where
        Self: Sized,
    {
        Normalize {
            inner: self,
            _operation: PhantomData,
            _poll_error: PhantomData,
        }
    }
}

impl<F, Op, Exts, PollError> OperationServiceExt<Op, Exts, PollError> for F
where
    Op: OperationShape,
    F: OperationService<Op, Exts, PollError>,
{
}

/// A [`Service`] normalizing the request type of a [`OperationService`].
#[derive(Debug)]
pub struct Normalize<Op, S, PollError> {
    inner: S,
    _operation: PhantomData<Op>,
    _poll_error: PhantomData<PollError>,
}

impl<Op, S, PollError> Clone for Normalize<Op, S, PollError>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _operation: PhantomData,
            _poll_error: PhantomData,
        }
    }
}

impl<Op, S, Exts, PollError> Service<(Op::Input, Exts)> for Normalize<Op, S, PollError>
where
    Op: OperationShape,
    S: OperationService<Op, Exts, PollError>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = <S as Service<S::Normalized>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, (input, exts): (Op::Input, Exts)) -> Self::Future {
        let req = S::normalize(input, exts);
        self.inner.call(req)
    }
}
