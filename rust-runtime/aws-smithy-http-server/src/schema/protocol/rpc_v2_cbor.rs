/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::LazyLock;

use aws_smithy_cbor::codec::{CborCodec, CborCodecSettings};
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::Schema;

use crate::body::BoxBody;
use crate::deserialize::DeserializeError;
use crate::modeled_error::HttpModeledError;
use crate::protocol::rpc_v2_cbor::RpcV2Cbor;
use crate::response::IntoResponse;
use crate::schema::protocol::discriminator::WithTypeFirst;
use crate::schema::protocol::request::deserialize_rpc_request;
use crate::schema::protocol::response::{
    assemble_response, log_serialize_failure, stamp_error_extension, AsSerializable,
};
use crate::schema::response_bindings::{resolve_status, serialize_response_parts, ResponseValueKind};

use super::{EventStreamProtocol, ServerProtocol};

static APPLICATION_CBOR_MIME: LazyLock<mime::Mime> = LazyLock::new(|| "application/cbor".parse().expect("valid mime"));

// ============================================================================
// rpcv2Cbor
// ============================================================================

impl ServerProtocol for RpcV2Cbor {
    type Codec = CborCodec;
    type RequestRejection = crate::protocol::rpc_v2_cbor::rejection::RequestRejection;

    fn codec() -> &'static Self::Codec {
        static CODEC: LazyLock<CborCodec> = LazyLock::new(|| CborCodec::new(CborCodecSettings::default()));
        &CODEC
    }

    fn with_request_deserializer<R>(
        schema: &Schema<'_>,
        _output_schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
        f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
    ) -> Result<R, Self::RequestRejection> {
        // The `smithy-protocol: rpc-v2-cbor` header is validated by the
        // router; the body content type is validated here.
        if !crate::protocol::accept_header_classifier(&parts.headers, &APPLICATION_CBOR_MIME) {
            return Err(Self::RequestRejection::NotAcceptable);
        }
        deserialize_rpc_request(Self::codec(), "application/cbor", schema, parts, body, f)
    }

    fn serialize_response(schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody> {
        let result = serialize_response_parts(Self::codec(), schema, output, false, ResponseValueKind::OperationOutput)
            .and_then(|split| {
                let status = resolve_status(split.status, schema);
                assemble_response(split, status, "application/cbor", None)
            });
        match result {
            Ok(mut response) => {
                response.headers_mut().insert(
                    http::HeaderName::from_static("smithy-protocol"),
                    http::HeaderValue::from_static("rpc-v2-cbor"),
                );
                response
            }
            Err(err) => {
                log_serialize_failure(&err);
                IntoResponse::<RpcV2Cbor>::into_response(
                    crate::protocol::rpc_v2_cbor::runtime_error::RuntimeError::Serialization(crate::Error::new(err)),
                )
            }
        }
    }

    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
        let schema = error.schema();
        // Full shape ID as the FIRST map entry (legacy
        // `AddTypeFieldToServerErrorsCborCustomization` order).
        let wrapper = WithTypeFirst {
            type_value: schema.shape_id().as_str(),
            inner: &AsSerializable(error),
        };
        let result = serialize_response_parts(Self::codec(), schema, &wrapper, false, ResponseValueKind::ModeledError)
            .and_then(|split| assemble_response(split, error.status_code(), "application/cbor", None));
        match result {
            Ok(mut response) => {
                response.headers_mut().insert(
                    http::HeaderName::from_static("smithy-protocol"),
                    http::HeaderValue::from_static("rpc-v2-cbor"),
                );
                stamp_error_extension(response, schema.shape_id().shape_name())
            }
            Err(err) => {
                log_serialize_failure(&err);
                IntoResponse::<RpcV2Cbor>::into_response(
                    crate::protocol::rpc_v2_cbor::runtime_error::RuntimeError::Serialization(crate::Error::new(err)),
                )
            }
        }
    }
}

impl EventStreamProtocol for RpcV2Cbor {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/cbor";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/vnd.amazon.eventstream";
    const FRAMES_INITIAL_MESSAGES: bool = true;
}
