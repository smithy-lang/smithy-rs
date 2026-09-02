/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::marker::PhantomData;

use super::{Handler, IntoService, Normalize, OperationService};
use crate::{
    body::BoxBody,
    modeled_error::HttpServerError,
    schema::{protocol::ServerProtocol, OperationSchema},
    shape_id::ShapeId,
};

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

/// Operation marker extension for generated schema metadata.
pub trait SchemaOperationShape: OperationShape {
    /// Runtime schema descriptor for this operation.
    const SCHEMA: &'static OperationSchema<'static>;
}

/// Output values that can be serialized through an erased server protocol.
pub trait DynOutput: aws_smithy_schema::serde::SerializableStruct {
    /// Returns this output shape's schema.
    fn schema(&self) -> &aws_smithy_schema::Schema<'_>;
}

/// Operation errors that can be serialized through an erased server protocol.
pub trait IntoDynProtocolResponse {
    /// Converts this operation error into an HTTP response using the selected protocol.
    fn into_dyn_response(self, protocol: &dyn ServerProtocol) -> http::Response<BoxBody>;
}

impl IntoDynProtocolResponse for std::convert::Infallible {
    fn into_dyn_response(self, _protocol: &dyn ServerProtocol) -> http::Response<BoxBody> {
        match self {}
    }
}

impl<T> IntoDynProtocolResponse for T
where
    T: HttpServerError + 'static,
{
    fn into_dyn_response(self, protocol: &dyn ServerProtocol) -> http::Response<BoxBody> {
        protocol.serialize_error(&self)
    }
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
