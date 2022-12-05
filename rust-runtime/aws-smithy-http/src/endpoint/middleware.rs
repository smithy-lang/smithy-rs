/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::endpoint::{apply_endpoint, EndpointPrefix, ResolveEndpointError};
use crate::middleware::MapRequest;
use crate::operation::Request;
use aws_smithy_types::endpoint::Endpoint;
use http::header::HeaderName;
use http::{HeaderValue, Uri};
use std::str::FromStr;

/// Middleware to apply an HTTP endpoint to the request
///
/// This middleware reads [`aws_smithy_types::endpoint::Endpoint`] out of the request properties and applies
/// it to the HTTP request.
#[non_exhaustive]
#[derive(Default, Debug, Clone)]
pub struct SmithyEndpointStage;
impl SmithyEndpointStage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MapRequest for SmithyEndpointStage {
    type Error = ResolveEndpointError;

    fn apply(&self, request: Request) -> Result<Request, Self::Error> {
        request.augment(|mut http_req, props| {
            let endpoint = props
                .get::<Endpoint>()
                .ok_or(ResolveEndpointError::message("no endpoint present"))?;
            let uri: Uri = endpoint.url().parse().map_err(|err| {
                ResolveEndpointError::from_source("endpoint did not have a valid uri", err)
            })?;
            apply_endpoint(http_req.uri_mut(), &uri, props.get::<EndpointPrefix>()).map_err(
                |err| {
                    ResolveEndpointError::message("failed to imply endpoint")
                        .with_source(Some(err.into()))
                },
            )?;
            for (header_name, header_values) in endpoint.headers() {
                http_req.headers_mut().remove(header_name);
                for value in header_values {
                    http_req.headers_mut().insert(
                        HeaderName::from_str(header_name).map_err(|err| {
                            ResolveEndpointError::message("invalid header name")
                                .with_source(Some(err.into()))
                        })?,
                        HeaderValue::from_str(value).map_err(|err| {
                            ResolveEndpointError::message("invalid header value")
                                .with_source(Some(err.into()))
                        })?,
                    );
                }
            }
            Ok(http_req)
        })
    }
}
