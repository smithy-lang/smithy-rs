/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};

use crate::body::BoxBody;
use crate::protocol::rest_xml::rejection::RequestRejection;
use crate::protocol::rest_xml::runtime_error::RuntimeError;
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::request::payload_member;
use super::response::{log_serialize_failure, stamp_error_extension};
use super::rest::RestPolicy;
use super::{BodyDirective, EventStreamFraming, MetadataRoutedProtocol, ServerProtocol};

/// Stateful schema-driven restXml protocol implementation.
///
/// Wrapped collections read only their modeled item and entry names. Requests accept
/// `application/xml`, and parsing checks the modeled root while retaining the legacy
/// parser's recovery from malformed XML.
#[derive(Debug)]
pub struct RestXml {
    pub(crate) inner: crate::schema::protocol::rest::RestProtocol<aws_smithy_xml::codec::XmlCodec>,
    /// Reads the `<Error>` element of received event-stream exception frames, whose root name is
    /// not the error structure's.
    exception_codec: aws_smithy_xml::codec::XmlCodec,
    /// `@restXml(noErrorWrapping: true)`: exception frames are rooted at `<Error>`.
    no_error_wrapping: bool,
}

impl RestXml {
    fn new(
        strict_collection_element_names: bool,
        validate_document: bool,
        accept_text_xml: bool,
        no_error_wrapping: bool,
    ) -> Self {
        let codec = |validate_root_name: bool| {
            aws_smithy_xml::codec::XmlCodec::new(
                aws_smithy_xml::codec::XmlCodecSettings::builder()
                    .error_root_name("Error")
                    .validate_root_name(validate_root_name)
                    .validate_document(validate_document)
                    .strict_collection_element_names(strict_collection_element_names)
                    .build(),
            )
        };
        Self {
            exception_codec: codec(false),
            no_error_wrapping,
            inner: crate::schema::protocol::rest::RestProtocol::new(
                codec(true),
                RestPolicy {
                    request_content_type_aliases: if accept_text_xml {
                        REQUEST_CONTENT_TYPE_ALIASES
                    } else {
                        &[]
                    },
                    ..POLICY
                },
            ),
        }
    }
}

impl Default for RestXml {
    fn default() -> Self {
        Self::new(true, false, false, false)
    }
}

static PROTOCOL_ID: ShapeId<'static> = shape_id!("aws.protocols", "restXml");
const CONTENT_TYPE: &str = "application/xml";

/// Media types a request may use for an XML body besides [`CONTENT_TYPE`].
const REQUEST_CONTENT_TYPE_ALIASES: &[&str] = &["text/xml"];

/// restXml labels a response only when the output schema binds something to the body, gives an
/// untyped blob payload `application/octet-stream`, and sends an empty body for an output with
/// no body members whether or not the user modeled it.
pub(crate) const POLICY: RestPolicy = RestPolicy {
    payload_deserializer: Some(deserialize_payload),
    codec_content_type: CONTENT_TYPE,
    request_content_type_aliases: &[],
    default_response_content_type: None,
    untyped_blob_payload_content_type: Some("application/octet-stream"),
    empty_document: false,
};

fn deserialize_payload(
    member: &Schema<'_>,
    deserializer: &mut dyn ShapeDeserializer,
    consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
) -> Result<(), SerdeError> {
    let Some(metadata) = crate::schema::traits::XmlMetadata::from_schema(member) else {
        return consumer(member, deserializer);
    };
    consumer(
        member,
        &mut StructuredPayloadDeserializer {
            inner: deserializer,
            root: metadata.payload_schema(),
        },
    )
}

impl MetadataRoutedProtocol for RestXml {
    fn from_build_context(
        ctx: &crate::schema::ProtocolBuildContext<'_>,
    ) -> Result<Self, crate::schema::routing::RouterBuildError> {
        let mut protocol = Self::new(
            crate::schema::settings::get::<bool>(ctx.settings, "strictCollectionElementNames")?.unwrap_or(true),
            crate::schema::settings::get::<bool>(ctx.settings, "validateDocument")?.unwrap_or(false),
            crate::schema::settings::get::<bool>(ctx.settings, "acceptTextXml")?.unwrap_or(false),
            // A model fact, recorded by codegen on the service schema.
            crate::schema::traits::RestXmlTrait::from_service(ctx.service)
                .is_some_and(crate::schema::traits::RestXmlTrait::no_error_wrapping),
        );
        protocol
            .inner
            .prepare_response_plans(ctx.service)
            .map_err(|err| crate::schema::routing::RouterBuildError::Configuration(err.to_string()))?;
        Ok(protocol)
    }

    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<
        impl crate::schema::routing::MetadataProtocolRouter + 'static + use<>,
        crate::schema::routing::RouterBuildError,
    > {
        crate::schema::routing::rest_router(&ctx, CONTENT_TYPE, self.inner.policy().request_content_type_aliases)
    }

    fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>> {
        Some(
            EventStreamFraming::new(self.inner.codec(), CONTENT_TYPE)
                .unset_initial_response_payload(b"")
                .exception_http_bindings(true)
                .xml_exceptions(&self.exception_codec, !self.no_error_wrapping),
        )
    }
}

impl ServerProtocol for RestXml {
    fn protocol_id(&self) -> &'static ShapeId<'static> {
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
            .serialize_response_with_codec(&ResponseXmlCodec::new(self.inner.codec(), output), output, value)
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        _streaming_member: &Schema<'_>,
        body: Option<BoxBody>,
    ) -> Response {
        self.inner
            .serialize_streaming_response(output, value, body.unwrap_or_else(crate::body::empty))
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
        // restXml carries no discriminator: the error structure is the body.
        let schema = error.schema();
        // Legacy can omit this header, depending on the operation's success output.
        let content_type = match error.operation_schema() {
            Some(operation) => self.inner.response_content_type(operation.output()),
            None => Some(CONTENT_TYPE),
        };
        self.inner
            .serialize_modeled_error(
                &ResponseXmlCodec::for_error(self.inner.codec()),
                schema,
                error,
                error.status_code(),
                content_type,
            )
            .map(|response| stamp_error_extension(response, schema.shape_id().shape_name()))
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_routing_error(&self, err: &crate::schema::routing::RoutingError) -> Response {
        use crate::protocol::rest::router::Error;
        use crate::schema::routing::RoutingErrorKind;
        if err.kind() == RoutingErrorKind::MissingContentType {
            return self.serialize_rejection(
                crate::rejection::MissingContentTypeReason::UnexpectedMimeType {
                    expected_mime: err.expected_content_type().and_then(|expected| expected.parse().ok()),
                    found_mime: None,
                }
                .into(),
            );
        }
        let error = match err.kind() {
            RoutingErrorKind::MethodNotAllowed => Error::MethodNotAllowed,
            _ => Error::NotFound,
        };
        IntoResponse::<RestXml>::into_response(error)
    }

    fn serialize_runtime_error(&self, error: crate::runtime_error::RuntimeError) -> Response {
        IntoResponse::<RestXml>::into_response(RuntimeError::from(error))
    }

    /// restXml keeps 415 for `Content-Type` failures, but its `From<RequestRejection>` has no
    /// `NotAcceptable` arm, so an `Accept` mismatch falls through to a 400 `Serialization`, NOT
    /// a 406. And its `RuntimeError` response drops the validation body entirely: every runtime
    /// error, constraint violations included, answers with the literal `{}` body, so a constraint
    /// violation must NOT serialize the modeled error here. Both are the protocol's wire contract.
    fn serialize_rejection(&self, err: DeserializeError) -> Response {
        match err {
            DeserializeError::InitialRequestFrame(reason) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::Validation(reason))
            }
            DeserializeError::Serde(err) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
            }
            DeserializeError::BodyCollection(err) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
            }
            DeserializeError::UnsupportedMediaType(reason) => IntoResponse::<RestXml>::into_response(
                RuntimeError::from(RequestRejection::MissingContentType(*reason)),
            ),
            DeserializeError::NotAcceptable => {
                IntoResponse::<RestXml>::into_response(RuntimeError::from(RequestRejection::NotAcceptable))
            }
            // The smuggled reason string never reaches the wire; legacy renders `{}` regardless.
            DeserializeError::ConstraintViolation(_) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::Validation(String::new()))
            }
            DeserializeError::InternalFailure(err) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::InternalFailure(err))
            }
        }
    }
}

/// Configures the XML document root for this response's structured payload.
/// The target schema still supplies its namespace and child metadata.
struct ResponseXmlCodec<'a> {
    inner: &'a aws_smithy_xml::codec::XmlCodec,
    payload_member: Option<&'a Schema<'a>>,
}

