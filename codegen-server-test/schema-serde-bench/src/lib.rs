//! Shared bench cases: the golden shapes from
//! `wire-capture/tests/schema_serde_goldens.rs`, each exposing the error value
//! and response path for both crates of a `schemaSerde` flag pair:
//!
//! - legacy: `IntoResponse<P>` on the flag-OFF crate's operation error enum —
//!   the generated per-protocol serializers;
//! - schema: `IntoResponse<P>` on the flag-ON (`*-schema`) crate's operation
//!   error enum, which delegates to `ServerProtocol::serialize_error` (the
//!   legacy serializers are not even generated in that crate).
//!
//! Both sides do the same work: enum dispatch + status + headers + body
//! assembly.

use aws_smithy_http_server::body::BoxBody;
use aws_smithy_http_server::protocol::aws_json_11::AwsJson1_1;
use aws_smithy_http_server::protocol::rest_json_1::RestJson1;
use aws_smithy_http_server::protocol::rpc_v2_cbor::RpcV2Cbor;
use aws_smithy_http_server::response::IntoResponse;
use bytes::Bytes;
use http_body_util::BodyExt;

/// Fully drains a response so no lazy body work escapes measurement.
pub async fn drain(response: http::Response<BoxBody>) -> Bytes {
    let (_parts, body) = response.into_parts();
    body.collect().await.expect("body").to_bytes()
}

/// restJson1 `ValidationException` (message + 1-entry `fieldList`) — the hot
/// validation-rejection shape.
pub mod validation_exception {
    use super::*;
    pub type LegacyError = constraints::error::ValidationException;
    pub type SchemaError = constraints_schema::error::ValidationException;

    const MESSAGE: &str = "1 validation error detected. Value with length 1 at '/conA/lengthString' failed to satisfy constraint: Member must have length between 2 and 69, inclusive";
    const FIELD_MESSAGE: &str = "Value with length 1 at '/conA/lengthString' failed to satisfy constraint: Member must have length between 2 and 69, inclusive";
    const PATH: &str = "/conA/lengthString";

    pub fn legacy_error() -> LegacyError {
        LegacyError {
            message: MESSAGE.to_owned(),
            field_list: Some(vec![constraints::model::ValidationExceptionField {
                path: PATH.to_owned(),
                message: FIELD_MESSAGE.to_owned(),
            }]),
        }
    }

    pub fn schema_error() -> SchemaError {
        SchemaError {
            message: MESSAGE.to_owned(),
            field_list: Some(vec![constraints_schema::model::ValidationExceptionField {
                path: PATH.to_owned(),
                message: FIELD_MESSAGE.to_owned(),
            }]),
        }
    }

    pub fn legacy(error: LegacyError) -> http::Response<BoxBody> {
        IntoResponse::<RestJson1>::into_response(
            constraints::error::ConstrainedShapesOperationError::ValidationException(error),
        )
    }

    pub fn schema(error: SchemaError) -> http::Response<BoxBody> {
        IntoResponse::<RestJson1>::into_response(
            constraints_schema::error::ConstrainedShapesOperationError::ValidationException(error),
        )
    }
}

/// restJson1 `ComplexError` with an `@httpHeader`-bound member — measures the
/// header-split cost.
pub mod complex_error_header {
    use super::*;
    pub type LegacyError = rest_json::error::ComplexError;
    pub type SchemaError = rest_json_schema::error::ComplexError;

    pub fn legacy_error() -> LegacyError {
        LegacyError {
            header: Some("header-value".to_owned()),
            top_level: Some("top level".to_owned()),
            nested: Some(rest_json::model::ComplexNestedErrorData {
                foo: Some("bar".to_owned()),
            }),
        }
    }

    pub fn schema_error() -> SchemaError {
        SchemaError {
            header: Some("header-value".to_owned()),
            top_level: Some("top level".to_owned()),
            nested: Some(rest_json_schema::model::ComplexNestedErrorData {
                foo: Some("bar".to_owned()),
            }),
        }
    }

    pub fn legacy(error: LegacyError) -> http::Response<BoxBody> {
        IntoResponse::<RestJson1>::into_response(
            rest_json::error::GreetingWithErrorsError::ComplexError(error),
        )
    }

    pub fn schema(error: SchemaError) -> http::Response<BoxBody> {
        IntoResponse::<RestJson1>::into_response(
            rest_json_schema::error::GreetingWithErrorsError::ComplexError(error),
        )
    }
}

/// awsJson1.1 `InvalidGreeting` — measures the name-only `__type`
/// discriminator-wrapper cost.
pub mod awsjson11_invalid_greeting {
    use super::*;
    pub type LegacyError = json_rpc11::error::InvalidGreeting;
    pub type SchemaError = json_rpc11_schema::error::InvalidGreeting;

    pub fn legacy_error() -> LegacyError {
        LegacyError {
            message: Some("Hi".to_owned()),
        }
    }

    pub fn schema_error() -> SchemaError {
        SchemaError {
            message: Some("Hi".to_owned()),
        }
    }

    pub fn legacy(error: LegacyError) -> http::Response<BoxBody> {
        IntoResponse::<AwsJson1_1>::into_response(
            json_rpc11::error::GreetingWithErrorsError::InvalidGreeting(error),
        )
    }

    pub fn schema(error: SchemaError) -> http::Response<BoxBody> {
        IntoResponse::<AwsJson1_1>::into_response(
            json_rpc11_schema::error::GreetingWithErrorsError::InvalidGreeting(error),
        )
    }
}

/// rpcv2Cbor `InvalidGreeting` — measures the full-ID-first `__type` map entry
/// plus the `smithy-protocol` header.
pub mod rpcv2cbor_invalid_greeting {
    use super::*;
    pub type LegacyError = rpcv2cbor::error::InvalidGreeting;
    pub type SchemaError = rpcv2cbor_schema::error::InvalidGreeting;

    pub fn legacy_error() -> LegacyError {
        LegacyError {
            message: Some("Hi".to_owned()),
        }
    }

    pub fn schema_error() -> SchemaError {
        SchemaError {
            message: Some("Hi".to_owned()),
        }
    }

    pub fn legacy(error: LegacyError) -> http::Response<BoxBody> {
        IntoResponse::<RpcV2Cbor>::into_response(
            rpcv2cbor::error::GreetingWithErrorsError::InvalidGreeting(error),
        )
    }

    pub fn schema(error: SchemaError) -> http::Response<BoxBody> {
        IntoResponse::<RpcV2Cbor>::into_response(
            rpcv2cbor_schema::error::GreetingWithErrorsError::InvalidGreeting(error),
        )
    }
}
