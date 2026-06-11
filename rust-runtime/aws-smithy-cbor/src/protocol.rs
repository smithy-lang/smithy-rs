/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! RPC v2 CBOR protocol implementation.

use crate::codec::{CborCodec, CborCodecSettings};
use crate::Decoder;
use aws_smithy_runtime_api::http::{Headers, Request, Response};
use aws_smithy_schema::http_protocol::HttpRpcProtocol;
use aws_smithy_schema::protocol::ClientProtocolInner;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::error::metadata::{Builder as ErrorMetadataBuilder, ErrorMetadata};

/// RPC v2 CBOR protocol (`smithy.protocols#rpcv2Cbor`).
#[derive(Debug)]
pub struct RpcV2CborProtocol {
    inner: HttpRpcProtocol<CborCodec>,
}

impl RpcV2CborProtocol {
    /// Creates a new RPC v2 CBOR protocol instance.
    pub fn new() -> Self {
        Self {
            inner: HttpRpcProtocol::new(
                shape_id!("smithy.protocols", "rpcv2Cbor"),
                CborCodec::new(CborCodecSettings::default()),
                "application/cbor",
            ),
        }
    }
}

impl Default for RpcV2CborProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientProtocolInner for RpcV2CborProtocol {
    type Request = Request;
    type Response = Response;

    fn protocol_id(&self) -> &ShapeId<'static> {
        self.inner.protocol_id()
    }

    fn serialize_request(
        &self,
        input: &dyn SerializableStruct,
        input_schema: &Schema<'_>,
        endpoint: &str,
        cfg: &ConfigBag,
    ) -> Result<Request, SerdeError> {
        self.inner
            .serialize_request(input, input_schema, endpoint, cfg)
    }

    fn deserialize_response<'a>(
        &self,
        response: &'a Response,
        output_schema: &Schema<'_>,
        cfg: &ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
        self.inner
            .deserialize_response(response, output_schema, cfg)
    }

    fn payload_codec(&self) -> Option<&dyn aws_smithy_schema::codec::DynCodec> {
        self.inner.payload_codec()
    }

    fn update_endpoint(
        &self,
        request: &mut Request,
        endpoint: &aws_smithy_types::endpoint::Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), SerdeError> {
        self.inner.update_endpoint(request, endpoint, cfg)
    }

    fn parse_error_metadata(
        &self,
        response: &Response,
        _cfg: &ConfigBag,
    ) -> Result<ErrorMetadataBuilder, SerdeError> {
        let body = response.body().bytes().unwrap_or(&[]);
        parse_error_envelope_metadata(body, response.headers())
    }
}

/// Parses the canonical CBOR error envelope. The envelope is a CBOR map at the
/// document root containing `__type` (the error code, optionally namespaced
/// and/or URL-suffixed in the same way as JSON envelopes) and an optional
/// message field (`message`, `Message`, or `errorMessage`).
///
/// `headers` is consulted only for the queryCompatible header
/// (`X-Amzn-Query-Error`); when present, it overrides any body-derived code
/// and stores the AWS error type as a `type` extra. Per the rpcv2Cbor spec,
/// non-queryCompatible services do not emit this header, so the override is
/// a no-op for them.
///
/// Returns an empty builder when the body is empty (and the queryCompatible
/// header is absent).
pub(crate) fn parse_error_envelope_metadata(
    response_body: &[u8],
    response_headers: &Headers,
) -> Result<ErrorMetadataBuilder, SerdeError> {
    let mut builder = if response_body.is_empty() {
        ErrorMetadata::builder()
    } else {
        parse_error_body(response_body)?
    };

    // queryCompatible override — see comment on the corresponding helper in
    // `aws-smithy-json::protocol::error`.
    if let Some((qc_code, qc_type)) = parse_query_compatible_header(response_headers) {
        builder = builder.code(qc_code).custom("type", qc_type);
    }

    Ok(builder)
}

