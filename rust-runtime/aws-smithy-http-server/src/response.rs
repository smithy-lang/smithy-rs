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
