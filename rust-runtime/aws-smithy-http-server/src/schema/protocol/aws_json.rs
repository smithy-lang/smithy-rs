/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! awsJson1.0 and awsJson1.1 share a codec, a rejection type and a runtime error; they differ in
//! the content type and in whether the `__type` discriminator is the full shape ID or the name.

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::aws_json::rejection::RequestRejection;
use crate::protocol::aws_json::runtime_error::RuntimeError;
use crate::protocol::aws_json_10::AwsJson1_0;
use crate::protocol::aws_json_11::AwsJson1_1;
use crate::response::{IntoResponse, Response};
use crate::schema::response_bindings::{serialize_response_parts, ResponseValueKind};
use crate::schema::{DeserializeError, HttpModeledError};

use super::discriminator::{BodyDiscriminator, TypeValue};
use super::response::{
    assemble_response, assemble_streaming_response, log_serialize_failure, resolve_status,
    serialize_modeled_error_response, stamp_error_extension, stamp_validation_extension, ResponseBindings,
};
use super::{BodyDirective, EventStreamFraming, MetadataRoutedProtocol, ServerProtocol};

/// Stateful schema-driven AWS JSON 1.0 protocol implementation.
#[derive(Debug)]
/// Defaults to validating escape syntax in skipped strings without decoding Unicode.
/// Set `customizationConfig.protocols` for this protocol to `{"validateSkippedValues":true}`
/// to skip escape validation, matching legacy smithy-rs servers.
///
/// `responseBindings` defaults to `"historical"`, which keeps what smithy-rs awsJson servers have
/// always sent: the operation's `@http(code)`, `@httpError` statuses and `@httpHeader` members
/// as response headers (each also stays in the document). Set it to `"protocol"` in this
/// protocol's settings for the AWS JSON specification instead: always 200 (errors use the
/// `@error` class status) with every member in the document and no REST headers.
pub struct AwsJson1_0Protocol {
    pub(crate) inner: crate::schema::protocol::rpc::RpcProtocol<aws_smithy_json::codec::JsonCodec>,
    historical_response_bindings: bool,
}

impl Default for AwsJson1_0Protocol {
    fn default() -> Self {
        Self::new(false, true)
    }
}

impl AwsJson1_0Protocol {
    fn new(validate_skipped_values: bool, historical_response_bindings: bool) -> Self {
        Self {
            historical_response_bindings,
            inner: crate::schema::protocol::rpc::RpcProtocol::new(
                schema_codec(validate_skipped_values),
                "application/x-amz-json-1.0",
                Some("application/x-amz-json-1.0"),
                crate::schema::protocol::rpc::RpcAccept::Always,
                crate::schema::protocol::rpc::RpcStreaming::CodecContentType,
            ),
        }
    }
}

/// Stateful schema-driven AWS JSON 1.1 protocol implementation.
#[derive(Debug)]
/// Defaults to validating escape syntax in skipped strings without decoding Unicode.
/// Set `customizationConfig.protocols` for this protocol to `{"validateSkippedValues":true}`
/// to skip escape validation, matching legacy smithy-rs servers.
///
/// `responseBindings` defaults to `"historical"`, which keeps what smithy-rs awsJson servers have
/// always sent: the operation's `@http(code)`, `@httpError` statuses and `@httpHeader` members
/// as response headers (each also stays in the document). Set it to `"protocol"` in this
/// protocol's settings for the AWS JSON specification instead: always 200 (errors use the
/// `@error` class status) with every member in the document and no REST headers.
pub struct AwsJson1_1Protocol {
    pub(crate) inner: crate::schema::protocol::rpc::RpcProtocol<aws_smithy_json::codec::JsonCodec>,
    historical_response_bindings: bool,
}

impl Default for AwsJson1_1Protocol {
    fn default() -> Self {
        Self::new(false, true)
    }
}

