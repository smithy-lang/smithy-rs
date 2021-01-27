/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! This modules defines the core, framework agnostic HTTP middleware interface
//! used by the SDK
//!
//! smithy-middleware-tower provides Tower-specific middleware utilities

use crate::operation;
use std::error::Error;
use bytes::{Bytes, Buf};
use std::fmt::Debug;
use crate::response::ParseHttpResponse;
use crate::result::{SdkSuccess, SdkError};
use http_body::Body;

type BoxError = Box<dyn Error + Send + Sync>;

/// `MapRequest` defines a synchronous middleware that transforms an `operation::Request`.
///
/// Typically, these middleware will read configuration from the `PropertyBag` and use it to
/// augment the request. Most fundamental middleware is expressed as `MapRequest`, including
/// signing & endpoint resolution.
///
/// ```rust
/// # use smithy_http::middleware::MapRequest;
/// # use std::convert::Infallible;
/// # use smithy_http::operation;
/// # use http::header::HeaderName;
/// struct AddHeader(&'static str, &'static str);
/// impl MapRequest for AddHeader {
///     type Error = &'static str;
///     fn apply(&self, request: operation::Request) -> Result<operation::Request, Self::Error> {
///         request.augment(|mut request, _| {
///             request.headers_mut().append(
///                 self.0,
///                 self.1.parse().map_err(|_|"must be valid header")?,
///             );
///             Ok(request)
///         })
///     }
/// }
/// ```
pub trait MapRequest {
    type Error: Into<BoxError>;
    fn apply(&self, request: operation::Request) -> Result<operation::Request, Self::Error>;
}

/// Load a response using `handler` to parse the results.
///
/// This function is intended to be used on the response side of a middleware chain.
///
/// Success and failure will be split and mapped into `SdkSuccess` and `SdkError`.
pub async fn load_response<B, T, E, O>(
    mut response: http::Response<B>,
    handler: &O,
) -> Result<SdkSuccess<T>, SdkError<E>>
    where
        B: http_body::Body + Unpin,
        B: From<Bytes> + Debug + 'static,
        B::Error: Error + Send + Sync + 'static,
        O: ParseHttpResponse<B, Output=Result<T, E>>,
{
    if let Some(parsed_response) = handler.parse_unloaded(&mut response) {
        return sdk_result(parsed_response, response);
    }

    let body = match read_body(response.body_mut()).await {
        Ok(body) => body,
        Err(e) => {
            return Err(SdkError::ResponseError {
                raw: response.map(|b| Box::new(b) as _),
                err: Box::new(e),
            });
        }
    };

    let response = response.map(|_| Bytes::from(body));
    let parsed = handler.parse_loaded(&response);
    return sdk_result(parsed, response.map(B::from));
}

async fn read_body<B: http_body::Body>(body: B) -> Result<Vec<u8>, B::Error> {
    let mut output = Vec::new();
    pin_utils::pin_mut!(body);
    while let Some(buf) = body.data().await {
        let mut buf = buf?;
        while buf.has_remaining() {
            output.extend_from_slice(buf.chunk());
            buf.advance(buf.chunk().len())
        }
    }
    Ok(output)
}

fn sdk_result<T, E, B: Debug + 'static>(
    parsed: Result<T, E>,
    raw: http::Response<B>,
) -> Result<SdkSuccess<T>, SdkError<E>> {
    let dyn_body = raw.map(|b| Box::new(b) as _);
    match parsed {
        Ok(parsed) => Ok(SdkSuccess {
            raw: dyn_body,
            parsed,
        }),
        Err(err) => Err(SdkError::ServiceError {
            raw: dyn_body,
            err,
        }),
    }
}
