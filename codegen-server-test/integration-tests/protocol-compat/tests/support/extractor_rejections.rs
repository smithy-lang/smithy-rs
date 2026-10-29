/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(dead_code)]

use aws_smithy_http_server::{
    body::{Body, BoxBody},
    extension::RuntimeErrorExtension,
    request::FromParts,
};
use bytes::Bytes;
use http::{HeaderMap, Request};
use http_body_util::BodyExt;
use std::{
    convert::Infallible,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tower::{Service, ServiceExt};

pub const MESSAGE: &str = "Caller identity could not be resolved";

#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub runtime_error: Option<String>,
}

pub async fn send<S>(service: S, request: Request<Body>) -> Reply
where
    S: Service<Request<Body>, Response = http::Response<BoxBody>, Error = Infallible>,
{
    snapshot(service.oneshot(request).await.unwrap()).await
}

pub async fn snapshot(response: http::Response<BoxBody>) -> Reply {
    let (parts, body) = response.into_parts();
    Reply {
        status: parts.status.as_u16(),
        runtime_error: parts
            .extensions
            .get::<RuntimeErrorExtension>()
            .map(|e| e.to_string()),
        headers: parts.headers,
        body: body.collect().await.unwrap().to_bytes(),
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Wire {
    Rest,
    Cbor,
    AwsJson,
    AwsJson11,
    Xml,
}

pub fn request(wire: Wire, service: &str, kind: &str) -> Request<Body> {
    let (method, uri, payload, content_type) = match wire {
        Wire::Rest => ("GET", "/widgets/1".to_owned(), b"".as_slice(), None),
        Wire::Cbor => (
            "POST",
            format!("/service/{service}/operation/GetWidget"),
            b"\xa1\x62id\x611".as_slice(),
            Some("application/cbor"),
        ),
        Wire::Xml => (
            "GET",
            "/widgets/1".to_owned(),
            b"".as_slice(),
            Some("application/xml"),
        ),
        Wire::AwsJson11 => (
            "POST",
            "/".to_owned(),
            br#"{"id":"1"}"#.as_slice(),
            Some("application/x-amz-json-1.1"),
        ),
        Wire::AwsJson => (
            "POST",
            "/".to_owned(),
            br#"{"id":"1"}"#.as_slice(),
            Some("application/x-amz-json-1.0"),
        ),
    };
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-rejection", kind);
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    match wire {
        Wire::Cbor => builder = builder.header("smithy-protocol", "rpc-v2-cbor"),
        Wire::AwsJson | Wire::AwsJson11 => {
            builder = builder.header("x-amz-target", format!("{service}.GetWidget"))
        }
        Wire::Rest | Wire::Xml => {}
    }
    builder
        .body(Body::new(http_body_util::Full::new(
            Bytes::copy_from_slice(payload),
        )))
        .unwrap()
}

macro_rules! runtime_service {
    ($module:ident, $sdk:ident, $service:ident, $config:ident, [$($builder:tt)*], $marker:ty, $error:ty) => {
        pub mod $module {
            use super::*;
            use $sdk::{input::GetWidgetInput, output::GetWidgetOutput, error::GetWidgetError, $service, $config};
            struct Caller;
            impl FromParts<$marker> for Caller {
                type Rejection = $error;
                fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
                    use $error as RuntimeError;
                    match parts.headers.get("x-rejection").unwrap().to_str().unwrap() {
                        "success" => Ok(Caller),
                        "validation" => Err(RuntimeError::Validation(MESSAGE.to_owned().into())),
                        "serialization" => Err(RuntimeError::Serialization(aws_smithy_http_server::error::Error::new(MESSAGE))),
                        "internal" => Err(RuntimeError::InternalFailure(aws_smithy_http_server::error::Error::new(MESSAGE))),
                        "accept" => Err(RuntimeError::NotAcceptable),
                        "content-type" => Err(RuntimeError::UnsupportedMediaType),
                        other => panic!("unexpected rejection kind: {other}"),
                    }
                }
            }
            pub fn service(calls: Arc<AtomicUsize>) -> impl Clone + Service<Request<Body>, Response = http::Response<BoxBody>, Error = Infallible> {
                $service::builder $($builder)* ($config::builder().build())
                    .get_widget(move |input: GetWidgetInput, _: Caller| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        async move { Ok::<_, GetWidgetError>(GetWidgetOutput { id: Some(input.id) }) }
                    })
                    .build_unchecked()
            }

            pub fn streaming_service() -> impl Clone + Service<Request<Body>, Response = http::Response<BoxBody>, Error = Infallible> {
                async fn must_not_run(_: $sdk::input::DownloadBlobInput, _: Caller) -> $sdk::output::DownloadBlobOutput {
                    panic!("a rejected streaming request reached its handler")
                }
                $service::builder $($builder)* ($config::builder().build())
                    .download_blob(must_not_run)
                    .build_unchecked()
            }
        }
    };
}
use aws_smithy_http_server::protocol::{aws_json, aws_json_10, rest_json_1, rpc_v2_cbor};
runtime_service!(legacy_rest, compat_rest_legacy_server_sdk, ReviewRest, ReviewRestConfig, [::<Body, _, _, _>], rest_json_1::RestJson1, rest_json_1::runtime_error::RuntimeError);
runtime_service!(
    schema_rest,
    compat_rest_server_sdk,
    ReviewRest,
    ReviewRestConfig,
    [],
    rest_json_1::RestJson1,
    aws_smithy_http_server::runtime_error::RuntimeError
);
runtime_service!(legacy_cbor, compat_cbor_legacy_server_sdk, ReviewCbor, ReviewCborConfig, [::<Body, _, _, _>], rpc_v2_cbor::RpcV2Cbor, rpc_v2_cbor::runtime_error::RuntimeError);
runtime_service!(
    schema_cbor,
    compat_cbor_server_sdk,
    ReviewCbor,
    ReviewCborConfig,
    [],
    rpc_v2_cbor::RpcV2Cbor,
    aws_smithy_http_server::runtime_error::RuntimeError
);
runtime_service!(legacy_json, compat_awsjson_legacy_server_sdk, ReviewAwsJson, ReviewAwsJsonConfig, [::<Body, _, _, _>], aws_json_10::AwsJson1_0, aws_json::runtime_error::RuntimeError);
runtime_service!(
    schema_json,
    compat_awsjson_server_sdk,
    ReviewAwsJson,
    ReviewAwsJsonConfig,
    [],
    aws_json_10::AwsJson1_0,
    aws_smithy_http_server::runtime_error::RuntimeError
);

