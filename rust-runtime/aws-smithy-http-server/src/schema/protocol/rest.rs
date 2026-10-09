/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The HTTP-binding engine shared by the REST protocols. Requests interpret their schemas inline;
//! registered operation outputs reuse binding plans prepared during protocol construction.

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeType};

use crate::body::BoxBody;
use crate::response::Response;
use crate::schema::request_bindings::RestRequestDeserializer;
use crate::schema::response_bindings::{
    serialize_response_parts, ResponseBindings, ResponsePlanCache, ResponseValueKind,
};
use crate::schema::DeserializeError;

use super::request::{
    enforce_content_type, enforce_expected_accept, expected_request_content_type, is_body_member, payload_member,
    EVENT_STREAM_CONTENT_TYPE, OCTET_STREAM_CONTENT_TYPE,
};
use super::response::{assemble_response, assemble_streaming_response, resolve_status};

/// Build-time schema validation reports through the factory's error path, which carries strings.
fn mime_validation_failure(err: DeserializeError) -> SerdeError {
    SerdeError::custom(err.to_string())
}

/// How a REST protocol labels its responses. These are the rules the legacy generated servers
/// follow, so the schema path stays byte-identical to them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RestPolicy {
    /// The codec's media type: codec-framed bodies and structured payloads.
    pub(crate) codec_content_type: &'static str,
    /// Further media types a request may carry wherever `codec_content_type` is expected.
    /// Responses always use `codec_content_type`.
    pub(crate) request_content_type_aliases: &'static [&'static str],
    /// The response media type when nothing in the output schema determines one. restJson1
    /// stamps `application/json` on every such response, restXml stamps nothing.
    pub(crate) default_response_content_type: Option<&'static str>,
    /// The response media type of a non-streaming `@httpPayload` blob without `@mediaType`.
    /// restJson1 sets none, restXml `application/octet-stream`.
    pub(crate) untyped_blob_payload_content_type: Option<&'static str>,
    /// Whether the codec's empty document (`{}` on restJson1) stands in for a missing body: a
    /// user-modeled output with no body members, or an unset structure `@httpPayload`. restXml
    /// sends an empty body in both cases.
    pub(crate) empty_document: bool,
}

#[derive(Debug)]
pub(crate) struct RestProtocol<C> {
    codec: C,
    policy: RestPolicy,
    // Plans for ordinary output serialization. Streaming heads use a different strategy.
    response_plans: ResponsePlanCache,
    // Plans for modeled errors: errors are serialized per response, so without these every
    // throttling or validation error would recompile its plan.
    error_plans: ResponsePlanCache,
}

impl<C> RestProtocol<C> {
    pub(crate) fn new(codec: C, policy: RestPolicy) -> Self {
        Self {
            codec,
            policy,
            response_plans: Default::default(),
            error_plans: Default::default(),
        }
    }

    /// Compiles plans for every registered output and error schema and validates the metadata
    /// those plans and the request/accept gates interpret. Failing here turns an invalid
    /// registered schema — a bad `@httpHeader` name or `@mediaType` — into a build error
    /// instead of a per-request failure.
    pub(crate) fn prepare_response_plans(
        &mut self,
        service: &'static crate::schema::ServiceSchema<'static>,
    ) -> Result<(), SerdeError> {
        for operation in service.operations() {
            let output = operation.output();
            self.response_plans.prepare(
                output,
                ResponseBindings::Rest,
                ResponseValueKind::OperationOutput {
                    empty_document: self.policy.empty_document,
                },
            )?;
            for error in operation.errors() {
                self.error_plans
                    .prepare(error, ResponseBindings::Rest, ResponseValueKind::ModeledError)?;
            }
            // The request gate parses the input's derived media type and the Accept gate the
            // output's; validate both now so neither can fail per request.
            expected_request_content_type(
                operation.input(),
                self.policy.codec_content_type,
                self.policy.request_content_type_aliases,
            )
            .map_err(mime_validation_failure)?;
            if let Some(content_type) = self.response_content_type(output) {
                super::request::parse_mime(content_type).map_err(mime_validation_failure)?;
            }
        }
        Ok(())
    }

    pub(crate) fn codec(&self) -> &C {
        &self.codec
    }

    pub(crate) fn policy(&self) -> &RestPolicy {
        &self.policy
    }

    /// The `Content-Type` a response for `output` carries, if any: the runtime mirror of the
    /// legacy `HttpBindingResolver.responseContentType`. It depends on the schema alone, never on
    /// which members are set.
    pub(crate) fn response_content_type<'s>(&self, output: &'s Schema<'s>) -> Option<&'s str> {
        if let Some(payload) = payload_member(output) {
            return match payload.shape_type() {
                ShapeType::Union if payload.streaming() => Some(EVENT_STREAM_CONTENT_TYPE),
                ShapeType::Structure | ShapeType::Union | ShapeType::Document => Some(self.policy.codec_content_type),
                _ if payload.media_type().is_some() => payload.media_type().map(|m| m.value()),
                ShapeType::Blob if payload.streaming() => Some(OCTET_STREAM_CONTENT_TYPE),
                ShapeType::Blob => self.policy.untyped_blob_payload_content_type,
                ShapeType::String => Some("text/plain"),
                _ => Some(self.policy.codec_content_type),
            };
        }
        if output.members().iter().any(|m| is_body_member(m)) {
            Some(self.policy.codec_content_type)
        } else {
            self.policy.default_response_content_type
        }
    }

