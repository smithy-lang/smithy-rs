/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

use aws_smithy_http::endpoint::EndpointPrefix;
use aws_smithy_http::middleware::MapRequest;
use aws_smithy_http::operation::Request;
use aws_types::region::SigningRegion;
use aws_types::SigningService;
use aws_types::endpoint::{AwsEndpoint, BoxError, CredentialScope};

#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct AwsEndpointStage {}

#[derive(Debug)]
#[non_exhaustive]
pub enum AwsEndpointStageError {
    NoResolvedEndpoint,
    NoSigningService,
    NoSigningRegion,
    EndpointResolutionError(BoxError),
}

impl Display for AwsEndpointStageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}
impl Error for AwsEndpointStageError {}

impl MapRequest for AwsEndpointStage {
    type Error = AwsEndpointStageError;

    fn apply(&self, request: Request) -> Result<Request, Self::Error> {
        use AwsEndpointStageError::*;

        request.augment(|mut http_req, props| {
            // TODO(Zelda) is removing this prop bad? Nothing else should need to access this
            let endpoint = props
                .remove::<aws_smithy_http::endpoint::Result>()
                .map_or(Err(NoResolvedEndpoint), |res| {
                    res.map_err(|e| EndpointResolutionError(Box::new(e)))
                })?;

            let signing_service = props.get::<SigningService>().ok_or(NoSigningService)?;
            let signing_region = props.get::<SigningRegion>().ok_or(NoSigningRegion)?;
            let credential_scope = CredentialScope::builder()
                .service(signing_service.clone())
                .region(signing_region.clone())
                .build();
            let endpoint = AwsEndpoint::new(endpoint, credential_scope);
            tracing::debug!(endpoint = ?endpoint, base_region = ?signing_region, "resolved endpoint");
            endpoint.set_endpoint(http_req.uri_mut(), props.get::<EndpointPrefix>());

            Ok(http_req)
        })
    }
}