fn parse_error_body(response_body: &[u8]) -> Result<ErrorMetadataBuilder, SerdeError> {
    let decoder = &mut Decoder::new(response_body);
    let mut builder = ErrorMetadata::builder();

    match decoder.map().map_err(deser_err)? {
        // Indefinite-length map: read entries until a `Break` token.
        None => loop {
            match decoder.datatype().map_err(deser_err)? {
                crate::data::Type::Break => {
                    decoder.skip().map_err(deser_err)?;
                    break;
                }
                _ => {
                    builder = error_code_and_message(builder, decoder)?;
                }
            }
        },
        // Definite-length map: read exactly `n` entries.
        Some(n) => {
            for _ in 0..n {
                builder = error_code_and_message(builder, decoder)?;
            }
        }
    }

    Ok(builder)
}

/// Parses an `X-Amzn-Query-Error: <code>;<type>` header, returning the two
/// halves. Returns `None` if the header is absent or malformed.
fn parse_query_compatible_header(headers: &Headers) -> Option<(&str, &str)> {
    let value = headers.get("x-amzn-query-error")?;
    value
        .find(';')
        .map(|idx| (&value[..idx], &value[idx + 1..]))
}

fn error_code_and_message(
    mut builder: ErrorMetadataBuilder,
    decoder: &mut Decoder,
) -> Result<ErrorMetadataBuilder, SerdeError> {
    let key = decoder.str().map_err(deser_err)?;
    builder = match key.as_ref() {
        "__type" => {
            let code = decoder.str().map_err(deser_err)?;
            builder.code(sanitize_error_code(&code))
        }
        "message" | "Message" | "errorMessage" => {
            // Silently skip if the value isn't a string. Custom error
            // structures may use non-string types under these keys.
            match decoder.str() {
                Ok(message) => builder.message(message),
                Err(_) => builder,
            }
        }
        _ => {
            decoder.skip().map_err(deser_err)?;
            builder
        }
    };
    Ok(builder)
}

/// Strips the namespace prefix (`com.example#`) and any URL suffix (`:url`) from
/// a wire-format error code, leaving just the shape name. Mirrors the JSON
/// envelope's `sanitize_error_code`.
fn sanitize_error_code(error_code: &str) -> &str {
    let error_code = match error_code.find(':') {
        Some(idx) => &error_code[..idx],
        None => error_code,
    };
    match error_code.find('#') {
        Some(idx) => &error_code[idx + 1..],
        None => error_code,
    }
}

