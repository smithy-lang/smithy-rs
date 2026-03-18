/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::config::endpoint::{DefaultResolver, Params, ResolveEndpoint};

pub struct S3EndpointBenchmark {
    resolver: DefaultResolver,
    params: Params,
}

impl S3EndpointBenchmark {
    fn new(params: Params) -> Self {
        Self {
            resolver: DefaultResolver::new(),
            params,
        }
    }

    pub fn resolve(&self) {
        let _ = self.resolver.resolve_endpoint(&self.params);
    }

    pub fn s3_outposts() -> Self {
        Self::new(
            Params::builder()
                .set_region(Some("us-west-2".to_owned()))
                .set_bucket(Some(
                    "arn:aws:s3-outposts:us-west-2:123456789012:outpost/op-01234567890123456/accesspoint/reports".to_owned()
                ))
                .set_key(Some("key".to_owned()))
                .build()
                .expect("valid params"),
        )
    }

    pub fn s3_accesspoint() -> Self {
        Self::new(
            Params::builder()
                .set_region(Some("us-west-2".to_owned()))
                .set_bucket(Some(
                    "arn:aws:s3:us-west-2:123456789012:accesspoint:myendpoint".to_owned(),
                ))
                .set_key(Some("key".to_owned()))
                .build()
                .expect("valid params"),
        )
    }

    pub fn s3express() -> Self {
        Self::new(
            Params::builder()
                .set_region(Some("us-east-1".to_owned()))
                .set_bucket(Some("mybucket--abcd-ab1--x-s3".to_owned()))
                .set_key(Some("key".to_owned()))
                .build()
                .expect("valid params"),
        )
    }

    pub fn s3_path_style() -> Self {
        Self::new(
            Params::builder()
                .set_region(Some("us-west-2".to_owned()))
                .set_bucket(Some("bucket-name".to_owned()))
                .set_key(Some("key".to_owned()))
                .set_force_path_style(Some(true))
                .build()
                .expect("valid params"),
        )
    }

    pub fn s3_virtual_addressing() -> Self {
        Self::new(
            Params::builder()
                .set_region(Some("us-west-2".to_owned()))
                .set_bucket(Some("bucket-name".to_owned()))
                .set_key(Some("key".to_owned()))
                .build()
                .expect("valid params"),
        )
    }
}
