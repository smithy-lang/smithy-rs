/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The [`lambda_http`] types included in [`http::Request`]s when [`LambdaHandler`](crate::routing::LambdaHandler) is
//! used. Each are given a [`FromParts`] implementation for easy use within handlers.

use lambda_http::request::RequestContext;
#[doc(inline)]
pub use lambda_http::{
    aws_lambda_events::apigw::{ApiGatewayProxyRequestContext, ApiGatewayV2httpRequestContext},
    Context,
};
use thiserror::Error;

use super::{internal_server_error, FromParts};
use crate::{body::BoxBody, response::IntoResponse};

/// The [`Context`] was not found in the [`http::Request`] extensions.
///
/// Use [`LambdaHandler`](crate::routing::LambdaHandler) to ensure it's present.
#[non_exhaustive]
#[derive(Debug, Error)]
#[error("`Context` is not present in the `http::Request` extensions - consider using `aws_smithy_legacy_http_server::routing::LambdaHandler`")]
pub struct MissingContext;

impl<Protocol> IntoResponse<Protocol> for MissingContext {
    fn into_response(self) -> http::Response<BoxBody> {
        internal_server_error()
    }
}

impl<P> FromParts<P> for Context {
    type Rejection = MissingContext;

    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        parts.extensions.remove().ok_or(MissingContext)
    }
}

#[derive(Debug, Error)]
enum MissingGatewayContextTypeV1 {
    #[error("`RequestContext` is not present in the `http::Request` extensions - consider using `aws_smithy_legacy_http_server::routing::LambdaHandler`")]
    MissingRequestContext,
    #[error("`RequestContext::ApiGatewayV2` is present in the `http::Request` extensions - consider using the `aws_smithy_legacy_http_server::request::lambda::ApiGatewayV2httpRequestContext` extractor")]
    VersionMismatch,
}

/// The [`RequestContext::ApiGatewayV1`] was not found in the [`http::Request`] extensions.
///
/// Use [`LambdaHandler`](crate::routing::LambdaHandler) to ensure it's present and ensure that you're using "ApiGatewayV1".
#[derive(Debug, Error)]
#[error("{inner}")]
pub struct MissingGatewayContextV1 {
    inner: MissingGatewayContextTypeV1,
}

impl<Protocol> IntoResponse<Protocol> for MissingGatewayContextV1 {
    fn into_response(self) -> http::Response<BoxBody> {
        internal_server_error()
    }
}

impl<P> FromParts<P> for ApiGatewayProxyRequestContext {
    type Rejection = MissingGatewayContextV1;

    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        let context = parts.extensions.remove().ok_or(MissingGatewayContextV1 {
            inner: MissingGatewayContextTypeV1::MissingRequestContext,
        })?;
        if let RequestContext::ApiGatewayV1(context) = context {
            Ok(context)
        } else {
            Err(MissingGatewayContextV1 {
                inner: MissingGatewayContextTypeV1::VersionMismatch,
            })
        }
    }
}

#[derive(Debug, Error)]
enum MissingGatewayContextTypeV2 {
    #[error("`RequestContext` is not present in the `http::Request` extensions - consider using `aws_smithy_legacy_http_server::routing::LambdaHandler`")]
    MissingRequestContext,
    #[error("`RequestContext::ApiGatewayV1` is present in the `http::Request` extensions - consider using the `aws_smithy_legacy_http_server::request::lambda::ApiGatewayProxyRequestContext` extractor")]
    VersionMismatch,
}

/// The [`RequestContext::ApiGatewayV2`] was not found in the [`http::Request`] extensions.
///
/// Use [`LambdaHandler`](crate::routing::LambdaHandler) to ensure it's present and ensure that you're using "ApiGatewayV2".
#[derive(Debug, Error)]
#[error("{inner}")]
pub struct MissingGatewayContextV2 {
    inner: MissingGatewayContextTypeV2,
}

impl<Protocol> IntoResponse<Protocol> for MissingGatewayContextV2 {
    fn into_response(self) -> http::Response<BoxBody> {
        internal_server_error()
    }
}

impl<P> FromParts<P> for ApiGatewayV2httpRequestContext {
    type Rejection = MissingGatewayContextV2;

    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        let context = parts.extensions.remove().ok_or(MissingGatewayContextV2 {
            inner: MissingGatewayContextTypeV2::MissingRequestContext,
        })?;
        if let RequestContext::ApiGatewayV2(context) = context {
            Ok(context)
        } else {
            Err(MissingGatewayContextV2 {
                inner: MissingGatewayContextTypeV2::VersionMismatch,
            })
        }
    }
}