impl AwsJson1_1Protocol {
    fn new(validate_skipped_values: bool, historical_response_bindings: bool) -> Self {
        Self {
            historical_response_bindings,
            inner: crate::schema::protocol::rpc::RpcProtocol::new(
                schema_codec(validate_skipped_values),
                "application/x-amz-json-1.1",
                Some("application/x-amz-json-1.1"),
                crate::schema::protocol::rpc::RpcAccept::Always,
                crate::schema::protocol::rpc::RpcStreaming::CodecContentType,
            ),
        }
    }
}

/// Reads the `responseBindings` setting: `"historical"` (the default) or `"protocol"`.
fn historical_response_bindings(
    settings: Option<&aws_smithy_types::Document>,
) -> Result<bool, crate::schema::routing::RouterBuildError> {
    match crate::schema::settings::get::<&str>(settings, "responseBindings")? {
        None | Some("historical") => Ok(true),
        Some("protocol") => Ok(false),
        Some(other) => Err(crate::schema::routing::RouterBuildError::Configuration(format!(
            "setting `responseBindings` must be \"historical\" or \"protocol\", got {other:?}"
        ))),
    }
}

fn schema_codec(validate_skipped_values: bool) -> aws_smithy_json::codec::JsonCodec {
    aws_smithy_json::codec::JsonCodec::new(
        aws_smithy_json::codec::JsonCodecSettings::builder()
            .use_json_name(false)
            .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
            .enforce_strictness(true)
            .allow_leading_zeros(true)
            .allow_trailing_decimal_point(true)
            .normalize_arbitrary_precision(true)
            .validate_skipped_values(validate_skipped_values)
            .validate_skipped_string_encoding(true)
            .allow_integral_float_numbers(true)
            .strict_timestamp_formats(true)
            .build(),
    )
}

// AWS JSON uses @error classification, independently of the REST @httpError override.
fn classification_status(schema: &Schema<'_>) -> Result<u16, SerdeError> {
    let classification = schema
        .traits()
        .and_then(|traits| traits.get(&shape_id!("smithy.api", "error")))
        .and_then(|value| value.as_any().downcast_ref::<aws_smithy_schema::StringTrait>())
        .map(|value| value.value());
    match classification {
        Some("client") => Ok(400),
        Some("server") => Ok(500),
        _ => Err(SerdeError::custom("missing or invalid @error trait")),
    }
}

