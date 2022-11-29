/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The [`lambda_http`] types included in [`http::Request`]s when [`LambdaHandler`](crate::routing::LambdaHandler) is
//! used. Each are given a [`FromParts`] implementation for easy use within handlers.

use lambda_http::request::RequestContext;
pub use lambda_http::{
    aws_lambda_events::apigw::{ApiGatewayProxyRequestContext, ApiGatewayV2httpRequestContext},
    Context,
};

use super::{extension::MissingExtension, FromParts};
use crate::Extension;

impl<P> FromParts<P> for Context {
    type Rejection = MissingExtension;

    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        let Extension(context) = <Extension<Self> as FromParts<P>>::from_parts(parts)?;
        Ok(context)
    }
}

impl<P> FromParts<P> for ApiGatewayProxyRequestContext {
    type Rejection = MissingExtension;

    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        let Extension(context) = <Extension<RequestContext> as FromParts<P>>::from_parts(parts)?;
        if let RequestContext::ApiGatewayV1(context) = context {
            Ok(context)
        } else {
            Err(MissingExtension)
        }
    }
}

impl<P> FromParts<P> for ApiGatewayV2httpRequestContext {
    type Rejection = MissingExtension;

    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        let Extension(context) = <Extension<RequestContext> as FromParts<P>>::from_parts(parts)?;
        if let RequestContext::ApiGatewayV2(context) = context {
            Ok(context)
        } else {
            Err(MissingExtension)
        }
    }
}
