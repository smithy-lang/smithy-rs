/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::orchestrator::EndpointResolverParams;

pub struct EmptyEndpointResolverParams {}

impl EmptyEndpointResolverParams {
    pub fn new() -> EndpointResolverParams {
        EndpointResolverParams::new(Self {})
    }
}