runtime_service!(legacy_json11, compat_awsjson11_legacy_server_sdk, ReviewAwsJson11, ReviewAwsJson11Config, [::<Body, _, _, _>], aws_smithy_http_server::protocol::aws_json_11::AwsJson1_1, aws_json::runtime_error::RuntimeError);
runtime_service!(
    schema_json11,
    compat_awsjson11_server_sdk,
    ReviewAwsJson11,
    ReviewAwsJson11Config,
    [],
    aws_smithy_http_server::protocol::aws_json_11::AwsJson1_1,
    aws_smithy_http_server::runtime_error::RuntimeError
);
runtime_service!(legacy_xml, compat_xml_legacy_server_sdk, ReviewXml, ReviewXmlConfig, [::<Body, _, _, _>], aws_smithy_http_server::protocol::rest_xml::RestXml, aws_smithy_http_server::protocol::rest_xml::runtime_error::RuntimeError);
runtime_service!(
    schema_xml,
    compat_xml_server_sdk,
    ReviewXml,
    ReviewXmlConfig,
    [],
    aws_smithy_http_server::protocol::rest_xml::RestXml,
    aws_smithy_http_server::runtime_error::RuntimeError
);

pub const REJECTIONS: [&str; 5] = [
    "validation",
    "serialization",
    "internal",
    "accept",
    "content-type",
];

