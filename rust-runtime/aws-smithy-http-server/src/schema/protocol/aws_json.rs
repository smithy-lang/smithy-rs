/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! awsJson1.0 and awsJson1.1 share a codec, a rejection type and a runtime error; they differ in
//! the content type and in whether the `__type` discriminator is the full shape ID or the name.

use aws_smithy_json::codec::JsonCodec;
use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::aws_json::rejection::RequestRejection;
use crate::protocol::aws_json::runtime_error::RuntimeError;
use crate::protocol::aws_json_10::AwsJson1_0;
use crate::protocol::aws_json_11::AwsJson1_1;
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::discriminator::{BodyDiscriminator, TypeValue};
use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, stamp_validation_extension,
    ResponseBindings,
};
use super::{BodyDirective, EventStreamFraming, MetadataRoutedProtocol, ServerProtocol};
use crate::schema::response_bindings::ResponsePlanCache;

/// Stateful schema-driven AWS JSON 1.0 protocol implementation.
#[derive(Debug)]
/// Defaults to validating escape syntax in skipped strings without decoding Unicode.
/// Set `customizationConfig.protocols` for this protocol to `{"validateSkippedValues":true}`
/// to skip escape validation, matching legacy smithy-rs servers.
pub struct AwsJson1_0Protocol {
    pub(crate) inner: crate::schema::protocol::rpc::RpcProtocol<aws_smithy_json::codec::JsonCodec>,
    /// Plans for the REST header bindings this protocol applies on top of its body responses.
    binding_plans: ResponsePlanCache,
}

impl Default for AwsJson1_0Protocol {
    fn default() -> Self {
        Self::new(false)
    }
}

impl AwsJson1_0Protocol {
    fn new(validate_skipped_values: bool) -> Self {
        Self {
            inner: crate::schema::protocol::rpc::RpcProtocol::new(
                schema_codec(validate_skipped_values),
                "application/x-amz-json-1.0",
                Some("application/x-amz-json-1.0"),
                crate::schema::protocol::rpc::RpcAccept::Always,
                crate::schema::protocol::rpc::RpcStreaming::CodecContentType,
            ),
            binding_plans: Default::default(),
        }
    }
}

/// Stateful schema-driven AWS JSON 1.1 protocol implementation.
#[derive(Debug)]
/// Defaults to validating escape syntax in skipped strings without decoding Unicode.
/// Set `customizationConfig.protocols` for this protocol to `{"validateSkippedValues":true}`
/// to skip escape validation, matching legacy smithy-rs servers.
pub struct AwsJson1_1Protocol {
    pub(crate) inner: crate::schema::protocol::rpc::RpcProtocol<aws_smithy_json::codec::JsonCodec>,
    /// Plans for the REST header bindings this protocol applies on top of its body responses.
    binding_plans: ResponsePlanCache,
}

impl Default for AwsJson1_1Protocol {
    fn default() -> Self {
        Self::new(false)
    }
}

impl AwsJson1_1Protocol {
    fn new(validate_skipped_values: bool) -> Self {
        Self {
            inner: crate::schema::protocol::rpc::RpcProtocol::new(
                schema_codec(validate_skipped_values),
                "application/x-amz-json-1.1",
                Some("application/x-amz-json-1.1"),
                crate::schema::protocol::rpc::RpcAccept::Always,
                crate::schema::protocol::rpc::RpcStreaming::CodecContentType,
            ),
            binding_plans: Default::default(),
        }
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
            .validate_skipped_values(validate_skipped_values)
            .validate_skipped_string_encoding(true)
            .allow_integral_float_numbers(true)
            .strict_timestamp_formats(true)
            .build(),
    )
}

/// The binding plans both AWS JSON protocols apply are identical: REST bindings over the
/// response head, with the body already serialized separately.
const BINDING_PLAN_KIND: crate::schema::response_bindings::ResponseValueKind =
    crate::schema::response_bindings::ResponseValueKind::StreamingOutput;

/// Compiles the header-binding plans for every registered output and error schema. Failing here
/// turns an invalid `@httpHeader` name into a build error instead of a per-response failure.
fn prepare_binding_plans(
    plans: &mut ResponsePlanCache,
    service: &'static crate::schema::ServiceSchema<'static>,
) -> Result<(), crate::schema::routing::RouterBuildError> {
    for operation in service.operations() {
        plans
            .prepare(operation.output(), ResponseBindings::Rest, BINDING_PLAN_KIND)
            .and_then(|()| {
                operation
                    .errors()
                    .iter()
                    .try_for_each(|error| plans.prepare(error, ResponseBindings::Rest, BINDING_PLAN_KIND))
            })
            .map_err(|err| crate::schema::routing::RouterBuildError::Configuration(err.to_string()))?;
    }
    Ok(())
}

// Legacy AWS JSON writes all members to the body and also applies modeled HTTP
// response headers. These headers override the protocol defaults.
fn apply_response_bindings(
    mut response: Response,
    codec: &JsonCodec,
    plans: &ResponsePlanCache,
    schema: &Schema<'_>,
    value: &dyn SerializableStruct,
    success: bool,
) -> Result<Response, SerdeError> {
    let parts = plans.serialize(codec, schema, value, ResponseBindings::Rest, BINDING_PLAN_KIND)?;
    // Remove defaults before appending the entire modeled group, preserving list multiplicity.
    // This includes Content-Length: legacy modeled values override the computed fallback
    // without length-specific validation.
    for (name, _) in &parts.headers {
        response.headers_mut().remove(name);
    }
    for (name, value) in parts.headers {
        response.headers_mut().append(name, value);
    }
    if success {
        *response.status_mut() =
            http::StatusCode::from_u16(super::response::resolve_status(parts.status, schema.http()))
                .map_err(|error| SerdeError::custom(error.to_string()))?;
    }
    Ok(response)
}

fn serialize_error<P>(
    codec: &JsonCodec,
    plans: &ResponsePlanCache,
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
    .and_then(|response| apply_response_bindings(response, codec, plans, schema, error, false))
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
                let mut protocol = Self::new(
                    crate::schema::settings::get::<bool>(ctx.settings, "validateSkippedValues")?.unwrap_or(false),
                );
                prepare_binding_plans(&mut protocol.binding_plans, ctx.service)?;
                Ok(protocol)
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
                self.inner
                    .serialize_response(output, value)
                    .and_then(|response| {
                        apply_response_bindings(
                            response,
                            self.inner.codec(),
                            &self.binding_plans,
                            output,
                            value,
                            true,
                        )
                    })
                    .unwrap_or_else(serialization_failure::<$marker>)
            }

            fn serialize_streaming_response(
                &self,
                output: &Schema<'_>,
                value: &dyn SerializableStruct,
                body: BoxBody,
            ) -> Response {
                self.inner
                    .serialize_streaming_response(output, value, body)
                    .and_then(|response| {
                        apply_response_bindings(
                            response,
                            self.inner.codec(),
                            &self.binding_plans,
                            output,
                            value,
                            true,
                        )
                    })
                    .unwrap_or_else(serialization_failure::<$marker>)
            }

            fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
                serialize_error::<$marker>(
                    self.inner.codec(),
                    &self.binding_plans,
                    error,
                    $content_type,
                    BodyDiscriminator { value: $type_value },
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
