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

use http::request::Parts;

use crate::Extension;

use super::FromParts;

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
    type Rejection = <Extension<Self> as FromParts<P>>::Rejection;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let Extension(connect_info) = <Extension<Self> as FromParts<P>>::from_parts(parts)?;
        Ok(connect_info)
    }
}