fn deser_err(e: crate::decode::DeserializeError) -> SerdeError {
    SerdeError::InvalidInput {
        message: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Encoder;
    use aws_smithy_runtime_api::http::{Response, StatusCode};
    use aws_smithy_types::body::SdkBody;
    use aws_smithy_types::config_bag::ConfigBag;

    fn cbor_response(body: Vec<u8>) -> Response {
        Response::new(StatusCode::try_from(400).unwrap(), SdkBody::from(body))
    }

    fn encode_envelope(type_value: Option<&str>, message_value: Option<(&str, &str)>) -> Vec<u8> {
        let mut entries: Vec<(&str, &str)> = vec![];
        if let Some(t) = type_value {
            entries.push(("__type", t));
        }
        if let Some((k, v)) = message_value {
            entries.push((k, v));
        }
        let mut encoder = Encoder::new(Vec::new());
        encoder.map(entries.len());
        for (k, v) in &entries {
            encoder.str(k).str(v);
        }
        encoder.into_writer()
    }

    #[test]
    fn parse_error_metadata_extracts_code_and_message() {
        let body = encode_envelope(Some("InvalidGreeting"), Some(("message", "Hi")));
        let response = cbor_response(body);
        let cfg = ConfigBag::base();
        let protocol = RpcV2CborProtocol::new();

        let meta = protocol
            .parse_error_metadata(&response, &cfg)
            .expect("parse succeeds")
            .build();

        assert_eq!(meta.code(), Some("InvalidGreeting"));
        assert_eq!(meta.message(), Some("Hi"));
    }

    #[test]
    fn parse_error_metadata_sanitizes_namespaced_code() {
        let body = encode_envelope(
            Some("aws.protocoltests.rpcv2cbor#InvalidGreeting:http://example/"),
            None,
        );
        let response = cbor_response(body);
        let cfg = ConfigBag::base();
        let protocol = RpcV2CborProtocol::new();

        let meta = protocol
            .parse_error_metadata(&response, &cfg)
            .expect("parse succeeds")
            .build();

        assert_eq!(meta.code(), Some("InvalidGreeting"));
    }

    #[test]
    fn parse_error_metadata_accepts_alternate_message_keys() {
        for key in &["Message", "errorMessage"] {
            let body = encode_envelope(Some("X"), Some((key, "msg")));
            let response = cbor_response(body);
            let cfg = ConfigBag::base();
            let protocol = RpcV2CborProtocol::new();

            let meta = protocol
                .parse_error_metadata(&response, &cfg)
                .expect("parse succeeds")
                .build();

            assert_eq!(meta.message(), Some("msg"), "key={}", key);
        }
    }

    #[test]
    fn parse_error_metadata_empty_body_returns_empty_builder() {
        let response = cbor_response(Vec::new());
        let cfg = ConfigBag::base();
        let protocol = RpcV2CborProtocol::new();

        let meta = protocol
            .parse_error_metadata(&response, &cfg)
            .expect("parse succeeds")
            .build();

        assert_eq!(meta.code(), None);
        assert_eq!(meta.message(), None);
    }

    #[test]
    fn parse_error_metadata_malformed_body_returns_error() {
        // 0xff is a Break token at top level — not a valid CBOR document root.
        let response = cbor_response(vec![0xff]);
        let cfg = ConfigBag::base();
        let protocol = RpcV2CborProtocol::new();

        let err = protocol
            .parse_error_metadata(&response, &cfg)
            .expect_err("malformed body should fail");
        assert!(
            matches!(err, SerdeError::InvalidInput { .. }),
            "expected InvalidInput, got {:?}",
            err
        );
    }

    #[test]
    fn parse_error_metadata_ignores_unknown_keys() {
        // Encode a 3-entry map with an unknown key in the middle, so the
        // decoder must `skip()` past its value to continue reading.
        let mut encoder = Encoder::new(Vec::new());
        encoder.map(3);
        encoder.str("__type").str("InvalidGreeting");
        encoder.str("not_a_known_key").str("ignore me");
        encoder.str("message").str("Hi");
        let body = encoder.into_writer();
        let response = cbor_response(body);
        let cfg = ConfigBag::base();
        let protocol = RpcV2CborProtocol::new();

        let meta = protocol
            .parse_error_metadata(&response, &cfg)
            .expect("parse succeeds")
            .build();

        assert_eq!(meta.code(), Some("InvalidGreeting"));
        assert_eq!(meta.message(), Some("Hi"));
    }

    #[test]
    fn parse_error_metadata_query_compat_header_overrides_body_code() {
        // The body says `__type: CustomCodeError` (the shape name); the
        // queryCompatible header says `Customized;Sender` — codegen dispatch
        // keys for queryCompatible services use the awsQueryError code
        // (`Customized`), so the header MUST win.
        let body = encode_envelope(Some("CustomCodeError"), Some(("message", "Hi")));
        let mut response = cbor_response(body);
        response.headers_mut().insert(
            "x-amzn-query-error".to_string(),
            "Customized;Sender".to_string(),
        );
        let cfg = ConfigBag::base();
        let protocol = RpcV2CborProtocol::new();

        let meta = protocol
            .parse_error_metadata(&response, &cfg)
            .expect("parse succeeds")
            .build();

        assert_eq!(meta.code(), Some("Customized"));
        assert_eq!(meta.message(), Some("Hi"));
        assert_eq!(meta.extra("type"), Some("Sender"));
    }
}
