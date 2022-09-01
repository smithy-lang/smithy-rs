/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{Handler, IntoService, Normalize, Operation, OperationService};

/// Models the [Smithy Operation shape].
///
/// [Smithy Operation shape]: https://awslabs.github.io/smithy/1.0/spec/core/model.html#operation
pub trait OperationShape {
    /// The name of the operation.
    const NAME: &'static str;

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
    /// Creates a new [`Operation`] for well-formed [`Handler`]s.
    fn from_handler<H, Exts>(handler: H) -> Operation<IntoService<Self, H>>
    where
        H: Handler<Self, Exts>,
        Self: Sized,
    {
        Operation::from_handler(handler)
    }

    /// Creates a new [`Operation`] for well-formed [`Service`](tower::Service)s.
    fn from_service<S, Exts, PollError>(svc: S) -> Operation<Normalize<Self, S, PollError>>
    where
        S: OperationService<Self, Exts, PollError>,
        Self: Sized,
    {
        Operation::from_service(svc)
    }
}

impl<S> OperationShapeExt for S where S: OperationShape {}
