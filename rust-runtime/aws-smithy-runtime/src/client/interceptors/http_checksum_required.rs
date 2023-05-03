/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::interceptors::error::BoxError;
use aws_smithy_runtime_api::client::interceptors::{Interceptor, InterceptorContext};
use aws_smithy_runtime_api::config_bag::ConfigBag;
use http::header::HeaderName;
use md5::{Digest, Md5};

/// Plugin that adds legacy MD5 checksums to requests. This will panic for streaming requests. Only
/// non-streaming requests are supported.
#[derive(Debug, Default)]
pub struct HttpChecksumRequiredInterceptor {}

impl HttpChecksumRequiredInterceptor {
    /// Creates a new `HttpChecksumRequiredInterceptor`
    pub fn new() -> Self {
        Self::default()
    }
}

impl Interceptor for HttpChecksumRequiredInterceptor {
    fn modify_before_signing(
        &self,
        ctx: &mut InterceptorContext,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let req = ctx.request_mut()?;
        let data = req
            .body()
            .bytes()
            .expect("checksum can only be computed for non-streaming operations");

        let checksum = Md5::digest(data);
        req.headers_mut().insert(
            HeaderName::from_static("content-md5"),
            aws_smithy_types::base64::encode(&checksum[..])
                .parse()
                .expect("checksum is valid header value"),
        );

        Ok(())
    }
}
