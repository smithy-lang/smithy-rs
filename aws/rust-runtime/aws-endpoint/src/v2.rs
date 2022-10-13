/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::AwsEndpointStageError;
use aws_smithy_http::endpoint::EndpointPrefix;
use aws_smithy_http::middleware::MapRequest;
use aws_smithy_http::operation::Request;
use aws_smithy_http::property_bag::PropertyBag;
use aws_types::endpoint::ResolveAwsEndpointV2;
use aws_types::region::SigningRegion;
use aws_types::SigningService;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct EndpointStage<T> {
    _phantom: PhantomData<T>,
}

impl<T> EndpointStage<T> {
    pub fn new() -> Self {
        Self {
            _phantom: Default::default(),
        }
    }
}

fn get_resolver<T: Send + Sync + 'static>(
    props: &PropertyBag,
) -> Option<&Arc<dyn ResolveAwsEndpointV2<T>>> {
    props.get::<Arc<dyn ResolveAwsEndpointV2<T>>>()
}

impl<T> MapRequest for EndpointStage<T>
where
    T: Send + Sync + Debug + 'static,
{
    type Error = AwsEndpointStageError;

    fn apply(&self, request: Request) -> Result<Request, Self::Error> {
        request.augment(|mut http_req, props| {
            let provider = get_resolver(props).ok_or(AwsEndpointStageError::NoEndpointResolver)?;
            let params = props.get::<T>().ok_or(AwsEndpointStageError::NoRegion)?;
            let endpoint = provider
                .resolve_endpoint(params)
                .map_err(AwsEndpointStageError::EndpointResolutionError)?;
            tracing::debug!(endpoint = ?endpoint, base_region = ?params, "resolved endpoint");
            if let Some(region) = endpoint.credential_scope().region() {
                props.insert::<SigningRegion>(region.clone());
            }
            if let Some(signing_service) = endpoint.credential_scope().service() {
                props.insert::<SigningService>(signing_service.clone());
            }
            endpoint.set_endpoint(http_req.uri_mut(), props.get::<EndpointPrefix>());
            Ok(http_req)
        })
    }
}
