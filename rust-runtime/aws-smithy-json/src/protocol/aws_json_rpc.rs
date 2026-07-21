/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! AWS JSON RPC protocol implementation (`awsJson1_0` and `awsJson1_1`).
//!
//! # Protocol behaviors
//!
//! - HTTP method: always POST, path: always `/`
//! - `X-Amz-Target`: `{ServiceName}.{OperationName}` (required)
//! - Does **not** use `@jsonName` trait
//! - Default timestamp format: `epoch-seconds`
//! - Ignores HTTP binding traits
//!
//! # Differences between 1.0 and 1.1
//!
//! - Content-Type: `application/x-amz-json-1.0` vs `application/x-amz-json-1.1`
//! - Error `__type` serialization differs on the server side, but clients MUST
//!   accept either format for both versions.

use crate::codec::{JsonCodec, JsonCodecSettings};
use aws_smithy_runtime_api::client::orchestrator::Metadata;
use aws_smithy_schema::http_protocol::HttpRpcProtocol;
use aws_smithy_schema::{shape_id, Schema, ShapeId};
use aws_smithy_types::config_bag::ConfigBag;

/// AWS JSON RPC protocol (`awsJson1_0` / `awsJson1_1`).
#[derive(Debug)]
pub struct AwsJsonRpcProtocol {
    inner: HttpRpcProtocol<JsonCodec>,
    target_prefix: String,
}

impl AwsJsonRpcProtocol {
    /// Creates an AWS JSON 1.0 protocol instance.
    ///
    /// `target_prefix` is the Smithy service shape name used in the `X-Amz-Target` header
    /// (e.g., `"TrentService"` for KMS, `"DynamoDB_20120810"` for DynamoDB).
    pub fn aws_json_1_0(target_prefix: impl Into<String>) -> Self {
        Self::new(
            shape_id!("aws.protocols", "awsJson1_0"),
            "application/x-amz-json-1.0",
            target_prefix.into(),
        )
    }

    /// Creates an AWS JSON 1.1 protocol instance.
    ///
    /// `target_prefix` is the Smithy service shape name used in the `X-Amz-Target` header.
    pub fn aws_json_1_1(target_prefix: impl Into<String>) -> Self {
        Self::new(
            shape_id!("aws.protocols", "awsJson1_1"),
            "application/x-amz-json-1.1",
            target_prefix.into(),
        )
    }

    fn new(
        protocol_id: ShapeId<'static>,
        content_type: &'static str,
        target_prefix: String,
    ) -> Self {
        let codec = JsonCodec::new(
            JsonCodecSettings::builder()
                .use_json_name(false)
                .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                .protocol_id(protocol_id.clone())
                .build(),
        );
        Self {
            inner: HttpRpcProtocol::new(protocol_id, codec, content_type),
            target_prefix,
        }
    }

    /// Configures the default Smithy namespace used to resolve relative
    /// shape IDs in JSON `__type` discriminator fields. Forwarded to
    /// [`JsonCodecSettings::default_namespace`] on the codec wrapped by
    /// this protocol.
    ///
    /// AWS JSON 1.0 / 1.1 services typically emit relative `__type`
    /// values (the shape name without a namespace prefix). Code-generated
    /// clients call this method with the service shape's namespace so
    /// that [`crate::codec::JsonDeserializer::read_discriminated_document`]
    /// can produce a fully-qualified discriminator.
    pub fn with_default_namespace(self, namespace: impl Into<String>) -> Self {
        let new_settings = self
            .inner
            .codec()
            .settings()
            .to_builder()
            .default_namespace(namespace)
            .build();
        let new_codec = JsonCodec::new(new_settings);
        Self {
            inner: self.inner.with_codec(new_codec),
            target_prefix: self.target_prefix,
        }
    }
}

impl aws_smithy_schema::protocol::ClientProtocolInner for AwsJsonRpcProtocol {
    type Request = aws_smithy_runtime_api::http::Request;
    type Response = aws_smithy_runtime_api::http::Response;

