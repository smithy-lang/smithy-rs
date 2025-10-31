/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    marker::PhantomData,
    task::{Context, Poll},
};

use tower::Service;

use super::OperationShape;

/// A utility trait used to provide an even interface for all operation services.
///
/// This serves to take [`Service`]s of the form `Service<(Op::Input, Ext0, Ext1, ...)>` to the canonical
/// representation of `Service<(Input, (Ext0, Ext1, ...))>` inline with
/// [`IntoService`](super::IntoService).
///
/// See [`operation`](crate::operation) documentation for more info.
pub trait OperationService<Op, Exts>: Service<Self::Normalized, Response = Op::Output, Error = Op::Error>
where
    Op: OperationShape,
{
    type Normalized;

    // Normalize the request type.
    fn normalize(input: Op::Input, exts: Exts) -> Self::Normalized;
}

// `Service<Op::Input>`
impl<Op, S> OperationService<Op, ()> for S
where
    Op: OperationShape,
    S: Service<Op::Input, Response = Op::Output, Error = Op::Error>,
{
    type Normalized = Op::Input;

    fn normalize(input: Op::Input, _exts: ()) -> Self::Normalized {
        input
    }
}

// `Service<(Op::Input, Ext0)>`
impl<Op, Ext0, S> OperationService<Op, (Ext0,)> for S
where
    Op: OperationShape,
    S: Service<(Op::Input, Ext0), Response = Op::Output, Error = Op::Error>,
{
    type Normalized = (Op::Input, Ext0);

    fn normalize(input: Op::Input, exts: (Ext0,)) -> Self::Normalized {
        (input, exts.0)
    }
}

// `Service<(Op::Input, Ext0, Ext1)>`
impl<Op, Ext0, Ext1, S> OperationService<Op, (Ext0, Ext1)> for S
where
    Op: OperationShape,
    S: Service<(Op::Input, Ext0, Ext1), Response = Op::Output, Error = Op::Error>,
{
    type Normalized = (Op::Input, Ext0, Ext1);

    fn normalize(input: Op::Input, exts: (Ext0, Ext1)) -> Self::Normalized {
        (input, exts.0, exts.1)
    }
}

/// An extension trait of [`OperationService`].
pub trait OperationServiceExt<Op, Exts>: OperationService<Op, Exts>
where
    Op: OperationShape,
{
    /// Convert the [`OperationService`] into a canonicalized [`Service`].
    fn normalize(self) -> Normalize<Op, Self>
    where
        Self: Sized,
    {
        Normalize {
            inner: self,
            _operation: PhantomData,
        }
    }
}

impl<F, Op, Exts> OperationServiceExt<Op, Exts> for F
where
    Op: OperationShape,
    F: OperationService<Op, Exts>,
{
}

/// A [`Service`] normalizing the request type of a [`OperationService`].
#[derive(Debug)]
pub struct Normalize<Op, S> {
    pub(crate) inner: S,
    pub(crate) _operation: PhantomData<Op>,
}

impl<Op, S> Clone for Normalize<Op, S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _operation: PhantomData,
        }
    }
}

impl<Op, S, Exts> Service<(Op::Input, Exts)> for Normalize<Op, S>
where
    Op: OperationShape,
    S: OperationService<Op, Exts>,
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