fn serialize_error<P>(
    codec: &aws_smithy_json::codec::JsonCodec,
    error: &dyn HttpModeledError,
    content_type: &'static str,
    discriminator: BodyDiscriminator,
    historical_response_bindings: bool,
) -> Response
where
    RuntimeError: IntoResponse<P>,
{
    let schema = error.schema();
    let status = if historical_response_bindings {
        error.status_code()
    } else {
        match classification_status(schema) {
            Ok(status) => status,
            Err(error) => return serialization_failure::<P>(error),
        }
    };
    let framed = discriminator.frame(schema, error);
    let bindings = if historical_response_bindings {
        ResponseBindings::LegacyRpc
    } else {
        ResponseBindings::BodyOnly
    };
    serialize_modeled_error_response(codec, schema, &framed, status, bindings, content_type)
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
        impl MetadataRoutedProtocol for $protocol {
            fn from_build_context(
                ctx: &crate::schema::ProtocolBuildContext<'_>,
            ) -> Result<Self, crate::schema::routing::RouterBuildError> {
                Ok(Self::new(
                    crate::schema::settings::get::<bool>(ctx.settings, "validateSkippedValues")?.unwrap_or(false),
                    historical_response_bindings(ctx.settings)?,
                ))
            }

            fn build_router(
                &self,
                ctx: crate::schema::routing::RouterBuildContext<'_>,
            ) -> Result<
                impl crate::schema::routing::MetadataProtocolRouter + 'static + use<>,
                crate::schema::routing::RouterBuildError,
            > {
                crate::schema::routing::aws_json_router(&ctx, $content_type)
            }

            fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>> {
                Some(
                    EventStreamFraming::new(self.inner.codec(), "application/json")
                        .initial_messages_in_frames(true)
                        .exception_discriminator(BodyDiscriminator { value: $type_value }),
                )
            }
        }

        impl ServerProtocol for $protocol {
            fn protocol_id(&self) -> &'static ShapeId<'static> {
                static PROTOCOL_ID: ShapeId<'static> = $protocol_id;
                &PROTOCOL_ID
            }

            fn validate_request_headers(
                &self,
                operation: &crate::schema::OperationSchema<'_>,
                headers: &Headers,
            ) -> Result<(), DeserializeError> {
                self.inner.check_accept(operation.output(), headers)
            }

            fn request_body_requirement(&self, operation: &crate::schema::OperationSchema<'_>) -> BodyDirective {
                if self.inner.reads_request_body(operation.input()) {
                    BodyDirective::Collect
                } else {
                    BodyDirective::Skip
                }
            }

            fn deserialize_request<'a>(
                &'a self,
                input: &Schema<'_>,
                request: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
            ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
                self.inner.deserialize_request(input, request)
            }

            fn serialize_response(&self, output: &Schema<'_>, value: &dyn SerializableStruct) -> Response {
                if self.historical_response_bindings {
                    return serialize_response_parts(
                        self.inner.codec(),
                        output,
                        value,
                        ResponseBindings::LegacyRpc,
                        ResponseValueKind::OperationOutput { empty_document: true },
                    )
                    .and_then(|parts| {
                        assemble_response(parts, resolve_status(None, output.http()), Some($content_type))
                    })
                    .unwrap_or_else(serialization_failure::<$marker>);
                }
                self.inner
                    .serialize_response(output, value)
                    .unwrap_or_else(serialization_failure::<$marker>)
            }

            fn serialize_streaming_response(
                &self,
                output: &Schema<'_>,
                value: &dyn SerializableStruct,
                _streaming_member: &Schema<'_>,
                body: Option<BoxBody>,
            ) -> Response {
                if self.historical_response_bindings {
                    return serialize_response_parts(
                        self.inner.codec(),
                        output,
                        value,
                        ResponseBindings::LegacyRpc,
                        ResponseValueKind::StreamingOutput,
                    )
                    .and_then(|parts| {
                        assemble_streaming_response(
                            parts,
                            resolve_status(None, output.http()),
                            Some($content_type),
                            body.unwrap_or_else(crate::body::empty),
                        )
                    })
                    .unwrap_or_else(serialization_failure::<$marker>);
                }
                self.inner
                    .serialize_streaming_response(output, value, body.unwrap_or_else(crate::body::empty))
                    .unwrap_or_else(serialization_failure::<$marker>)
            }

            fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
                serialize_error::<$marker>(
                    self.inner.codec(),
                    error,
                    $content_type,
                    BodyDiscriminator { value: $type_value },
                    self.historical_response_bindings,
                )
            }

            fn serialize_routing_error(&self, err: &crate::schema::routing::RoutingError) -> Response {
                use crate::protocol::aws_json::router::Error;
                use crate::schema::routing::RoutingErrorKind;
                let error = match err.kind() {
                    RoutingErrorKind::MethodNotAllowed => Error::MethodNotAllowed,
                    _ => Error::NotFound,
                };
                IntoResponse::<$marker>::into_response(error)
            }

            /// awsJson's `From<RequestRejection>` collapses every transport failure, `Accept`
            /// and `Content-Type` mismatches included, into a 400 `Serialization`; routing
            /// through that same `From` preserves the collapse. A constraint violation answers
            /// with the modeled validation error.
            fn serialize_rejection(&self, err: DeserializeError) -> Response {
                match err {
                    DeserializeError::Serde(err) => {
                        IntoResponse::<$marker>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
                    }
                    DeserializeError::BodyCollection(err) => {
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
                    DeserializeError::InternalFailure(err) => {
                        IntoResponse::<$marker>::into_response(RuntimeError::InternalFailure(err))
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
