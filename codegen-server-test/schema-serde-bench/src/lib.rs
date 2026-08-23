//! Shared bench cases: the golden shapes from
//! `wire-capture/tests/schema_serde_goldens.rs`, each exposing the error value,
//! the legacy response path (`IntoResponse<P>` on the operation error enum —
//! what the generated server does today), and the schema path
//! (`ServerProtocol::serialize_error`). Both sides do the same work: status +
//! headers + body assembly.

use aws_smithy_http_server::body::BoxBody;
use aws_smithy_http_server::protocol::aws_json_11::AwsJson1_1;
use aws_smithy_http_server::protocol::rest_json_1::RestJson1;
use aws_smithy_http_server::protocol::rpc_v2_cbor::RpcV2Cbor;
use aws_smithy_http_server::protocol::server_protocol::ServerProtocol;
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
    pub type Error = constraints::error::ValidationException;

    pub fn error() -> Error {
        Error {
            message: "1 validation error detected. Value with length 1 at '/conA/lengthString' failed to satisfy constraint: Member must have length between 2 and 69, inclusive".to_owned(),
            field_list: Some(vec![constraints::model::ValidationExceptionField {
                path: "/conA/lengthString".to_owned(),
                message: "Value with length 1 at '/conA/lengthString' failed to satisfy constraint: Member must have length between 2 and 69, inclusive".to_owned(),
            }]),
        }
    }

    pub fn legacy(error: Error) -> http::Response<BoxBody> {
        IntoResponse::<RestJson1>::into_response(
            constraints::error::ConstrainedShapesOperationError::ValidationException(error),
        )
    }

    pub fn schema(error: &Error) -> http::Response<BoxBody> {
        RestJson1.serialize_error(error)
    }
}

/// restJson1 `ComplexError` with an `@httpHeader`-bound member — measures the
/// header-split cost.
pub mod complex_error_header {
    use super::*;
    pub type Error = rest_json::error::ComplexError;

    pub fn error() -> Error {
        Error {
            header: Some("header-value".to_owned()),
            top_level: Some("top level".to_owned()),
            nested: Some(rest_json::model::ComplexNestedErrorData {
                foo: Some("bar".to_owned()),
            }),
        }
    }

    pub fn legacy(error: Error) -> http::Response<BoxBody> {
        IntoResponse::<RestJson1>::into_response(
            rest_json::error::GreetingWithErrorsError::ComplexError(error),
        )
    }

    pub fn schema(error: &Error) -> http::Response<BoxBody> {
        RestJson1.serialize_error(error)
    }
}

/// awsJson1.1 `InvalidGreeting` — measures the name-only `__type`
/// discriminator-wrapper cost.
pub mod awsjson11_invalid_greeting {
    use super::*;
    pub type Error = json_rpc11::error::InvalidGreeting;

    pub fn error() -> Error {
        Error {
            message: Some("Hi".to_owned()),
        }
    }

    pub fn legacy(error: Error) -> http::Response<BoxBody> {
        IntoResponse::<AwsJson1_1>::into_response(
            json_rpc11::error::GreetingWithErrorsError::InvalidGreeting(error),
        )
    }

    pub fn schema(error: &Error) -> http::Response<BoxBody> {
        AwsJson1_1.serialize_error(error)
    }
}

/// rpcv2Cbor `InvalidGreeting` — measures the full-ID-first `__type` map entry
/// plus the `smithy-protocol` header.
pub mod rpcv2cbor_invalid_greeting {
    use super::*;
    pub type Error = rpcv2cbor::error::InvalidGreeting;

    pub fn error() -> Error {
        Error {
            message: Some("Hi".to_owned()),
        }
    }

    pub fn legacy(error: Error) -> http::Response<BoxBody> {
        IntoResponse::<RpcV2Cbor>::into_response(
            rpcv2cbor::error::GreetingWithErrorsError::InvalidGreeting(error),
        )
    }

    pub fn schema(error: &Error) -> http::Response<BoxBody> {
        RpcV2Cbor.serialize_error(error)
    }
}
