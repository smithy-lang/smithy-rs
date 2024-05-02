/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use super::{Handler, IntoService, Normalize, OperationService};
use crate::shape_id::ShapeId;

/// Models the [Smithy Operation shape].
///
/// [Smithy Operation shape]: https://smithy.io/2.0/spec/service-types.html#operation
pub trait OperationShape {
    /// The ID of the operation.
    const ID: ShapeId;

    /// The operation input.
    type Input;
    /// The operation output.
    type Output;
    /// The operation error. [`Infallible`](std::convert::Infallible) in the case where no error
    /// exists.
    type Error;
}

/// An extension trait over [`OperationShape`].
pub trait OperationShapeExt: OperationShape {
    /// Creates a new [`Service`](tower::Service), [`IntoService`], for well-formed [`Handler`]s.
    fn from_handler<H, Exts>(handler: H) -> IntoService<Self, H>
    where
        H: Handler<Self, Exts>,
        Self: Sized,
    {
        IntoService {
            handler,
            _operation: PhantomData,
        }
    }

    /// Creates a new normalized [`Service`](tower::Service), [`Normalize`], for well-formed
    /// [`Service`](tower::Service)s.
    fn from_service<S, Exts>(svc: S) -> Normalize<Self, S>
    where
        S: OperationService<Self, Exts>,
        Self: Sized,
    {
        Normalize {
            inner: svc,
            _operation: PhantomData,
        }
    }
}

impl<S> OperationShapeExt for S where S: OperationShape {}
