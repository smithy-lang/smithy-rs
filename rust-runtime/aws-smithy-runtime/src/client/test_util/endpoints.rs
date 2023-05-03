/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::orchestrator::EndpointResolverParams;

#[derive(Debug, Default)]
pub struct EmptyEndpointResolverParams {}

impl EmptyEndpointResolverParams {
    pub fn new() -> EmptyEndpointResolverParams {
        Self::default()
    }
}

impl From<EmptyEndpointResolverParams> for EndpointResolverParams {
    fn from(params: EmptyEndpointResolverParams) -> Self {
        Self::new(params)
    }
}
