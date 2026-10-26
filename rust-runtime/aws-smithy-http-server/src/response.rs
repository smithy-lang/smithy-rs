/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This code was copied and then modified from Tokio's Axum.

/* Copyright (c) 2022 Tower Contributors
 *
 * Permission is hereby granted, free of charge, to any
 * person obtaining a copy of this software and associated
 * documentation files (the "Software"), to deal in the
 * Software without restriction, including without
 * limitation the rights to use, copy, modify, merge,
 * publish, distribute, sublicense, and/or sell copies of
 * the Software, and to permit persons to whom the Software
 * is furnished to do so, subject to the following
 * conditions:
 *
 * The above copyright notice and this permission notice
 * shall be included in all copies or substantial portions
 * of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
 * ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
 * TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
 * PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
 * SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
 * CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
 * IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 * DEALINGS IN THE SOFTWARE.
 */

use crate::body::BoxBody;

pub type Response<T = BoxBody> = http::Response<T>;

/// Converts `self` to an HTTP response for a statically known protocol.
///
/// Generated serde uses this trait. Schema serde uses [`IntoResponseWithProtocol`] instead,
/// because the protocol selected for a request can carry configuration that a marker type cannot.
pub trait IntoResponse<Protocol> {
    /// Performs a conversion into a [`http::Response`].
    fn into_response(self) -> http::Response<BoxBody>;
}

impl<P> IntoResponse<P> for std::convert::Infallible {
    fn into_response(self) -> Response {
        match self {}
    }
}

/// Converts an error or extractor rejection using the protocol selected for this request.
///
/// Every [`crate::schema::HttpModeledError`] implements this trait through the selected
/// protocol's modeled-error serializer. This includes errors defined by downstream packages.
/// The shared [`crate::runtime_error::RuntimeError`] uses that protocol's runtime-error serializer.
///
/// Custom extractor rejections implement this trait to return their own complete HTTP response
/// or to use the selected protocol. They can also implement [`IntoResponse`] independently for
/// generated serde callers. A custom response must retain its status, headers, body and extensions;
/// it is not serialized again by the schema upgrade.
///
/// This trait has no protocol type parameter: a service's protocol tuple describes supported
/// protocols, while the supplied [`crate::schema::ServerProtocol`] determines this response.
pub trait IntoResponseWithProtocol {
    /// Converts `self` into a response using the supplied protocol instance.
    fn into_response_with_protocol(self, protocol: &dyn crate::schema::ServerProtocol) -> Response;
}

impl<E: crate::schema::HttpModeledError> IntoResponseWithProtocol for E {
    fn into_response_with_protocol(self, protocol: &dyn crate::schema::ServerProtocol) -> Response {
        protocol.serialize_error(&self)
    }
}

/// Marker for services whose errors convert context-free through [`IntoResponse<P>`](IntoResponse)
/// for the statically known protocol `P`.
///
/// Named by a generated-serde service's
/// [`ServiceSerde::SerdeKind`](crate::service::ServiceSerde::SerdeKind).
pub struct GeneratedSerde<P>(std::marker::PhantomData<P>);

/// Marker for services whose errors are serialized by the protocol routing selected per request,
/// through [`IntoResponseWithProtocol`].
///
/// Named by a schema-serde service's [`ServiceSerde::SerdeKind`](crate::service::ServiceSerde::SerdeKind).
pub struct SchemaSerde;

/// Converts an error into an HTTP response on behalf of middleware that is generic over the
/// service's serialization system.
///
/// Middleware names the converter through
/// [`ServiceSerde::SerdeKind`](crate::service::ServiceSerde::SerdeKind) and bounds it with the error
/// types it produces. Exactly two implementations exist: [`GeneratedSerde<P>`] ignores the request
/// and uses the error's [`IntoResponse<P>`](IntoResponse); [`SchemaSerde`] reads the
/// [`SelectedOperation`](crate::schema::SelectedOperation) from request extensions and uses the
/// error's [`IntoResponseWithProtocol`] with the selected protocol.
pub trait ErrorConverter<E> {
    /// Converts `error` into a response, consulting `request` extensions when conversion is
    /// contextual.
    fn convert<B>(error: E, request: &http::Request<B>) -> Response;
}

impl<P, E: IntoResponse<P>> ErrorConverter<E> for GeneratedSerde<P> {
    fn convert<B>(error: E, _request: &http::Request<B>) -> Response {
        error.into_response()
    }
}

