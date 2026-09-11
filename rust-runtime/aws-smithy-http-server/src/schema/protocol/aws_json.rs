/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! awsJson1.0 and awsJson1.1 share a codec, a rejection type and a runtime error; they differ in
//! the content type and in whether the `__type` discriminator is the full shape ID or the name.

use aws_smithy_json::codec::JsonCodec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, ShapeId};

use crate::protocol::aws_json::rejection::RequestRejection;
use crate::protocol::aws_json::runtime_error::RuntimeError;
use crate::protocol::aws_json_10::{AwsJson1_0, AwsJson1_0Protocol};
use crate::protocol::aws_json_11::{AwsJson1_1, AwsJson1_1Protocol};
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::discriminator::{BodyDiscriminator, TypePosition, TypeValue};
use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, stamp_validation_extension,
    ResponseBindings,
};
use super::rpc::RpcProtocolProvider;
use super::{CompiledOperation, RpcOperationState, ServerProtocol, ServerRequest};

fn serialize_error<P>(
    codec: &JsonCodec,
    error: &dyn HttpModeledError,
    content_type: &'static str,
    discriminator: BodyDiscriminator,
) -> Response
where
    RuntimeError: IntoResponse<P>,
{
    let schema = error.schema();
    let framed = discriminator.frame(schema, error);
    serialize_modeled_error_response(
        codec,
        schema,
        &framed,
        error.status_code(),
        ResponseBindings::BodyOnly,
        content_type,
    )
    .map(|response| stamp_error_extension(response, schema.shape_id().shape_name()))
    .unwrap_or_else(serialization_failure::<P>)
}

fn serialization_failure<P>(err: SerdeError) -> Response
where
    RuntimeError: IntoResponse<P>,
{
    log_serialize_failure(&err);
    IntoResponse::<P>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}

macro_rules! aws_json_protocol {
    ($protocol:ty, $marker:ty, $protocol_id:expr, $content_type:literal, $type_value:expr) => {
        impl RpcProtocolProvider for $protocol {
            type RpcCodec = JsonCodec;

            fn rpc_protocol(&self) -> &super::rpc::RpcProtocol<JsonCodec> {
                &self.inner
            }
        }

        impl ServerProtocol for $protocol {
            type Codec = JsonCodec;
            type OperationState = RpcOperationState;

            fn protocol_id(&self) -> &'static ShapeId<'static> {
                static PROTOCOL_ID: ShapeId<'static> = $protocol_id;
                &PROTOCOL_ID
            }

            fn codec(&self) -> &JsonCodec {
                self.inner.codec()
            }

            fn reads_request_body(&self, operation: &CompiledOperation<RpcOperationState>) -> bool {
                operation.state().reads_body()
            }

            fn deserialize_request<'a>(
                &'a self,
                operation: &'a CompiledOperation<RpcOperationState>,
                request: &'a ServerRequest,
            ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
                self.inner
                    .deserialize_request(operation.state(), operation.schema().input(), request)
            }

            fn serialize_response(
                &self,
                operation: &CompiledOperation<RpcOperationState>,
                output: &dyn SerializableStruct,
            ) -> Response {
                self.inner
                    .serialize_response(operation.schema(), output)
                    .unwrap_or_else(serialization_failure::<$marker>)
            }

            fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
                serialize_error::<$marker>(
                    self.codec(),
                    error,
                    $content_type,
                    BodyDiscriminator {
                        position: TypePosition::Last,
                        value: $type_value,
                    },
                )
            }

            /// awsJson's `From<RequestRejection>` collapses every transport failure — `Accept`
            /// and `Content-Type` mismatches included — into a 400 `Serialization`; routing
            /// through that same `From` preserves the collapse. A constraint violation answers
            /// with the modeled validation error.
            fn serialize_rejection(&self, err: DeserializeError) -> Response {
                match err {
                    DeserializeError::Serde(err) => {
                        IntoResponse::<$marker>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
                    }
                    DeserializeError::UnsupportedMediaType(reason) => IntoResponse::<$marker>::into_response(
                        RuntimeError::from(RequestRejection::MissingContentType(*reason)),
                    ),
                    DeserializeError::NotAcceptable => {
                        IntoResponse::<$marker>::into_response(RuntimeError::from(RequestRejection::NotAcceptable))
                    }
                    DeserializeError::ConstraintViolation(err) => {
                        stamp_validation_extension(self.serialize_error(&*err))
                    }
                }
            }
        }
    };
}

aws_json_protocol!(
    AwsJson1_0Protocol,
    AwsJson1_0,
    shape_id!("aws.protocols", "awsJson1_0"),
    "application/x-amz-json-1.0",
    TypeValue::FullShapeId
);
aws_json_protocol!(
    AwsJson1_1Protocol,
    AwsJson1_1,
    shape_id!("aws.protocols", "awsJson1_1"),
    "application/x-amz-json-1.1",
    TypeValue::ShapeName
);
