/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::config::endpoint::{DefaultResolver, Params, ResolveEndpoint};

pub fn resolve_s3_outposts_endpoint() {
    let resolver = DefaultResolver::new();

    let params = Params::builder()
        .set_region(Some("us-west-2".to_owned()))
        .set_bucket(Some(
            "arn:aws:s3-outposts:us-west-2:123456789012:outpost/op-01234567890123456/accesspoint/reports".to_owned()
        ))
        .set_key(Some("key".to_owned()))
        .build()
        .expect("valid params");

    let _ = resolver.resolve_endpoint(&params);
}

pub fn resolve_s3_accesspoint_endpoint() {
    let resolver = DefaultResolver::new();

    let params = Params::builder()
        .set_region(Some("us-west-2".to_owned()))
        .set_bucket(Some(
            "arn:aws:s3:us-west-2:123456789012:accesspoint:myendpoint".to_owned(),
        ))
        .set_key(Some("key".to_owned()))
        .build()
        .expect("valid params");

    let _ = resolver.resolve_endpoint(&params);
}

pub fn resolve_s3express_endpoint() {
    let resolver = DefaultResolver::new();

    let params = Params::builder()
        .set_region(Some("us-east-1".to_owned()))
        .set_bucket(Some("mybucket--abcd-ab1--x-s3".to_owned()))
        .set_key(Some("key".to_owned()))
        .build()
        .expect("valid params");

    let _ = resolver.resolve_endpoint(&params);
}

pub fn resolve_s3_path_style_endpoint() {
    let resolver = DefaultResolver::new();

    let params = Params::builder()
        .set_region(Some("us-west-2".to_owned()))
        .set_bucket(Some("bucket-name".to_owned()))
        .set_key(Some("key".to_owned()))
        .set_force_path_style(Some(true))
        .build()
        .expect("valid params");

    let _ = resolver.resolve_endpoint(&params);
}