    fn protocol_id(&self) -> &ShapeId<'static> {
        self.inner.protocol_id()
    }

    fn serialize_request(
        &self,
        input: &dyn aws_smithy_schema::serde::SerializableStruct,
        input_schema: &Schema<'_>,
        endpoint: &str,
        cfg: &ConfigBag,
    ) -> Result<aws_smithy_runtime_api::http::Request, aws_smithy_schema::serde::SerdeError> {
        let mut request = self
            .inner
            .serialize_request(input, input_schema, endpoint, cfg)?;
        if let Some(metadata) = cfg.load::<Metadata>() {
            request.headers_mut().insert(
                "X-Amz-Target",
                format!("{}.{}", self.target_prefix, metadata.name()),
            );
        }
        Ok(request)
    }

    fn deserialize_response<'a>(
        &self,
        response: &'a aws_smithy_runtime_api::http::Response,
        output_schema: &Schema<'_>,
        cfg: &ConfigBag,
    ) -> Result<
        Box<dyn aws_smithy_schema::serde::ShapeDeserializer + 'a>,
        aws_smithy_schema::serde::SerdeError,
    > {
        self.inner
            .deserialize_response(response, output_schema, cfg)
    }

    /// Extracts canonical error metadata from an `awsJson1_0` / `awsJson1_1`
    /// response.
    ///
    /// awsJson protocols carry the error code in the `__type` (or legacy
    /// `code`) field of the JSON body, with `X-Amzn-Errortype` taking
    /// priority. The error message comes from `message`, `Message`, or
    /// `errorMessage` body keys.
    ///
    /// Per the
    /// [`ClientProtocolInner::parse_error_metadata`](aws_smithy_schema::protocol::ClientProtocolInner::parse_error_metadata)
    /// contract the request id is **not** populated here — the
    /// orchestrator's request-id pipeline attaches it separately.
    ///
    /// `deserialize_error_response` is **not** overridden: awsJson has no
    /// error envelope, so the default (which forwards to
    /// `deserialize_response` against
    /// [`prelude::DOCUMENT`](aws_smithy_schema::prelude::DOCUMENT)) is
    /// already correct — the body root IS the error body.
    fn parse_error_metadata(
        &self,
        response: &aws_smithy_runtime_api::http::Response,
        _cfg: &ConfigBag,
    ) -> Result<aws_smithy_types::error::metadata::Builder, aws_smithy_schema::serde::SerdeError>
    {
        let body = response.body().bytes().unwrap_or(&[]);
        crate::protocol::error::parse_error_envelope_metadata(body, response.headers())
    }

    fn payload_codec(&self) -> Option<&dyn aws_smithy_schema::codec::DynCodec> {
        self.inner.payload_codec()
    }

    fn update_endpoint(
        &self,
        request: &mut aws_smithy_runtime_api::http::Request,
        endpoint: &aws_smithy_types::endpoint::Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), aws_smithy_schema::serde::SerdeError> {
        self.inner.update_endpoint(request, endpoint, cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::protocol::ClientProtocolInner;
    use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
    use aws_smithy_schema::ShapeType;
    use aws_smithy_types::config_bag::Layer;

    struct EmptyStruct;
    impl SerializableStruct for EmptyStruct {
        fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    static TEST_SCHEMA: aws_smithy_schema::Schema =
        aws_smithy_schema::Schema::new(shape_id!("test", "Input"), ShapeType::Structure);

    fn cfg_with_metadata(service: &str, operation: &str) -> ConfigBag {
        let mut layer = Layer::new("test");
        layer.store_put(Metadata::new(operation.to_string(), service.to_string()));
        ConfigBag::of_layers(vec![layer])
    }

    #[test]
    fn json_1_0_content_type() {
        let request = AwsJsonRpcProtocol::aws_json_1_0("TestService")
            .serialize_request(
                &EmptyStruct,
                &TEST_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(
            request.headers().get("Content-Type").unwrap(),
            "application/x-amz-json-1.0"
        );
    }

    #[test]
    fn json_1_1_content_type() {
        let request = AwsJsonRpcProtocol::aws_json_1_1("TestService")
            .serialize_request(
                &EmptyStruct,
                &TEST_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(
            request.headers().get("Content-Type").unwrap(),
            "application/x-amz-json-1.1"
        );
    }

    #[test]
    fn sets_x_amz_target() {
        let cfg = cfg_with_metadata("MyService", "DoThing");
        let request = AwsJsonRpcProtocol::aws_json_1_0("MyService")
            .serialize_request(&EmptyStruct, &TEST_SCHEMA, "https://example.com", &cfg)
            .unwrap();
        assert_eq!(
            request.headers().get("X-Amz-Target").unwrap(),
            "MyService.DoThing"
        );
    }

    #[test]
    fn json_1_0_protocol_id() {
        assert_eq!(
            AwsJsonRpcProtocol::aws_json_1_0("Svc")
                .protocol_id()
                .as_str(),
            "aws.protocols#awsJson1_0"
        );
    }

    #[test]
    fn json_1_1_protocol_id() {
        assert_eq!(
            AwsJsonRpcProtocol::aws_json_1_1("Svc")
                .protocol_id()
                .as_str(),
            "aws.protocols#awsJson1_1"
        );
    }

    // ---- parse_error_metadata overrides --------------------------------

    use aws_smithy_runtime_api::http::{Response, StatusCode};
    use aws_smithy_types::body::SdkBody;

    fn http_response(headers: &[(&str, &str)], body: &str) -> Response {
        let mut response = Response::new(StatusCode::try_from(400).unwrap(), SdkBody::from(body));
        for (name, value) in headers {
            response
                .headers_mut()
                .insert(name.to_string(), value.to_string());
        }
        response
    }

    #[test]
    fn parse_error_metadata_extracts_code_and_message_from_body() {
        let proto = AwsJsonRpcProtocol::aws_json_1_0("Svc");
        let response = http_response(&[], r#"{"__type":"InvalidGreeting","message":"hi"}"#);
        let cfg = ConfigBag::base();
        let meta = proto.parse_error_metadata(&response, &cfg).unwrap().build();
        assert_eq!(meta.code(), Some("InvalidGreeting"));
        assert_eq!(meta.message(), Some("hi"));
    }

    #[test]
    fn parse_error_metadata_header_takes_priority() {
        let proto = AwsJsonRpcProtocol::aws_json_1_1("Svc");
        let response = http_response(
            &[("x-amzn-errortype", "FromHeader")],
            r#"{"__type":"FromBody","message":"go"}"#,
        );
        let cfg = ConfigBag::base();
        let meta = proto.parse_error_metadata(&response, &cfg).unwrap().build();
        assert_eq!(meta.code(), Some("FromHeader"));
        assert_eq!(meta.message(), Some("go"));
    }

    #[test]
    fn parse_error_metadata_sanitizes_namespaced_code() {
        let proto = AwsJsonRpcProtocol::aws_json_1_0("Svc");
        let response = http_response(&[], r#"{"__type":"aws.protocoltests.json#FooError"}"#);
        let cfg = ConfigBag::base();
        let meta = proto.parse_error_metadata(&response, &cfg).unwrap().build();
        assert_eq!(meta.code(), Some("FooError"));
    }

    #[test]
    fn parse_error_metadata_empty_body_returns_empty_builder() {
        let proto = AwsJsonRpcProtocol::aws_json_1_0("Svc");
        let response = http_response(&[], "");
        let cfg = ConfigBag::base();
        let meta = proto.parse_error_metadata(&response, &cfg).unwrap().build();
        assert!(meta.code().is_none());
        assert!(meta.message().is_none());
    }

    #[test]
    fn parse_error_metadata_malformed_body_returns_error() {
        let proto = AwsJsonRpcProtocol::aws_json_1_0("Svc");
        let response = http_response(&[], r#"{"__type":"FooError""#); // truncated
        let cfg = ConfigBag::base();
        let err = proto.parse_error_metadata(&response, &cfg).unwrap_err();
        assert!(matches!(err, SerdeError::InvalidInput { .. }));
    }

    #[test]
    fn with_default_namespace_propagates_to_codec_settings() {
        // The protocol's `with_default_namespace` builder must surface
        // the namespace on the codec's settings — this is the wiring
        // codegen relies on so that wire-bytes `__type:"Capacity"`
        // lifts to a fully-qualified `com.amazonaws.dynamodb#Capacity`
        // discriminator on the resulting [`DiscriminatedDocument`].
        let proto = AwsJsonRpcProtocol::aws_json_1_0("DynamoDB_20120810")
            .with_default_namespace("com.amazonaws.dynamodb");
        assert_eq!(
            proto.inner.codec().settings().default_namespace(),
            Some("com.amazonaws.dynamodb"),
        );
    }

    #[test]
    fn with_default_namespace_preserves_other_settings() {
        // Sanity-check that rebuilding the codec to set
        // `default_namespace` doesn't reset other configured fields
        // — the AwsJsonRpc constructor already disables `@jsonName`
        // and sets epoch-seconds as the default timestamp format.
        let proto =
            AwsJsonRpcProtocol::aws_json_1_0("TestService").with_default_namespace("com.example");
        let settings = proto.inner.codec().settings();
        assert_eq!(settings.default_namespace(), Some("com.example"));
        assert_eq!(
            settings.default_timestamp_format(),
            aws_smithy_types::date_time::Format::EpochSeconds,
        );
    }
}
