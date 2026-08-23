//! Byte-identity goldens: legacy generated `IntoResponse<P>` responses vs the
//! schema-driven error path.
//!
//! Since the `schemaSerde` codegen flag became a real opt-in, each model is
//! generated twice: the unsuffixed crates (flag OFF) carry only the legacy
//! serializers, and the `*_schema` crates (flag ON) serve modeled errors
//! through `ServerProtocol::serialize_error` — their `IntoResponse<P>` impls
//! ARE the schema path, and the legacy error serializers are not generated at
//! all. The goldens therefore compare `IntoResponse` across the crate pair,
//! which exercises the real serving path on both sides.
//!
//! These are the merge gate for the schema-decoupled error path (RFC §2, P1):
//! for every protocol, status code, headers (content-type, discriminator,
//! content-length), and body **bytes** must match the legacy generated
//! serializers exactly.
//!
//! Known, deliberate exclusions:
//! - restXml: today's generated restXml server error bodies are broken (bare
//!   `<Error>` envelope, discarded validation bodies) — freezing them would
//!   freeze a bug (assumptions register B4/B6), so restXml has no goldens here.
//! - Event-stream operations: these keep the legacy path even in flag-on
//!   crates (their pre-first-event HTTP error stamps
//!   `Content-Type: application/vnd.amazon.eventstream`, register A2, and
//!   their frame marshallers need the legacy payload serializers). The
//!   `pokemon_eventstream_error` golden calls `serialize_error` directly on
//!   the flag-on crate's error, asserts everything else matches, and pins the
//!   content-type divergence explicitly.

use aws_smithy_http_server::body::BoxBody;
use aws_smithy_http_server::protocol::aws_json_10::AwsJson1_0;
use aws_smithy_http_server::protocol::aws_json_11::AwsJson1_1;
use aws_smithy_http_server::protocol::rest_json_1::RestJson1;
use aws_smithy_http_server::protocol::rpc_v2_cbor::RpcV2Cbor;
use aws_smithy_http_server::protocol::server_protocol::ServerProtocol;
use aws_smithy_http_server::response::IntoResponse;
use bytes::Bytes;
use http_body_util::BodyExt;

/// Collapses a response into comparable parts: status, sorted headers, body bytes.
async fn parts(response: http::Response<BoxBody>) -> (http::StatusCode, Vec<(String, String)>, Bytes) {
    let (parts, body) = response.into_parts();
    let body = body.collect().await.expect("failed to collect body").to_bytes();
    let mut headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    headers.sort();
    (parts.status, headers, body)
}

async fn assert_identical(
    label: &str,
    legacy: http::Response<BoxBody>,
    schema: http::Response<BoxBody>,
) {
    let (legacy_status, legacy_headers, legacy_body) = parts(legacy).await;
    let (schema_status, schema_headers, schema_body) = parts(schema).await;
    assert_eq!(legacy_status, schema_status, "[{label}] status mismatch");
    assert_eq!(legacy_headers, schema_headers, "[{label}] headers mismatch");
    assert_eq!(
        legacy_body,
        schema_body,
        "[{label}] body mismatch\n legacy: {}\n schema: {}",
        String::from_utf8_lossy(&legacy_body),
        String::from_utf8_lossy(&schema_body),
    );
}

// ---------------------------------------------------------------------------
// restJson1 (rest_json / rest_json_schema crates)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn restjson_invalid_greeting() {
    const MESSAGE: &str = "Hi\n\"quoted\" \\ and\ttabs — non-ASCII";
    let legacy = IntoResponse::<RestJson1>::into_response(
        rest_json::error::GreetingWithErrorsError::InvalidGreeting(
            rest_json::error::InvalidGreeting {
                message: Some(MESSAGE.to_owned()),
            },
        ),
    );
    let schema = IntoResponse::<RestJson1>::into_response(
        rest_json_schema::error::GreetingWithErrorsError::InvalidGreeting(
            rest_json_schema::error::InvalidGreeting {
                message: Some(MESSAGE.to_owned()),
            },
        ),
    );
    assert_identical("restJson1 InvalidGreeting", legacy, schema).await;
}

#[tokio::test]
async fn restjson_complex_error_header_split() {
    // `Header` is `@httpHeader("X-Header")`-bound: legacy splits it out of the
    // body into a response header, and writes the body members in
    // binding-resolver order (member-name sorted: Nested before TopLevel).
    let legacy = IntoResponse::<RestJson1>::into_response(
        rest_json::error::GreetingWithErrorsError::ComplexError(rest_json::error::ComplexError {
            header: Some("header-value".to_owned()),
            top_level: Some("top level".to_owned()),
            nested: Some(rest_json::model::ComplexNestedErrorData {
                foo: Some("bar".to_owned()),
            }),
        }),
    );
    let schema = IntoResponse::<RestJson1>::into_response(
        rest_json_schema::error::GreetingWithErrorsError::ComplexError(
            rest_json_schema::error::ComplexError {
                header: Some("header-value".to_owned()),
                top_level: Some("top level".to_owned()),
                nested: Some(rest_json_schema::model::ComplexNestedErrorData {
                    foo: Some("bar".to_owned()),
                }),
            },
        ),
    );
    assert_identical("restJson1 ComplexError", legacy, schema).await;
}