    /// The legacy REST deserializers never touch the body when nothing is bound to it.
    pub(crate) fn reads_request_body(&self, input: &Schema<'_>) -> bool {
        input.members().iter().any(|m| is_body_member(m) && !m.streaming())
    }

    /// The `Accept` gate is driven by the same media type the response will carry.
    pub(crate) fn check_accept(&self, output: &Schema<'_>, headers: &Headers) -> Result<(), DeserializeError> {
        enforce_expected_accept(headers, self.response_content_type(output))
    }
}

impl<C: Codec> RestProtocol<C> {
    pub(crate) fn deserialize_request<'a>(
        &'a self,
        input: &Schema<'_>,
        request: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        enforce_content_type(
            request.headers(),
            &expected_request_content_type(
                input,
                self.policy.codec_content_type,
                self.policy.request_content_type_aliases,
            )?,
            request.body(),
        )?;
        Ok(Box::new(RestRequestDeserializer::new(
            &self.codec,
            request.uri_ref(),
            request.headers(),
            request.body(),
        )))
    }

    pub(crate) fn serialize_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<Response, SerdeError> {
        self.serialize_response_with_codec(&self.codec, output, value)
    }

    /// Uses a protocol-configured codec while retaining the shared HTTP binding plans and policy.
    pub(crate) fn serialize_response_with_codec<D: Codec>(
        &self,
        codec: &D,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<Response, SerdeError> {
        // Default/manual protocols and unregistered schemas compile per call.
        let parts = self.response_plans.serialize(
            codec,
            output,
            value,
            ResponseBindings::Rest,
            ResponseValueKind::OperationOutput {
                empty_document: self.policy.empty_document,
            },
        )?;
        let status = parts.status.unwrap_or_else(|| resolve_status(None, output.http()));
        assemble_response(parts, status, self.response_content_type(output))
    }

    /// Serializes a modeled error through the prepared plan for registered error schemas;
    /// unregistered schemas compile per call.
    pub(crate) fn serialize_modeled_error<D: Codec>(
        &self,
        codec: &D,
        schema: &Schema<'_>,
        error: &dyn SerializableStruct,
        status: u16,
        content_type: &'static str,
    ) -> Result<Response, SerdeError> {
        let parts = self.error_plans.serialize(
            codec,
            schema,
            error,
            ResponseBindings::Rest,
            ResponseValueKind::ModeledError,
        )?;
        assemble_response(parts, status, Some(content_type))
    }

    pub(crate) fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        body: BoxBody,
    ) -> Result<Response, SerdeError> {
        let parts = serialize_response_parts(
            &self.codec,
            output,
            value,
            ResponseBindings::Rest,
            ResponseValueKind::StreamingOutput,
        )?;
        let status = parts.status.unwrap_or_else(|| resolve_status(None, output.http()));
        assemble_streaming_response(parts, status, self.response_content_type(output), body)
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::schema::protocol::{RestJson1Protocol, RestXmlProtocol};
    use crate::schema::{MetadataRoutedProtocol, OperationSchema, ProtocolBuildContext, ServerProtocol, ServiceSchema};
    use aws_smithy_schema::serde::ShapeSerializer;
    use aws_smithy_schema::{shape_id, ShapeType};
    use http_body_util::BodyExt;

    static HEADER: Schema<'static> =
        Schema::new_member(shape_id!("test", "Cached", "v"), ShapeType::String, "v", 0).with_http_header("x-test");
    static BODY: Schema<'static> =
        Schema::new_member(shape_id!("test", "Cached", "body"), ShapeType::String, "body", 1);
    static STATUS: Schema<'static> =
        Schema::new_member(shape_id!("test", "Cached", "status"), ShapeType::Integer, "status", 2)
            .with_http_response_code();
    static MEMBERS: [&Schema<'static>; 3] = [&HEADER, &BODY, &STATUS];
    static OUTPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Cached"), ShapeType::Structure, &MEMBERS).with_original_name("Cached");
    static OP: OperationSchema<'static> = OperationSchema::new(shape_id!("test", "Op"), &OUTPUT, &OUTPUT, &[]);
    static STREAM: Schema<'static> =
        Schema::new_member(shape_id!("test", "Stream", "body"), ShapeType::Blob, "body", 1)
            .with_http_payload()
            .with_streaming();
    static STREAM_MEMBERS: [&Schema<'static>; 3] = [&HEADER, &STREAM, &STATUS];
    static STREAM_OUTPUT: Schema<'static> =
        Schema::new_struct(shape_id!("test", "Stream"), ShapeType::Structure, &STREAM_MEMBERS);
    static STREAM_OP: OperationSchema<'static> =
        OperationSchema::new(shape_id!("test", "StreamOp"), &OUTPUT, &STREAM_OUTPUT, &[]);
    static OPS: [&OperationSchema<'static>; 3] = [&OP, &OP, &STREAM_OP];
    static SERVICE: ServiceSchema<'static> = ServiceSchema::new(shape_id!("test", "Service"), None, &[], &OPS);

    struct Value<'a>(&'a Schema<'a>);

    impl SerializableStruct for Value<'_> {
        fn schema(&self) -> &Schema<'_> {
            self.0
        }
        fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            serializer.write_string(self.0.members()[0], "header-value")?;
            serializer.write_string(self.0.members()[1], "body-value")?;
            serializer.write_integer(self.0.members()[2], 202)
        }
    }

    #[test]
    fn factories_prepare_and_deduplicate_static_outputs() {
        let ctx = ProtocolBuildContext::new(&SERVICE);
        let json = RestJson1Protocol::from_build_context(&ctx).unwrap();
        let xml = RestXmlProtocol::from_build_context(&ctx).unwrap();
        for plans in [&json.inner.response_plans, &xml.inner.response_plans] {
            assert_eq!(plans.len(), 2);
            assert!(plans.contains(&OUTPUT));
            let same_id = Schema::new_struct(shape_id!("test", "Cached"), ShapeType::Structure, &MEMBERS);
            assert!(!plans.contains(&same_id));
        }
        assert_eq!(RestJson1Protocol::default().inner.response_plans.len(), 0);
        assert_eq!(RestXmlProtocol::default().inner.response_plans.len(), 0);
    }

    async fn snapshot(response: Response) -> (http::StatusCode, http::HeaderMap, bytes::Bytes) {
        let (parts, body) = response.into_parts();
        (parts.status, parts.headers, body.collect().await.unwrap().to_bytes())
    }

    async fn check_cached_and_fallback<P: MetadataRoutedProtocol + ServerProtocol + Default>(expected_body: &[u8]) {
        let cached = P::from_build_context(&ProtocolBuildContext::new(&SERVICE)).unwrap();
        let uncached = P::default();
        let value = Value(&OUTPUT);
        let expected = snapshot(uncached.serialize_response(&OUTPUT, &value)).await;
        assert_eq!(expected.0, http::StatusCode::ACCEPTED);
        assert_eq!(expected.1["x-test"], "header-value");
        assert_eq!(expected.2.as_ref(), expected_body);
        for _ in 0..2 {
            assert_eq!(snapshot(cached.serialize_response(&OUTPUT, &value)).await, expected);
        }

        // A dynamic schema may reuse the shape ID and member indices with different bindings.
        // It must miss the cache and interpret its own traits, not the registered output's.
        let header =
            Schema::new_member(shape_id!("test", "Cached", "v"), ShapeType::String, "v", 0).with_http_header("x-other");
        let members = [&header, &BODY, &STATUS];
        let dynamic = Schema::new_struct(shape_id!("test", "Cached"), ShapeType::Structure, &members)
            .with_original_name("Cached");
        let value = Value(&dynamic);
        let actual = snapshot(cached.serialize_response(&dynamic, &value)).await;
        assert_eq!(actual, snapshot(uncached.serialize_response(&dynamic, &value)).await);
        assert_eq!(actual.1["x-other"], "header-value");
        assert!(!actual.1.contains_key("x-test"));

        // The same registered schema must not use an ordinary-output plan for a streaming head.
        let stream = || crate::body::boxed(http_body_util::Full::new(bytes::Bytes::from_static(b"stream")));
        struct Head;
        impl SerializableStruct for Head {
            fn schema(&self) -> &Schema<'_> {
                &STREAM_OUTPUT
            }
            fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                serializer.write_string(&HEADER, "header-value")?;
                serializer.write_integer(&STATUS, 202)
            }
        }
        let actual = snapshot(cached.serialize_streaming_response(&STREAM_OUTPUT, &Head, stream())).await;
        assert_eq!(
            actual,
            snapshot(uncached.serialize_streaming_response(&STREAM_OUTPUT, &Head, stream())).await
        );
        assert_eq!(actual.2.as_ref(), b"stream");
    }

    #[tokio::test]
    async fn json_cached_responses_and_fallbacks_match_uncached() {
        check_cached_and_fallback::<RestJson1Protocol>(br#"{"body":"body-value"}"#).await;
    }

    #[tokio::test]
    async fn xml_cached_responses_and_fallbacks_match_uncached() {
        check_cached_and_fallback::<RestXmlProtocol>(b"<Cached><body>body-value</body></Cached>").await;
    }
}
