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

/// Selects response serializers emitted by generated serde code.
#[derive(Clone, Copy, Debug, Default)]
pub struct GeneratedSerde;

/// Selects schema serializers using the protocol selected for each request.
#[derive(Clone, Copy, Debug, Default)]
pub struct SchemaSerde;

/// Converts a middleware error according to the service's serialization mode.
///
/// Generated services select `SerdeMode` in their HTTP plugin bounds. Middleware can
/// carry that mode and use this single bound without choosing a serializer.
/// `P` is the static protocol (or supported-protocol tuple) from `ServiceShape`.
/// Schema mode uses only the routed selection stored in request extensions.
pub trait IntoResponseWithRequest<P, SerdeMode = GeneratedSerde>: Sized {
    /// Converts this error using request metadata when the mode requires it.
    fn into_response_with_request<B>(self, request: &http::Request<B>) -> Response;
}

impl<P, E: IntoResponse<P>> IntoResponseWithRequest<P, GeneratedSerde> for E {
    fn into_response_with_request<B>(self, _request: &http::Request<B>) -> Response {
        IntoResponse::<P>::into_response(self)
    }
}

impl<P, E: IntoResponseWithProtocol> IntoResponseWithRequest<P, SchemaSerde> for E {
    fn into_response_with_request<B>(self, request: &http::Request<B>) -> Response {
        match crate::schema::SelectedOperation::get_from_request(request) {
            Some(selected) => self.into_response_with_protocol(&**selected.protocol()),
            None => {
                tracing::error!("schema middleware reached without a selected operation");
                crate::operation::empty_internal_server_error()
            }
        }
    }
}

#[cfg(test)]
mod mode_tests {
    use super::*;
    use crate::schema::{OperationSchema, RequestBodyCollectionConfig, SelectedOperation};
    use aws_smithy_schema::{shape_id, Schema, ShapeType};
    use http_body_util::BodyExt;

    #[derive(Clone, Debug, PartialEq)]
    struct Extension(&'static str);

    struct CustomError(Response);

    impl IntoResponse<()> for CustomError {
        fn into_response(self) -> Response {
            self.0
        }
    }

    impl IntoResponseWithProtocol for CustomError {
        fn into_response_with_protocol(self, protocol: &dyn crate::schema::ServerProtocol) -> Response {
            assert_eq!(protocol.protocol_id().to_string(), "smithy.protocols#rpcv2Cbor");
            self.0
        }
    }

    fn custom_error() -> CustomError {
        let mut response = Response::builder()
            .status(http::StatusCode::TOO_MANY_REQUESTS)
            .header("x-custom", "unchanged")
            .body(crate::body::from_bytes(bytes::Bytes::from_static(b"custom rejection")))
            .unwrap();
        response.extensions_mut().insert(Extension("unchanged"));
        CustomError(response)
    }

    async fn assert_custom(response: Response) {
        assert_eq!(response.status(), http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().len(), 1);
        assert_eq!(response.headers()["x-custom"], "unchanged");
        assert_eq!(response.extensions().get::<Extension>(), Some(&Extension("unchanged")));
        assert_eq!(response.extensions().len(), 1);
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            "custom rejection"
        );
    }

    #[tokio::test]
    async fn generated_conversion_preserves_complete_response_without_selection() {
        assert_custom(IntoResponseWithRequest::<()>::into_response_with_request(
            custom_error(),
            &http::Request::new(()),
        ))
        .await;
    }

    #[tokio::test]
    async fn schema_tuple_uses_the_selected_protocol_and_preserves_custom_response() {
        static UNIT: Schema<'static> = Schema::new(shape_id!("test", "Unit"), ShapeType::Structure);
        static OP: OperationSchema<'static> = OperationSchema::new(shape_id!("test", "Op"), &UNIT, &UNIT, &[]);
        let mut request = http::Request::new(());
        SelectedOperation::new(
            crate::schema::SharedServerProtocol::metadata_routed(crate::schema::protocol::RpcV2Cbor::default()),
            &OP,
            RequestBodyCollectionConfig::default(),
        )
        .insert_into(request.extensions_mut());
        type Protocols = (crate::schema::protocol::RestJson1, crate::schema::protocol::RpcV2Cbor);
        assert_custom(
            IntoResponseWithRequest::<Protocols, SchemaSerde>::into_response_with_request(custom_error(), &request),
        )
        .await;
        type Reversed = (crate::schema::protocol::RpcV2Cbor, crate::schema::protocol::RestJson1);
        assert_custom(
            IntoResponseWithRequest::<Reversed, SchemaSerde>::into_response_with_request(custom_error(), &request),
        )
        .await;
    }

    #[tokio::test]
    async fn schema_without_selection_returns_the_existing_empty_internal_error() {
        let response = IntoResponseWithRequest::<(), SchemaSerde>::into_response_with_request(
            custom_error(),
            &http::Request::new(()),
        );
        assert_eq!(response.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(response.headers().is_empty());
        assert!(response.extensions().is_empty());
        assert!(response.into_body().collect().await.unwrap().to_bytes().is_empty());
    }
}
