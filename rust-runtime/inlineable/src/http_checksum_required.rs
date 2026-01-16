/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::{
    Interceptor, InterceptorRegistrar, SharedInterceptor,
};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::base64;
use aws_smithy_types::config_bag::ConfigBag;
use http_1x::header::HeaderName;

#[derive(Debug)]
pub(crate) struct HttpChecksumRequiredRuntimePlugin;

impl RuntimePlugin for HttpChecksumRequiredRuntimePlugin {
    fn interceptors(&self, interceptors: &mut InterceptorRegistrar) {
        interceptors.register(SharedInterceptor::new(HttpChecksumRequiredInterceptor));
    }
}

#[derive(Debug)]
struct HttpChecksumRequiredInterceptor;

impl Intercept for HttpChecksumRequiredInterceptor {
    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let request = context.request_mut();
        let body_bytes = request
            .body()
            .bytes()
            .expect("checksum can only be computed for non-streaming operations");
        let checksum = <md5::Md5 as md5::Digest>::digest(body_bytes);
        request.headers_mut().insert(
            HeaderName::from_static("content-md5"),
            base64::encode(&checksum[..])
                .parse()
                .expect("checksum is a valid header value"),
        );
        Ok(())
    }
}
