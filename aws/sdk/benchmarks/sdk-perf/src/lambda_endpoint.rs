/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_lambda::config::endpoint::{DefaultResolver, Params, ResolveEndpoint};

pub fn resolve_lambda_standard_endpoint() {
    let resolver = DefaultResolver::new();

    let params = Params::builder()
        .set_region(Some("us-east-1".to_owned()))
        .set_use_fips(Some(false))
        .set_use_dual_stack(Some(false))
        .build()
        .expect("valid params");

    let _ = resolver.resolve_endpoint(&params);
}

pub fn resolve_lambda_govcloud_fips_dualstack_endpoint() {
    let resolver = DefaultResolver::new();

    let params = Params::builder()
        .set_region(Some("us-gov-east-1".to_owned()))
        .set_use_fips(Some(true))
        .set_use_dual_stack(Some(true))
        .build()
        .expect("valid params");

    let _ = resolver.resolve_endpoint(&params);
}
