/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use super::{Handler, IntoService, Normalize, OperationService};
use crate::response::Response;
use crate::schema::{DeserializeError, SharedServerProtocol};
use crate::shape_id::ShapeId;
use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::OperationSchema;
use aws_smithy_types::body::SdkBody;

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

/// Associates a generated operation marker with its schema descriptor.
pub trait SchemaOperationShape: OperationShape {
    const SCHEMA: &'static OperationSchema<'static>;
}

/// The future returned by [`StreamingOperationShape::deserialize_streaming_input`].
pub type StreamingInputFuture<I> = Pin<Box<dyn Future<Output = Result<I, DeserializeError>> + Send>>;

/// The generated glue between the HTTP bodies and the streaming members of an operation with an
/// event stream or streaming blob on either side.
///
/// Both halves work through the erased protocol handle selected by routing: the generated
/// marshallers and unmarshallers ask it for the payload codec and the event media type, and
/// [`ServerEventStreamProtocol::initial_messages_in_frames`](crate::schema::ServerEventStreamProtocol::initial_messages_in_frames) decides at runtime whether the non-stream
/// members travel in `initial-request` and `initial-response` frames.
///
/// An operation that streams on one side only implements the other half in terms of the
/// collected path: [`DeserializableShape`](crate::schema::DeserializableShape) for the input,
/// [`ServerProtocol::serialize_response`](crate::schema::ServerProtocol::serialize_response) for the output.
pub trait StreamingOperationShape: SchemaOperationShape {
    /// Reads the input: the HTTP bindings from `deserializer`, the streaming member from `body`.
    ///
    /// `body` is the live request body when the input streams and an empty body otherwise. The
    /// walk over `deserializer` happens before the returned future is polled; the future reads
    /// the initial frame when the protocol carries one.
    fn deserialize_streaming_input(
        deserializer: &mut dyn ShapeDeserializer,
        body: SdkBody,
        protocol: SharedServerProtocol,
    ) -> StreamingInputFuture<Self::Input>;

    /// Serializes the output: the streaming member becomes the body, the rest the response head.
    fn serialize_streaming_output(output: Self::Output, protocol: &SharedServerProtocol) -> Response;
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
