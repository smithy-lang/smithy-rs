/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The same handlers mounted on every generated review service, one module per protocol set.

#[macro_use]
mod handlers;

/// Request-body construction shared with the mainline recorder, whose body type differs.
pub mod kit {
    pub use compat_rest_server_sdk::server::body::Body;

    pub fn from_bytes(bytes: &[u8]) -> Body {
        Body::new(http_body_util::Full::new(bytes::Bytes::copy_from_slice(bytes)))
    }
}

compat_service!(rest, compat_rest_server_sdk, ReviewRest, ReviewRestConfig, [], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), ReviewRest);
compat_service!(rest_cbor, compat_rest_cbor_server_sdk, ReviewRestCbor, ReviewRestCborConfig, [], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), ReviewRestCbor);
compat_service!(rest_awsjson, compat_rest_awsjson_server_sdk, ReviewRestAwsJson, ReviewRestAwsJsonConfig, [], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), ReviewRestAwsJson);
compat_service!(rest_xml, compat_rest_xml_server_sdk, ReviewRestXml, ReviewRestXmlConfig, [], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), ReviewRestXml);
compat_service!(awsjson, compat_awsjson_server_sdk, ReviewAwsJson, ReviewAwsJsonConfig, [], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), ReviewAwsJson);
compat_service!(awsjson_spec, compat_awsjson_spec_server_sdk, ReviewAwsJson, ReviewAwsJsonConfig, [], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), ReviewAwsJson);
compat_service!(cbor, compat_cbor_server_sdk, ReviewCbor, ReviewCborConfig, [], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), ReviewCbor);
compat_service!(all, compat_all_server_sdk, ReviewAll, ReviewAllConfig, [], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), ReviewAll);

/// `ReviewDocument` serves restJson1 only: restXml has no document type.
pub mod document {
    use compat_document_server_sdk::input::NullDocumentInput;
    use compat_document_server_sdk::output::NullDocumentOutput;
    pub use super::kit::Body;
    pub use compat_document_server_sdk::{ReviewDocument, ReviewDocumentConfig};

    async fn null_document(input: NullDocumentInput) -> NullDocumentOutput {
        NullDocumentOutput {
            kind: Some(format!("{:?}", input.doc)),
        }
    }

    pub fn service() -> ReviewDocument {
        ReviewDocument::builder(ReviewDocumentConfig::builder().build())
            .null_document(null_document)
            .build()
            .expect("every operation has a handler")
    }
}

// Legacy (schemaSerde=false, HTTP 1.x) twins of the sole-protocol services. rpcv2Cbor's legacy model
// transform turns the streaming blob into a plain blob.
compat_service!(legacy_rest, compat_rest_legacy_server_sdk, ReviewRest, ReviewRestConfig, [::<Body, _, _, _>], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), impl Clone + tower::Service<http::Request<Body>, Response = http::Response<aws_smithy_http_server::body::BoxBody>, Error = std::convert::Infallible>);
compat_service!(legacy_awsjson, compat_awsjson_legacy_server_sdk, ReviewAwsJson, ReviewAwsJsonConfig, [::<Body, _, _, _>], aws_smithy_types::byte_stream::ByteStream::from_static(b"raw blob bytes"), impl Clone + tower::Service<http::Request<Body>, Response = http::Response<aws_smithy_http_server::body::BoxBody>, Error = std::convert::Infallible>);
compat_service!(legacy_cbor, compat_cbor_legacy_server_sdk, ReviewCbor, ReviewCborConfig, [::<Body, _, _, _>], aws_smithy_types::Blob::new(b"raw blob bytes".to_vec()), impl Clone + tower::Service<http::Request<Body>, Response = http::Response<aws_smithy_http_server::body::BoxBody>, Error = std::convert::Infallible>);

pub mod legacy_document {
    use compat_document_legacy_server_sdk::input::NullDocumentInput;
    use compat_document_legacy_server_sdk::output::NullDocumentOutput;
    pub use super::kit::Body;
    pub use compat_document_legacy_server_sdk::{ReviewDocument, ReviewDocumentConfig};

    async fn null_document(input: NullDocumentInput) -> NullDocumentOutput {
        NullDocumentOutput {
            kind: Some(format!("{:?}", input.doc)),
        }
    }

    pub fn service() -> impl Clone + tower::Service<http::Request<Body>, Response = http::Response<aws_smithy_http_server::body::BoxBody>, Error = std::convert::Infallible> {
        ReviewDocument::builder::<Body, _, _, _>(ReviewDocumentConfig::builder().build())
            .null_document(null_document)
            .build()
            .expect("every operation has a handler")
    }
}

/// A `ReviewRest` whose operation services sit behind a layer that tampers with the routed
/// request's `SelectedOperation` extension, to exercise the upgrade layer's defensive paths.
pub mod tampered {
    use std::sync::Mutex;

    use aws_smithy_http_server::schema::SelectedOperation;
    use compat_rest_server_sdk::input::{GetWidgetInput, PingInput};
    use compat_rest_server_sdk::output::{GetWidgetOutput, PingOutput};
    use compat_rest_server_sdk::error::GetWidgetError;
    use compat_rest_server_sdk::{ReviewRest, ReviewRestConfig};

