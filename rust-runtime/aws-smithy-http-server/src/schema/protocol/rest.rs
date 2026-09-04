/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeId};

use crate::{
    body::{Body, BoxBody},
    modeled_error::{HttpServerError, ServerError},
    schema::protocol::{
        dynamic::{
            build_erased_input, collect_request_body, empty_request_body, modeled_or_bad_request_response,
            rest_input_schema_needs_body, rest_json_request_deserialization_response,
            rest_json_request_rejection_response, rest_xml_request_deserialization_response,
            rest_xml_request_rejection_response, RestJsonRequestRejection, RestXmlRequestRejection,
        },
        rest_json as schema_rest_json, rest_xml as schema_rest_xml, DeserializeInputConfig, DeserializeInputFuture,
        ErasedInputBuilder, ServerProtocolInner,
    },
};

/// REST server protocol implementation.
#[derive(Debug, Clone)]
pub struct RestServerProtocol {
    protocol: ShapeId<'static>,
    version: RestVersion,
}

#[derive(Debug, Clone, Copy)]
enum RestVersion {
    RestJson1,
    RestXml,
}

impl RestServerProtocol {
    /// Creates a restJson1 server protocol.
    pub fn rest_json_1() -> Self {
        Self {
            protocol: ShapeId::from_parts("aws.protocols#restJson1", "aws.protocols", "restJson1"),
            version: RestVersion::RestJson1,
        }
    }

    /// Creates a restXml server protocol.
    pub fn rest_xml() -> Self {
        Self {
            protocol: ShapeId::from_parts("aws.protocols#restXml", "aws.protocols", "restXml"),
            version: RestVersion::RestXml,
        }
    }

    fn request_deserializer<'a>(
        &self,
        request: &'a http::Request<bytes::Bytes>,
        input_schema: &Schema<'_>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, ServerError> {
        match self.version {
            RestVersion::RestJson1 => schema_rest_json::rest_json_1_request_deserializer(input_schema, request)
                .map_err(|rejection| Box::new(RestJsonRequestRejection(rejection)) as ServerError),
            RestVersion::RestXml => schema_rest_xml::rest_xml_request_deserializer(input_schema, request)
                .map_err(|rejection| Box::new(RestXmlRequestRejection(rejection)) as ServerError),
        }
    }
}

impl ServerProtocolInner for RestServerProtocol {
    fn protocol_id(&self) -> &ShapeId<'static> {
        &self.protocol
    }

    fn codec(&self) -> &dyn aws_smithy_schema::codec::DynCodec {
        match self.version {
            RestVersion::RestJson1 => schema_rest_json::rest_json_1_codec(),
            RestVersion::RestXml => schema_rest_xml::rest_xml_codec(),
        }
    }

    fn deserialize_input<'a>(
        &'a self,
        request: http::Request<Body>,
        input_schema: &'static Schema<'static>,
        config: DeserializeInputConfig,
        input: Box<dyn ErasedInputBuilder>,
    ) -> DeserializeInputFuture<'a> {
        Box::pin(async move {
            let request = if rest_input_schema_needs_body(input_schema) {
                collect_request_body(request, config.request_body_max_bytes).await?
            } else {
                empty_request_body(request)
            };
            let mut deserializer = self.request_deserializer(&request, input_schema)?;
            build_erased_input(input, &mut *deserializer)
        })
    }

    fn serialize_response(&self, schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody> {
        match self.version {
            RestVersion::RestJson1 => schema_rest_json::rest_json_1_serialize_response(schema, output),
            RestVersion::RestXml => schema_rest_xml::rest_xml_serialize_response(schema, output),
        }
    }

    fn serialize_error(&self, error: &dyn HttpServerError) -> http::Response<BoxBody> {
        match self.version {
            RestVersion::RestJson1 => {
                if let Some(err) = error.as_any().downcast_ref::<RestJsonRequestRejection>() {
                    return rest_json_request_rejection_response(err);
                }
                modeled_or_bad_request_response(
                    error,
                    schema_rest_json::rest_json_1_serialize_error,
                    rest_json_request_deserialization_response,
                )
            }
            RestVersion::RestXml => {
                if let Some(err) = error.as_any().downcast_ref::<RestXmlRequestRejection>() {
                    return rest_xml_request_rejection_response(err);
                }
                modeled_or_bad_request_response(
                    error,
                    schema_rest_xml::rest_xml_serialize_error,
                    rest_xml_request_deserialization_response,
                )
            }
        }
    }

    fn event_payload_content_type(&self) -> Option<&'static str> {
        match self.version {
            RestVersion::RestJson1 => Some("application/json"),
            RestVersion::RestXml => Some("application/xml"),
        }
    }

    fn event_stream_http_content_type(&self) -> Option<&'static str> {
        Some("application/vnd.amazon.eventstream")
    }

    fn frames_initial_messages(&self) -> bool {
        false
    }
}