#[tokio::test]
async fn restjson_complex_error_empty_header_skipped() {
    // Legacy skips empty-string header values entirely.
    let legacy = IntoResponse::<RestJson1>::into_response(
        rest_json::error::GreetingWithErrorsError::ComplexError(rest_json::error::ComplexError {
            header: Some(String::new()),
            top_level: Some("top level".to_owned()),
            nested: None,
        }),
    );
    let schema = IntoResponse::<RestJson1>::into_response(
        rest_json_schema::error::GreetingWithErrorsError::ComplexError(
            rest_json_schema::error::ComplexError {
                header: Some(String::new()),
                top_level: Some("top level".to_owned()),
                nested: None,
            },
        ),
    );
    assert_identical("restJson1 ComplexError empty header", legacy, schema).await;
}

#[tokio::test]
async fn restjson_validation_exception() {
    // The frozen ValidationException layout: `fieldList` before `message`
    // (binding-resolver member-name order), single-entry field list.
    const MESSAGE: &str = "1 validation error detected. Value with length 1 at '/conA/lengthString' failed to satisfy constraint: Member must have length between 2 and 69, inclusive";
    const FIELD_MESSAGE: &str = "Value with length 1 at '/conA/lengthString' failed to satisfy constraint: Member must have length between 2 and 69, inclusive";
    const PATH: &str = "/conA/lengthString";
    let legacy = IntoResponse::<RestJson1>::into_response(
        constraints::error::ConstrainedShapesOperationError::ValidationException(
            constraints::error::ValidationException {
                message: MESSAGE.to_owned(),
                field_list: Some(vec![constraints::model::ValidationExceptionField {
                    path: PATH.to_owned(),
                    message: FIELD_MESSAGE.to_owned(),
                }]),
            },
        ),
    );
    let schema = IntoResponse::<RestJson1>::into_response(
        constraints_schema::error::ConstrainedShapesOperationError::ValidationException(
            constraints_schema::error::ValidationException {
                message: MESSAGE.to_owned(),
                field_list: Some(vec![constraints_schema::model::ValidationExceptionField {
                    path: PATH.to_owned(),
                    message: FIELD_MESSAGE.to_owned(),
                }]),
            },
        ),
    );
    assert_identical("restJson1 ValidationException", legacy, schema).await;
}

// ---------------------------------------------------------------------------
// awsJson 1.1 (json_rpc11 / json_rpc11_schema crates)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn awsjson11_invalid_greeting() {
    let legacy = IntoResponse::<AwsJson1_1>::into_response(
        json_rpc11::error::GreetingWithErrorsError::InvalidGreeting(
            json_rpc11::error::InvalidGreeting {
                message: Some("Hi".to_owned()),
            },
        ),
    );
    let schema = IntoResponse::<AwsJson1_1>::into_response(
        json_rpc11_schema::error::GreetingWithErrorsError::InvalidGreeting(
            json_rpc11_schema::error::InvalidGreeting {
                message: Some("Hi".to_owned()),
            },
        ),
    );
    assert_identical("awsJson1.1 InvalidGreeting", legacy, schema).await;
}

#[tokio::test]
async fn awsjson11_complex_error() {
    // awsJson: `__type` (name only) written after the modeled members, which
    // appear in model member order (TopLevel before Nested).
    let legacy = IntoResponse::<AwsJson1_1>::into_response(
        json_rpc11::error::GreetingWithErrorsError::ComplexError(json_rpc11::error::ComplexError {
            top_level: Some("top level".to_owned()),
            nested: Some(json_rpc11::model::ComplexNestedErrorData {
                foo: Some("bar".to_owned()),
            }),
        }),
    );
    let schema = IntoResponse::<AwsJson1_1>::into_response(
        json_rpc11_schema::error::GreetingWithErrorsError::ComplexError(
            json_rpc11_schema::error::ComplexError {
                top_level: Some("top level".to_owned()),
                nested: Some(json_rpc11_schema::model::ComplexNestedErrorData {
                    foo: Some("bar".to_owned()),
                }),
            },
        ),
    );
    assert_identical("awsJson1.1 ComplexError", legacy, schema).await;
}

