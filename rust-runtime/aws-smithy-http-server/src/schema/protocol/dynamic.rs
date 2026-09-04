/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{any::Any, future::Future, pin::Pin};

use aws_smithy_schema::{serde::ShapeDeserializer, Schema};

use crate::{
    body::Body,
    deserialize::{DeserializableShape, DeserializeError, RequestDeserializationError},
    modeled_error::{HttpModeledError, HttpServerError, ServerError},
    protocol::{
        aws_json::{rejection as aws_json_rejection, runtime_error as aws_json_runtime_error},
        rest_json_1::{rejection as rest_json_rejection, runtime_error as rest_json_runtime_error, RestJson1},
        rest_xml::{rejection as rest_xml_rejection, runtime_error as rest_xml_runtime_error, RestXml},
        rpc_v2_cbor::{rejection as rpc_v2_cbor_rejection, runtime_error as rpc_v2_cbor_runtime_error, RpcV2Cbor},
    },
    response::IntoResponse,
};

/// Type-erased generated input builder used by dynamic server protocols.
///
/// Dynamic protocols own HTTP body handling. Once a protocol has built the
/// correct shape deserializer for its wire format, this object drives the
/// generated input walker and returns the concrete operation input erased as
/// `Any`.
pub trait ErasedInputBuilder: Send {
    /// Builds an operation input from the protocol-owned shape deserializer.
    fn build_input(
        self: Box<Self>,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<Box<dyn Any + Send>, DeserializeError>;
}

/// A concrete erased input visitor for `T`.
pub struct ErasedInputVisitor<T>(std::marker::PhantomData<T>);

impl<T> ErasedInputVisitor<T> {
    /// Creates a new erased input visitor.
    pub fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T> ErasedInputBuilder for ErasedInputVisitor<T>
where
    T: DeserializableShape + Send + 'static,
{
    fn build_input(
        self: Box<Self>,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<Box<dyn Any + Send>, DeserializeError> {
        Ok(Box::new(T::deserialize(deserializer)?))
    }
}

/// Future returned by dynamic request deserialization.
pub type DeserializeInputFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Box<dyn Any + Send>, ServerError>> + Send + 'a>>;

pub(super) fn request_deserialization_error(err: &RequestDeserializationError) -> DeserializeError {
    DeserializeError::Serde(aws_smithy_schema::serde::SerdeError::custom(err.source().to_string()))
}

pub(super) fn modeled_or_bad_request_response(
    error: &dyn HttpServerError,
    modeled_response: impl FnOnce(&dyn HttpModeledError) -> http::Response<crate::body::BoxBody>,
    request_deserialization_response: impl FnOnce(&RequestDeserializationError) -> http::Response<crate::body::BoxBody>,
) -> http::Response<crate::body::BoxBody> {
    if let Some(modeled) = error.as_modeled_error() {
        return modeled_response(modeled);
    }
    if let Some(err) = error.as_any().downcast_ref::<RequestDeserializationError>() {
        return request_deserialization_response(err);
    }

    let mut response = http::Response::new(crate::body::to_boxed(error.to_string()));
    *response.status_mut() =
        http::StatusCode::from_u16(error.status_code()).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
    response
}

pub(super) fn aws_json_request_deserialization_response<P>(
    err: &RequestDeserializationError,
) -> http::Response<crate::body::BoxBody>
where
    aws_json_runtime_error::RuntimeError: IntoResponse<P>,
{
    let rejection = aws_json_rejection::RequestRejection::from(request_deserialization_error(err));
    IntoResponse::<P>::into_response(aws_json_runtime_error::RuntimeError::from(rejection))
}

pub(super) fn rpc_v2_cbor_request_deserialization_response(
    err: &RequestDeserializationError,
) -> http::Response<crate::body::BoxBody> {
    let rejection = rpc_v2_cbor_rejection::RequestRejection::from(request_deserialization_error(err));
    IntoResponse::<RpcV2Cbor>::into_response(rpc_v2_cbor_runtime_error::RuntimeError::from(rejection))
}

pub(super) fn rest_json_request_deserialization_response(
    err: &RequestDeserializationError,
) -> http::Response<crate::body::BoxBody> {
    let rejection = rest_json_rejection::RequestRejection::from(request_deserialization_error(err));
    IntoResponse::<RestJson1>::into_response(rest_json_runtime_error::RuntimeError::from(rejection))
}

pub(super) fn rest_xml_request_deserialization_response(
    err: &RequestDeserializationError,
) -> http::Response<crate::body::BoxBody> {
    let rejection = rest_xml_rejection::RequestRejection::from(request_deserialization_error(err));
    IntoResponse::<RestXml>::into_response(rest_xml_runtime_error::RuntimeError::from(rejection))
}

pub(super) fn aws_json_request_rejection_response<P>(
    err: &AwsJsonRequestRejection,
) -> http::Response<crate::body::BoxBody>
where
    aws_json_runtime_error::RuntimeError: IntoResponse<P>,
{
    let runtime_error = match &err.0 {
        aws_json_rejection::RequestRejection::ConstraintViolation(reason) => {
            aws_json_runtime_error::RuntimeError::Validation(reason.clone())
        }
        _ => aws_json_runtime_error::RuntimeError::Serialization(crate::Error::new(err.to_string())),
    };
    IntoResponse::<P>::into_response(runtime_error)
}

pub(super) fn rpc_v2_cbor_request_rejection_response(
    err: &RpcV2CborRequestRejection,
) -> http::Response<crate::body::BoxBody> {
    let runtime_error = match &err.0 {
        rpc_v2_cbor_rejection::RequestRejection::ConstraintViolation(reason) => {
            rpc_v2_cbor_runtime_error::RuntimeError::Validation(reason.clone())
        }
        _ => rpc_v2_cbor_runtime_error::RuntimeError::Serialization(crate::Error::new(err.to_string())),
    };
    IntoResponse::<RpcV2Cbor>::into_response(runtime_error)
}

pub(super) fn rest_json_request_rejection_response(
    err: &RestJsonRequestRejection,
) -> http::Response<crate::body::BoxBody> {
    let runtime_error = match &err.0 {
        rest_json_rejection::RequestRejection::MissingContentType(_) => {
            rest_json_runtime_error::RuntimeError::UnsupportedMediaType
        }
        rest_json_rejection::RequestRejection::NotAcceptable => rest_json_runtime_error::RuntimeError::NotAcceptable,
        rest_json_rejection::RequestRejection::ConstraintViolation(reason) => {
            rest_json_runtime_error::RuntimeError::Validation(reason.clone())
        }
        _ => rest_json_runtime_error::RuntimeError::Serialization(crate::Error::new(err.to_string())),
    };
    IntoResponse::<RestJson1>::into_response(runtime_error)
}

pub(super) fn rest_xml_request_rejection_response(
    err: &RestXmlRequestRejection,
) -> http::Response<crate::body::BoxBody> {
    let runtime_error = match &err.0 {
        rest_xml_rejection::RequestRejection::MissingContentType(_) => {
            rest_xml_runtime_error::RuntimeError::UnsupportedMediaType
        }
        rest_xml_rejection::RequestRejection::ConstraintViolation(reason) => {
            rest_xml_runtime_error::RuntimeError::Validation(reason.clone())
        }
        _ => rest_xml_runtime_error::RuntimeError::Serialization(crate::Error::new(err.to_string())),
    };
    IntoResponse::<RestXml>::into_response(runtime_error)
}

#[derive(Debug)]
pub(super) struct AwsJsonRequestRejection(pub(super) aws_json_rejection::RequestRejection);

#[derive(Debug)]
pub(super) struct RpcV2CborRequestRejection(pub(super) rpc_v2_cbor_rejection::RequestRejection);

#[derive(Debug)]
pub(super) struct RestJsonRequestRejection(pub(super) rest_json_rejection::RequestRejection);

#[derive(Debug)]
pub(super) struct RestXmlRequestRejection(pub(super) rest_xml_rejection::RequestRejection);

macro_rules! request_rejection_error {
    ($name:ident) => {
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::error::Error for $name {}

        impl HttpServerError for $name {
            fn status_code(&self) -> u16 {
                http::StatusCode::BAD_REQUEST.as_u16()
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
    };
}

request_rejection_error!(AwsJsonRequestRejection);
request_rejection_error!(RpcV2CborRequestRejection);
request_rejection_error!(RestJsonRequestRejection);
request_rejection_error!(RestXmlRequestRejection);

#[derive(Debug)]
struct DynModeledServerError(Box<dyn HttpModeledError>);

impl std::fmt::Display for DynModeledServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for DynModeledServerError {}

impl HttpServerError for DynModeledServerError {
    fn status_code(&self) -> u16 {
        HttpModeledError::status_code(&*self.0)
    }

    fn as_modeled_error(&self) -> Option<&dyn HttpModeledError> {
        Some(&*self.0)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn request_body_error(error: impl std::fmt::Display) -> ServerError {
    Box::new(RequestDeserializationError::new(
        aws_smithy_schema::serde::SerdeError::custom(error.to_string()),
    ))
}

pub(super) async fn collect_request_body(
    request: http::Request<Body>,
    request_body_max_bytes: usize,
) -> Result<http::Request<bytes::Bytes>, ServerError> {
    let (parts, body) = request.into_parts();
    let bytes = crate::body::collect_body_limited(body, request_body_max_bytes)
        .await
        .map_err(request_body_error)?;
    Ok(http::Request::from_parts(parts, bytes))
}

pub(super) fn empty_request_body(request: http::Request<Body>) -> http::Request<bytes::Bytes> {
    let (parts, _body) = request.into_parts();
    http::Request::from_parts(parts, bytes::Bytes::new())
}

pub(super) fn rest_input_schema_needs_body(input_schema: &Schema<'_>) -> bool {
    input_schema.members().iter().any(|member| {
        member.http_payload().is_some()
            || (member.http_label().is_none()
                && member.http_query().is_none()
                && member.http_header().is_none()
                && member.http_prefix_headers().is_none()
                && member.http_query_params().is_none())
    })
}

pub(super) fn build_erased_input(
    input: Box<dyn ErasedInputBuilder>,
    deserializer: &mut dyn ShapeDeserializer,
) -> Result<Box<dyn Any + Send>, ServerError> {
    match input.build_input(deserializer) {
        Ok(input) => Ok(input),
        Err(DeserializeError::Serde(err)) => Err(Box::new(RequestDeserializationError::new(err))),
        Err(DeserializeError::ConstraintViolation(err)) => Err(Box::new(DynModeledServerError(err))),
    }
}
