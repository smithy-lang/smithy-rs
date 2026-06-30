/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! AWS Query-compatible protocol implementation.
//!
//! This protocol wraps `awsJson1_0` and adds:
//! - `x-amzn-query-mode: true` request header (signals the service to include
//!   query-style error codes in responses)
//! - `x-amzn-query-error` response header parsing (extracts the query error
//!   code for error variant dispatch)

use crate::protocol::aws_json_rpc::AwsJsonRpcProtocol;
use aws_smithy_runtime_api::http::{Headers, Request, Response};
use aws_smithy_schema::protocol::ClientProtocolInner;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeId};
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::endpoint::Endpoint;

/// Protocol for services that migrated from awsQuery to awsJson1_0 but
/// still expose query-style error codes via the `x-amzn-query-error` header.
#[derive(Debug)]
pub struct AwsQueryCompatibleProtocol {
    inner: AwsJsonRpcProtocol,
}

impl AwsQueryCompatibleProtocol {
    /// Creates a new awsQuery-compatible protocol instance.
    ///
    /// `target_prefix` is the service name used in the `X-Amz-Target` header.
    pub fn new(target_prefix: impl Into<String>) -> Self {
        Self {
            inner: AwsJsonRpcProtocol::aws_json_1_0(target_prefix),
        }
    }
}

impl ClientProtocolInner for AwsQueryCompatibleProtocol {
    type Request = Request;
    type Response = Response;

    fn protocol_id(&self) -> &ShapeId {
        self.inner.protocol_id()
    }

    fn serialize_request(
        &self,
        input: &dyn SerializableStruct,
        input_schema: &Schema,
        endpoint: &str,
        cfg: &ConfigBag,
    ) -> Result<Request, SerdeError> {
        let mut request = self
            .inner
            .serialize_request(input, input_schema, endpoint, cfg)?;
        request.headers_mut().insert("x-amzn-query-mode", "true");
        Ok(request)
    }

    fn deserialize_response<'a>(
        &self,
        response: &'a Response,
        output_schema: &Schema,
        cfg: &ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
        self.inner
            .deserialize_response(response, output_schema, cfg)
    }

    /// Extracts the error code from the `x-amzn-query-error` header (format: "Code;Type").
    /// Only the portion before `;` is used for error variant dispatch. If the header
    /// is absent or malformed (no `;`), falls back to the JSON `__type` code.
    fn resolve_error_code<'a>(&self, headers: &'a Headers, code: &'a str) -> &'a str {
        headers
            .get("x-amzn-query-error")
            .and_then(|v| v.find(';').map(|idx| &v[..idx]))
            .unwrap_or(code)
    }

    fn update_endpoint(
        &self,
        request: &mut Request,
        endpoint: &Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), SerdeError> {
        self.inner.update_endpoint(request, endpoint, cfg)
    }

    fn payload_codec(&self) -> Option<&dyn aws_smithy_schema::codec::DynCodec> {
        self.inner.payload_codec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_runtime_api::client::orchestrator::Metadata;
    use aws_smithy_schema::serde::ShapeSerializer;
    use aws_smithy_schema::{shape_id, ShapeType};
    use aws_smithy_types::config_bag::Layer;

    struct EmptyInput;
    impl SerializableStruct for EmptyInput {
        fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    static SCHEMA: Schema = Schema::new(shape_id!("test", "Input"), ShapeType::Structure);

    fn cfg_with_metadata() -> ConfigBag {
        let mut layer = Layer::new("test");
        layer.store_put(Metadata::new("DoThing", "MyService"));
        ConfigBag::of_layers(vec![layer])
    }

    #[test]
    fn adds_query_mode_header() {
        let cfg = cfg_with_metadata();
        let request = AwsQueryCompatibleProtocol::new("MyService")
            .serialize_request(&EmptyInput, &SCHEMA, "https://example.com", &cfg)
            .unwrap();
        assert_eq!(request.headers().get("x-amzn-query-mode").unwrap(), "true");
    }

    #[test]
    fn preserves_json_content_type() {
        let cfg = cfg_with_metadata();
        let request = AwsQueryCompatibleProtocol::new("MyService")
            .serialize_request(&EmptyInput, &SCHEMA, "https://example.com", &cfg)
            .unwrap();
        assert_eq!(
            request.headers().get("Content-Type").unwrap(),
            "application/x-amz-json-1.0"
        );
    }

    #[test]
    fn preserves_x_amz_target() {
        let cfg = cfg_with_metadata();
        let request = AwsQueryCompatibleProtocol::new("MyService")
            .serialize_request(&EmptyInput, &SCHEMA, "https://example.com", &cfg)
            .unwrap();
        assert_eq!(
            request.headers().get("X-Amz-Target").unwrap(),
            "MyService.DoThing"
        );
    }

    #[test]
    fn resolve_error_code_extracts_code_before_semicolon() {
        let protocol = AwsQueryCompatibleProtocol::new("Svc");
        let mut headers = Headers::new();
        headers.insert("x-amzn-query-error", "InvalidAction;Sender");
        assert_eq!(
            protocol.resolve_error_code(&headers, "fallback"),
            "InvalidAction"
        );
    }

    #[test]
    fn resolve_error_code_no_semicolon_falls_back_to_code() {
        let protocol = AwsQueryCompatibleProtocol::new("Svc");
        let mut headers = Headers::new();
        headers.insert("x-amzn-query-error", "MalformedNoSemicolon");
        // Per spec the header format is "Code;Type" — without a semicolon we can't
        // reliably extract the code portion, so fall back to the JSON error code.
        assert_eq!(
            protocol.resolve_error_code(&headers, "fallback"),
            "fallback"
        );
    }

    #[test]
    fn resolve_error_code_missing_header_returns_fallback() {
        let protocol = AwsQueryCompatibleProtocol::new("Svc");
        let headers = Headers::new();
        assert_eq!(
            protocol.resolve_error_code(&headers, "SomeError"),
            "SomeError"
        );
    }
}
