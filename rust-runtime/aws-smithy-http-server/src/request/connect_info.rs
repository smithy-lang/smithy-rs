/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The [`ConnectInfo`] struct is included in [`http::Request`]s when
//! [`IntoMakeServiceWithConnectInfo`](crate::routing::IntoMakeServiceWithConnectInfo) is used. [`ConnectInfo`]'s
//! [`FromParts`] implementation allows it to be extracted from the [`http::Request`].
//!
//! The [`pokemon-service-connect-info.rs`](https://github.com/awslabs/smithy-rs/blob/main/rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/bin/pokemon-service-connect-info.rs)
//! example illustrates the use of [`IntoMakeServiceWithConnectInfo`](crate::routing::IntoMakeServiceWithConnectInfo)
//! and [`ConnectInfo`] with a service builder.

use http::{request::Parts, StatusCode};
use thiserror::Error;

use crate::{
    body::{empty, BoxBody},
    response::IntoResponse,
};

use super::FromParts;

/// The [`ConnectInfo`] was not found in the [`http::Request`] extensions.
///
/// Use [`IntoMakeServiceWithConnectInfo`](crate::routing::IntoMakeServiceWithConnectInfo) to ensure it's present.

#[non_exhaustive]
#[derive(Debug, Error)]
#[error("the `ConnectInfo` is not present in the `http::Request` - consider using `IntoMakeServiceWithConnectInfo`")]
pub struct MissingConnectInfo;

impl<Protocol> IntoResponse<Protocol> for MissingConnectInfo {
    fn into_response(self) -> http::Response<BoxBody> {
        let mut response = http::Response::new(empty());
        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        response
    }
}

/// Extractor for getting connection information produced by a `Connected`.
///
/// Note this extractor requires the existence of [`Extension<ConnectInfo<T>>`] in the [`http::Extensions`]. This is
/// automatically inserted by the [`IntoMakeServiceWithConnectInfo`](crate::routing::IntoMakeServiceWithConnectInfo)
/// middleware, which can be applied using the `into_make_service_with_connect_info` method on your generated service.
#[derive(Clone, Debug)]
pub struct ConnectInfo<T>(pub T);

impl<P, T> FromParts<P> for ConnectInfo<T>
where
    T: Send + Sync + 'static,
{
    type Rejection = MissingConnectInfo;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        parts.extensions.remove().ok_or(MissingConnectInfo)
    }
}