// ---------------------------------------------------------------------------
// awsJson 1.0 (json_rpc10 / json_rpc10_schema crates)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn awsjson10_invalid_greeting() {
    // awsJson 1.0: `__type` carries the FULL shape ID
    // (`aws.protocoltests.json10#InvalidGreeting`).
    let legacy = IntoResponse::<AwsJson1_0>::into_response(
        json_rpc10::error::GreetingWithErrorsError::InvalidGreeting(
            json_rpc10::error::InvalidGreeting {
                message: Some("Hi".to_owned()),
            },
        ),
    );
    let schema = IntoResponse::<AwsJson1_0>::into_response(
        json_rpc10_schema::error::GreetingWithErrorsError::InvalidGreeting(
            json_rpc10_schema::error::InvalidGreeting {
                message: Some("Hi".to_owned()),
            },
        ),
    );
    assert_identical("awsJson1.0 InvalidGreeting", legacy, schema).await;
}

// ---------------------------------------------------------------------------
// rpcv2Cbor (rpcv2cbor / rpcv2cbor_schema crates)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rpcv2cbor_invalid_greeting() {
    // rpcv2Cbor: `__type` (full shape ID) is the FIRST map entry; response
    // carries `smithy-protocol: rpc-v2-cbor`.
    let legacy = IntoResponse::<RpcV2Cbor>::into_response(
        rpcv2cbor::error::GreetingWithErrorsError::InvalidGreeting(
            rpcv2cbor::error::InvalidGreeting {
                message: Some("Hi".to_owned()),
            },
        ),
    );
    let schema = IntoResponse::<RpcV2Cbor>::into_response(
        rpcv2cbor_schema::error::GreetingWithErrorsError::InvalidGreeting(
            rpcv2cbor_schema::error::InvalidGreeting {
                message: Some("Hi".to_owned()),
            },
        ),
    );
    assert_identical("rpcv2Cbor InvalidGreeting", legacy, schema).await;
}

#[tokio::test]
async fn rpcv2cbor_complex_error() {
    let legacy = IntoResponse::<RpcV2Cbor>::into_response(
        rpcv2cbor::error::GreetingWithErrorsError::ComplexError(rpcv2cbor::error::ComplexError {
            top_level: Some("top level".to_owned()),
            nested: Some(rpcv2cbor::model::ComplexNestedErrorData {
                foo: Some("bar".to_owned()),
            }),
        }),
    );
    let schema = IntoResponse::<RpcV2Cbor>::into_response(
        rpcv2cbor_schema::error::GreetingWithErrorsError::ComplexError(
            rpcv2cbor_schema::error::ComplexError {
                top_level: Some("top level".to_owned()),
                nested: Some(rpcv2cbor_schema::model::ComplexNestedErrorData {
                    foo: Some("bar".to_owned()),
                }),
            },
        ),
    );
    assert_identical("rpcv2Cbor ComplexError", legacy, schema).await;
}

// ---------------------------------------------------------------------------
// Event-stream operation errors (pokemon-service-server-sdk, restJson1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pokemon_eventstream_error_body_and_status() {
    // Pre-first-event errors on a streaming operation traverse the normal HTTP
    // error path, but the legacy serializer resolves content-type from the
    // *operation* (`application/vnd.amazon.eventstream` over a JSON body —
    // register A2's quirk). Event-stream operations keep the legacy
    // `IntoResponse` even in flag-on crates for exactly this reason, so this
    // golden calls `serialize_error` directly on the flag-on crate's error to
    // pin what the seam WOULD produce: `application/json`; everything else must
    // match.
    let legacy = IntoResponse::<RestJson1>::into_response(
        pokemon_service_server_sdk::error::CapturePokemonError::UnsupportedRegionError(
            pokemon_service_server_sdk::error::UnsupportedRegionError {
                region: "Kanto".to_owned(),
            },
        ),
    );
    let schema_error = pokemon_service_server_sdk_schema::error::UnsupportedRegionError {
        region: "Kanto".to_owned(),
    };
    let schema = RestJson1.serialize_error(&schema_error);

    let (legacy_status, legacy_headers, legacy_body) = parts(legacy).await;
    let (schema_status, schema_headers, schema_body) = parts(schema).await;

    assert_eq!(legacy_status, schema_status, "status mismatch");
    assert_eq!(legacy_body, schema_body, "body mismatch");

    let content_type = |headers: &[(String, String)]| {
        headers
            .iter()
            .find(|(k, _)| k == "content-type")
            .map(|(_, v)| v.clone())
    };
    // Pin the known divergence explicitly so a change on either side is noticed.
    assert_eq!(
        content_type(&legacy_headers).as_deref(),
        Some("application/vnd.amazon.eventstream"),
        "legacy event-stream content-type quirk changed",
    );
    assert_eq!(
        content_type(&schema_headers).as_deref(),
        Some("application/json"),
        "schema-path content-type changed",
    );
    // All other headers must match.
    let strip_ct = |headers: Vec<(String, String)>| {
        headers
            .into_iter()
            .filter(|(k, _)| k != "content-type")
            .collect::<Vec<_>>()
    };
    assert_eq!(
        strip_ct(legacy_headers),
        strip_ct(schema_headers),
        "non-content-type headers mismatch"
    );
}