/// These expectations were recorded by running generated schemaSerde=false services.
/// Keep them independent of the proposed shared runtime-error implementation.
pub fn observed_runtime_rejection(wire: Wire, kind: &str) -> Reply {
    let (status, name) = match kind {
        "validation" => (400, "ValidationException"),
        "serialization" => (400, "SerializationException"),
        "internal" => (500, "InternalFailureException"),
        "accept" => (406, "NotAcceptableException"),
        "content-type" => (415, "UnsupportedMediaTypeException"),
        other => panic!("unexpected rejection kind: {other}"),
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        match wire {
            Wire::Rest => "application/json",
            Wire::Cbor => "application/cbor",
            Wire::AwsJson => "application/x-amz-json-1.0",
            Wire::AwsJson11 => "application/x-amz-json-1.1",
            Wire::Xml => "application/xml",
        }
        .parse()
        .unwrap(),
    );
    if matches!(wire, Wire::Rest) {
        headers.insert("x-amzn-errortype", name.parse().unwrap());
    }
    let body = if kind == "validation" && !matches!(wire, Wire::Xml) {
        Bytes::from_static(MESSAGE.as_bytes())
    } else if matches!(wire, Wire::Cbor) {
        Bytes::from_static(b"\xa0")
    } else if matches!(wire, Wire::AwsJson11) {
        Bytes::new()
    } else {
        Bytes::from_static(b"{}")
    };
    Reply {
        status,
        headers,
        body,
        runtime_error: Some(name.to_owned()),
    }
}

/// Schema CBOR validation errors encode the message as a map; legacy emits raw bytes.
/// All other response fields and rejection bodies retain their legacy expectations.
pub fn assert_schema_runtime_rejection(actual: Reply, wire: Wire, kind: &str) {
    let expected = observed_runtime_rejection(wire, kind);
    assert_eq!(actual.status, expected.status, "{wire:?} {kind}");
    assert_eq!(actual.headers, expected.headers, "{wire:?} {kind}");
    assert_eq!(
        actual.runtime_error, expected.runtime_error,
        "{wire:?} {kind}"
    );
    assert_schema_rejection_body(&actual.body, &expected.body, wire, kind);
}

pub fn assert_schema_rejection_body(actual: &[u8], legacy: &[u8], wire: Wire, kind: &str) {
    if matches!(wire, Wire::Cbor) && kind == "validation" {
        assert_eq!(legacy, MESSAGE.as_bytes(), "legacy CBOR validation body");
        assert_cbor_message(actual, MESSAGE);
    } else {
        assert_eq!(actual, legacy, "{wire:?} {kind}");
    }
}

fn assert_cbor_message(body: &[u8], message: &str) {
    let mut decoder = minicbor::Decoder::new(body);
    let length = decoder.map().expect("CBOR error body must be a map");
    if let Some(length) = length {
        assert_eq!(length, 1, "CBOR error map must contain only the message");
    }
    assert_eq!(decoder.str().unwrap(), "message");
    assert_eq!(decoder.str().unwrap(), message);
    if length.is_none() {
        assert_eq!(decoder.datatype().unwrap(), minicbor::data::Type::Break);
        decoder.skip().unwrap();
    }
    assert_eq!(decoder.position(), body.len(), "trailing CBOR error bytes");
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomResponseMarker(pub &'static str);

pub struct CustomRejection;

impl std::fmt::Display for CustomRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("custom missing caller")
    }
}

/// The inherited contextual conversion preserves this complete custom response.
impl<P> aws_smithy_http_server::response::IntoResponse<P> for CustomRejection {
    fn into_response(self) -> http::Response<BoxBody> {
        http::Response::builder()
            .status(401)
            .header("content-type", "text/plain")
            .header("x-service-rejection", "custom")
            .extension(CustomResponseMarker("preserved"))
            .body(aws_smithy_http_server::body::to_boxed(
                "service chose this response",
            ))
            .unwrap()
    }
}

pub struct CustomCaller;
impl<P> FromParts<P> for CustomCaller {
    type Rejection = CustomRejection;
    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        if parts
            .headers
            .get("x-rejection")
            .is_some_and(|v| v == "success")
        {
            Ok(Self)
        } else {
            Err(CustomRejection)
        }
    }
}

pub async fn assert_custom_rejection<S>(service: S, request: Request<Body>)
where
    S: Service<Request<Body>, Response = http::Response<BoxBody>, Error = Infallible>,
{
    let response = service.oneshot(request).await.unwrap();
    assert_eq!(
        response.extensions().get::<CustomResponseMarker>(),
        Some(&CustomResponseMarker("preserved"))
    );
    let (parts, body) = response.into_parts();
    assert_eq!(parts.status, 401);
    let mut expected = HeaderMap::new();
    expected.insert("content-type", "text/plain".parse().unwrap());
    expected.insert("x-service-rejection", "custom".parse().unwrap());
    assert_eq!(parts.headers, expected);
    assert_eq!(
        body.collect().await.unwrap().to_bytes(),
        "service chose this response"
    );
}