impl<E: IntoResponseWithProtocol> ErrorConverter<E> for SchemaSerde {
    fn convert<B>(error: E, request: &http::Request<B>) -> Response {
        match crate::schema::SelectedOperation::get_from_request(request) {
            Some(selected) => error.into_response_with_protocol(&**selected.protocol()),
            None => {
                // Middleware discarded the request extensions: no protocol can frame the error.
                tracing::error!(
                    "error conversion reached SchemaSerde without a selected protocol operation; returning an unframed 500"
                );
                let mut response = http::Response::new(crate::body::empty());
                *response.status_mut() = http::StatusCode::INTERNAL_SERVER_ERROR;
                response
                    .extensions_mut()
                    .insert(crate::extension::RuntimeErrorExtension::new(
                        "InternalFailureException".to_owned(),
                    ));
                response
            }
        }
    }
}

#[cfg(test)]
mod error_converter_tests {
    use super::*;
    use crate::schema::{OperationSchema, SelectedOperation, ServerProtocol, SharedServerProtocol};
    use aws_smithy_schema::{shape_id, Schema, ShapeType};
    use http_body_util::BodyExt;

    #[derive(Clone, Default)]
    struct LogWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct TestError;

    struct StaticProtocol;

    impl IntoResponse<StaticProtocol> for TestError {
        fn into_response(self) -> Response {
            http::Response::builder()
                .status(http::StatusCode::FORBIDDEN)
                .header("x-conversion", "static")
                .body(crate::body::to_boxed("static denial"))
                .unwrap()
        }
    }

    impl IntoResponseWithProtocol for TestError {
        fn into_response_with_protocol(self, protocol: &dyn ServerProtocol) -> Response {
            http::Response::builder()
                .status(http::StatusCode::FORBIDDEN)
                .header("x-selected-protocol", protocol.protocol_id().to_string())
                .body(crate::body::to_boxed("contextual denial"))
                .unwrap()
        }
    }

    fn routed_request() -> http::Request<()> {
        static EMPTY: Schema<'static> = Schema::new_struct(shape_id!("test", "Empty"), ShapeType::Structure, &[]);
        static SCHEMA: OperationSchema<'static> =
            OperationSchema::new(shape_id!("test", "Operation"), &EMPTY, &EMPTY, &[]);
        let mut request = http::Request::new(());
        SelectedOperation::new(
            SharedServerProtocol::metadata_routed(crate::schema::protocol::RestJson1::default()),
            &SCHEMA,
            Default::default(),
        )
        .insert_into(request.extensions_mut());
        request
    }

    #[tokio::test]
    async fn generated_serde_converts_context_free_and_ignores_the_request() {
        // No `SelectedOperation` extension is present; the static conversion must not need one.
        let response = <GeneratedSerde<StaticProtocol>>::convert(TestError, &http::Request::new(()));
        assert_eq!(response.status(), http::StatusCode::FORBIDDEN);
        assert_eq!(response.headers()["x-conversion"], "static");
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "static denial"
        );
    }

    #[tokio::test]
    async fn schema_serde_converts_with_the_routed_protocol() {
        let request = routed_request();
        let expected_id = SelectedOperation::get_from_request(&request)
            .unwrap()
            .protocol()
            .protocol_id()
            .to_string();
        let response = SchemaSerde::convert(TestError, &request);
        assert_eq!(response.status(), http::StatusCode::FORBIDDEN);
        assert_eq!(response.headers()["x-selected-protocol"], expected_id);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "contextual denial"
        );
    }

    #[tokio::test]
    async fn schema_serde_without_a_selection_returns_an_empty_internal_error() {
        let writer = LogWriter::default();
        let sink = writer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || sink.clone())
            .finish();
        let response =
            tracing::subscriber::with_default(subscriber, || SchemaSerde::convert(TestError, &http::Request::new(())));
        let log = String::from_utf8(writer.0.lock().unwrap().clone()).unwrap();
        assert!(log.contains("ERROR"), "{log}");
        assert!(
            log.contains(
                "error conversion reached SchemaSerde without a selected protocol operation; returning an unframed 500"
            ),
            "{log}"
        );
        assert_eq!(response.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response
                .extensions()
                .get::<crate::extension::RuntimeErrorExtension>()
                .expect("the fallback response must classify the runtime error")
                .as_str(),
            "InternalFailureException"
        );
        assert!(response.headers().is_empty());
        assert!(response.into_body().collect().await.unwrap().to_bytes().is_empty());
    }
}