impl<'a> ResponseXmlCodec<'a> {
    fn new(inner: &'a aws_smithy_xml::codec::XmlCodec, output: &'a Schema<'a>) -> Self {
        Self {
            inner,
            payload_member: payload_member(output),
        }
    }

    /// Modeled errors are documents rooted at `Error`: like legacy servers, they drop an
    /// `@httpPayload` member, so its `@xmlName` never names the root.
    fn for_error(inner: &'a aws_smithy_xml::codec::XmlCodec) -> Self {
        Self {
            inner,
            payload_member: None,
        }
    }
}

impl Codec for ResponseXmlCodec<'_> {
    type Serializer = aws_smithy_xml::codec::XmlSerializer;
    type Deserializer<'a> = aws_smithy_xml::codec::XmlDeserializer<'a>;

    fn create_serializer(&self) -> Self::Serializer {
        let mut serializer = self.inner.create_serializer();
        // Legacy payload naming gives the member's @xmlName precedence over the target.
        // Without an override, the serializer resolves the target's name as usual.
        if let Some(name) = self.payload_member.and_then(|member| member.xml_name()) {
            serializer.set_next_root_xml_name(name.value().to_owned());
        }
        serializer
    }

    fn create_deserializer<'a>(&self, input: &'a [u8]) -> Self::Deserializer<'a> {
        self.inner.create_deserializer(input)
    }
}

fn serialization_failure(err: SerdeError) -> Response {
    log_serialize_failure(&err);
    IntoResponse::<RestXml>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}

macro_rules! unsupported_reads {
    ($why:literal; $($method:ident -> $ret:ty),+ $(,)?) => {
        $(
            fn $method(&mut self, _schema: &Schema<'_>) -> Result<$ret, SerdeError> {
                Err(SerdeError::unsupported($why))
            }
        )+
    };
}

/// Wraps a codec deserializer to preserve a structured `@httpPayload` member's
/// XML name when generated deserialization switches to the target shape's schema.
/// Only the document root is aliased; child schemas and the underlying codec
/// are passed directly to the consumer.
///
/// For example, if the payload member has `@xmlName("PayloadRoot")` and its target
/// structure has `@xmlName("TargetRoot")`, the request body uses the member's name:
///
/// ```xml
/// <PayloadRoot>
///     <Child>hello</Child>
/// </PayloadRoot>
/// ```
///
/// This wrapper makes the codec expect `PayloadRoot` when the generated walker
/// passes the target structure's schema. The child's XML name remains `Child`.
struct StructuredPayloadDeserializer<'a> {
    inner: &'a mut dyn ShapeDeserializer,
    root: &'static Schema<'static>,
}

