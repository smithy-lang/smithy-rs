/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_lambda::config::endpoint::{DefaultResolver, Params, ResolveEndpoint};

pub struct LambdaEndpointBenchmark {
    resolver: DefaultResolver,
    params: Params,
}

impl LambdaEndpointBenchmark {
    fn new(params: Params) -> Self {
        Self {
            resolver: DefaultResolver::new(),
            params,
        }
    }

    pub fn resolve(&self) {
        let _ = self.resolver.resolve_endpoint(&self.params);
    }

    pub fn standard() -> Self {
        Self::new(
            Params::builder()
                .set_region(Some("us-east-1".to_owned()))
                .set_use_fips(Some(false))
                .set_use_dual_stack(Some(false))
                .build()
                .expect("valid params"),
        )
    }

    pub fn govcloud_fips_dualstack() -> Self {
        Self::new(
            Params::builder()
                .set_region(Some("us-gov-east-1".to_owned()))
                .set_use_fips(Some(true))
                .set_use_dual_stack(Some(true))
                .build()
                .expect("valid params"),
        )
    }
}
