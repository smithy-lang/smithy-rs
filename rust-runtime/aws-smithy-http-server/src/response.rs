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

/// A protocol aware function taking `self` to [`http::Response`].
///
/// Every [`crate::schema::HttpModeledError`] implements this trait for every protocol type,
/// including tuples. Supply the selected protocol to [`Self::into_response_with_protocol`]
/// to serialize its complete modeled response. Without that context, its conversion returns
/// an empty body with the modeled status and a [`crate::extension::ModeledErrorExtension`],
/// without protocol headers.
pub trait IntoResponse<Protocol> {
    /// Performs a conversion into a [`http::Response`].
    fn into_response(self) -> http::Response<BoxBody>;

    /// Converts an error or extractor rejection using the protocol selected for this request.
    ///
    /// The default preserves an existing implementation's complete HTTP response. Override
    /// this method to frame a rejection through the selected protocol, as
    /// [`crate::runtime_error::RuntimeError`] and the modeled-error blanket implementation do.
    /// Schema upgrades call this method for extractor rejections; middleware returning modeled
    /// errors must also supply its selected protocol. Direct calls and legacy upgrades continue
    /// to use [`Self::into_response`].
    fn into_response_with_protocol(self, _protocol: &dyn crate::schema::ServerProtocol) -> http::Response<BoxBody>
    where
        Self: Sized,
    {
        self.into_response()
    }
}

impl<P, E: crate::schema::HttpModeledError> IntoResponse<P> for E {
    fn into_response(self) -> Response {
        let mut response = http::Response::new(crate::body::empty());
        *response.status_mut() =
            http::StatusCode::from_u16(self.status_code()).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
        crate::schema::protocol::response::stamp_error_extension(response, self.schema().shape_id().shape_name())
    }

    fn into_response_with_protocol(self, protocol: &dyn crate::schema::ServerProtocol) -> Response {
        protocol.serialize_error(&self)
    }
}