    use super::kit::Body;

    static CAPTURED: Mutex<Option<SelectedOperation>> = Mutex::new(None);

    async fn get_widget(input: GetWidgetInput) -> Result<GetWidgetOutput, GetWidgetError> {
        Ok(GetWidgetOutput { id: Some(input.id) })
    }

    async fn ping(_: PingInput) -> PingOutput {
        PingOutput {
            message: Some("pong".to_owned()),
        }
    }

    /// `GET /widgets/..` requests record their selection. `POST /ping` requests then either carry
    /// that (wrong) selection (`x-tamper: swap`) or none at all (`x-tamper: drop`).
    fn tamper(mut request: http::Request<Body>) -> http::Request<Body> {
        if request.uri().path().starts_with("/widgets") {
            *CAPTURED.lock().unwrap() = SelectedOperation::get_from_request(&request).cloned();
        } else {
            match request.headers().get("x-tamper").and_then(|v| v.to_str().ok()) {
                Some("swap") => {
                    let wrong = CAPTURED.lock().unwrap().clone().expect("send a GET /widgets request first");
                    request.extensions_mut().insert(wrong);
                }
                Some("drop") => {
                    request.extensions_mut().remove::<SelectedOperation>();
                }
                _ => {}
            }
        }
        request
    }

    pub fn service() -> impl Clone + tower::Service<http::Request<Body>, Response = http::Response<aws_smithy_http_server::body::BoxBody>, Error = std::convert::Infallible> {
        let layer = tower::util::MapRequestLayer::new(tamper as fn(http::Request<Body>) -> http::Request<Body>);
        ReviewRest::builder(ReviewRestConfig::builder().layer(layer).build())
            .get_widget(get_widget)
            .ping(ping)
            .build_unchecked()
    }
}

/// A custom extractor written for one protocol, as service teams write them today.
pub mod caller {
    use aws_smithy_http_server::body::{to_boxed, BoxBody};
    use aws_smithy_http_server::protocol::rest_json_1::RestJson1;
    use aws_smithy_http_server::request::FromParts;
    use aws_smithy_http_server::response::IntoResponse;
    use compat_rest_server_sdk::error::GetWidgetError;
    use compat_rest_server_sdk::input::GetWidgetInput;
    use compat_rest_server_sdk::output::GetWidgetOutput;
    use compat_rest_server_sdk::{ReviewRest, ReviewRestConfig};

    pub struct Caller(pub String);

    pub struct MissingCaller;

    impl std::fmt::Display for MissingCaller {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("missing x-caller")
        }
    }

    impl IntoResponse<RestJson1> for MissingCaller {
        fn into_response(self) -> http::Response<BoxBody> {
            http::Response::builder()
                .status(401)
                .header("content-type", "application/json")
                .header("x-amzn-errortype", "MissingCaller")
                .body(to_boxed(r#"{"message":"missing x-caller"}"#))
                .unwrap()
        }
    }

    impl FromParts<RestJson1> for Caller {
        type Rejection = MissingCaller;
        fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
            parts
                .headers
                .get("x-caller")
                .and_then(|v| v.to_str().ok())
                .map(|v| Caller(v.to_owned()))
                .ok_or(MissingCaller)
        }
    }

    async fn get_widget(input: GetWidgetInput, caller: Caller) -> Result<GetWidgetOutput, GetWidgetError> {
        Ok(GetWidgetOutput {
            id: Some(format!("{}:{}", caller.0, input.id)),
        })
    }

    /// A sole-restJson1 schema service whose handler takes the `FromParts<RestJson1>` extractor.
    pub fn service() -> ReviewRest {
        ReviewRest::builder(ReviewRestConfig::builder().build())
            .get_widget(get_widget)
            .build_unchecked()
    }
}

/// A multi-protocol service's protocol type is a tuple, so a single-protocol extractor does not
/// compile there:
///
/// ```compile_fail
/// use compat_all_server_sdk::{input::GetWidgetInput, output::GetWidgetOutput, error::GetWidgetError};
/// async fn get_widget(input: GetWidgetInput, _caller: protocol_compat::caller::Caller) -> Result<GetWidgetOutput, GetWidgetError> {
///     Ok(GetWidgetOutput { id: Some(input.id) })
/// }
/// let _ = compat_all_server_sdk::ReviewAll::builder(compat_all_server_sdk::ReviewAllConfig::builder().build())
///     .get_widget(get_widget);
/// ```
///
/// while the same handler compiles on the sole-restJson1 service:
///
/// ```
/// use compat_rest_server_sdk::{input::GetWidgetInput, output::GetWidgetOutput, error::GetWidgetError};
/// async fn get_widget(input: GetWidgetInput, _caller: protocol_compat::caller::Caller) -> Result<GetWidgetOutput, GetWidgetError> {
///     Ok(GetWidgetOutput { id: Some(input.id) })
/// }
/// let _ = compat_rest_server_sdk::ReviewRest::builder(compat_rest_server_sdk::ReviewRestConfig::builder().build())
///     .get_widget(get_widget);
/// ```
pub struct MultiProtocolExtractorsMustBeGeneric;