impl ShapeDeserializer for StructuredPayloadDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.inner.read_struct(self.root, consumer)
    }

    fn read_list(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.inner.read_list(schema, consumer)
    }

    fn read_map(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.inner.read_map(schema, consumer)
    }

    unsupported_reads! {
        "expected a structured payload";
        read_boolean -> bool,
        read_byte -> i8,
        read_short -> i16,
        read_integer -> i32,
        read_long -> i64,
        read_float -> f32,
        read_double -> f64,
        read_big_integer -> BigInteger,
        read_big_decimal -> BigDecimal,
        read_blob -> Blob,
        read_timestamp -> DateTime,
        read_string -> String,
    }
    fn read_document(&mut self, schema: &Schema<'_>) -> Result<Document, SerdeError> {
        self.inner.read_document(schema)
    }
    fn is_null(&self) -> bool {
        self.inner.is_null()
    }
    fn container_size(&self) -> Option<usize> {
        self.inner.container_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::ShapeType;

    #[test]
    fn structured_payload_preserves_target_metadata_when_aliasing_xml_root() {
        static TRAITS: std::sync::LazyLock<aws_smithy_schema::TraitMap> = std::sync::LazyLock::new(|| {
            let mut traits = aws_smithy_schema::TraitMap::new();
            traits.insert(Box::new(aws_smithy_schema::StringTrait::new(
                aws_smithy_schema::shape_id!("test", "custom"),
                "preserved",
            )));
            traits
        });
        static CHILD: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Target", "value"),
            ShapeType::String,
            "value",
            0,
        )
        .with_xml_name("Child");
        static TARGET: Schema = Schema::new_struct(
            aws_smithy_schema::shape_id!("test", "Target"),
            ShapeType::Structure,
            &[&CHILD],
        )
        .with_xml_name("TargetRoot")
        .with_original_name("OriginalTarget")
        .with_xml_namespace("urn:test:target", Some("t"))
        .with_sensitive()
        .with_xml_unwrapped_output()
        .with_traits(&TRAITS);

        /// Test codec stand-in that checks root aliasing preserves the target's
        /// other metadata and passes the original child schema to the consumer.
        struct MetadataCheckingDeserializer;
        impl ShapeDeserializer for MetadataCheckingDeserializer {
            fn read_struct(
                &mut self,
                schema: &Schema<'_>,
                consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
            ) -> Result<(), SerdeError> {
                assert_eq!(schema.shape_id(), TARGET.shape_id());
                assert_eq!(schema.shape_type(), TARGET.shape_type());
                assert_eq!(schema.xml_name().unwrap().value(), "PayloadRoot");
                assert_eq!(schema.original_name(), Some("OriginalTarget"));
                let namespace = schema.xml_namespace().expect("target namespace preserved");
                assert_eq!(namespace.uri(), "urn:test:target");
                assert_eq!(namespace.prefix(), Some("t"));
                assert!(schema.sensitive().is_some());
                assert!(schema.xml_unwrapped_output());
                assert!(std::ptr::eq(schema.traits().unwrap(), TARGET.traits().unwrap()));
                assert_eq!(schema.members().len(), 1);
                assert!(std::ptr::eq(schema.members()[0], &CHILD));
                consumer(schema.members()[0], self)
            }

            fn read_list(
                &mut self,
                _: &Schema<'_>,
                _: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
            ) -> Result<(), SerdeError> {
                unreachable!()
            }

            fn read_map(
                &mut self,
                _: &Schema<'_>,
                _: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
            ) -> Result<(), SerdeError> {
                unreachable!()
            }

            unsupported_reads! {
                "metadata checking only";
                read_boolean -> bool,
                read_byte -> i8,
                read_short -> i16,
                read_integer -> i32,
                read_long -> i64,
                read_float -> f32,
                read_double -> f64,
                read_big_integer -> BigInteger,
                read_big_decimal -> BigDecimal,
                read_blob -> Blob,
                read_timestamp -> DateTime,
                read_document -> Document,
            }

            fn read_string(&mut self, schema: &Schema<'_>) -> Result<String, SerdeError> {
                assert!(std::ptr::eq(schema, &CHILD));
                Ok("decoded".into())
            }

            fn is_null(&self) -> bool {
                false
            }

            fn container_size(&self) -> Option<usize> {
                None
            }
        }

        static PAYLOAD: Schema = Schema::new_struct(
            aws_smithy_schema::shape_id!("test", "Target"),
            ShapeType::Structure,
            &[&CHILD],
        )
        .with_xml_name("PayloadRoot")
        .with_original_name("OriginalTarget")
        .with_xml_namespace("urn:test:target", Some("t"))
        .with_sensitive()
        .with_xml_unwrapped_output()
        .with_traits(&TRAITS);
        static MEMBER_TRAITS: std::sync::LazyLock<aws_smithy_schema::TraitMap> = std::sync::LazyLock::new(|| {
            let mut traits = aws_smithy_schema::TraitMap::new();
            traits.insert(Box::new(crate::schema::traits::XmlMetadata::new(&PAYLOAD)));
            traits
        });
        static MEMBER_WITH_METADATA: Schema = Schema::new_member(
            aws_smithy_schema::shape_id!("test", "Input", "data"),
            ShapeType::Structure,
            "data",
            0,
        )
        .with_xml_name("PayloadRoot")
        .with_traits(&MEMBER_TRAITS);

        let member = &MEMBER_WITH_METADATA;
        let mut inner = MetadataCheckingDeserializer;
        let mut value = None;
        deserialize_payload(member, &mut inner, &mut |actual_member, payload| {
            assert!(std::ptr::eq(actual_member, member));
            payload.read_struct(&TARGET, &mut |child, deser| {
                value = Some(deser.read_string(child)?);
                Ok(())
            })
        })
        .unwrap();
        assert_eq!(value.as_deref(), Some("decoded"));
        assert_eq!(TARGET.xml_name().unwrap().value(), "TargetRoot");
    }
}
