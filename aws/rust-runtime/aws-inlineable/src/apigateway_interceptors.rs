/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(dead_code)]

use aws_smithy_runtime_api::client::interceptors::{
    BeforeTransmitInterceptorContextMut, BoxError, Interceptor,
};
use aws_smithy_runtime_api::config_bag::ConfigBag;
use http::header::ACCEPT;
use http::HeaderValue;

/// Interceptor that adds an Accept header to API Gateway requests.
#[derive(Debug, Default)]
pub(crate) struct AcceptHeaderInterceptor;

impl Interceptor for AcceptHeaderInterceptor {
    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        context
            .request_mut()
            .headers_mut()
            .insert(ACCEPT, HeaderValue::from_static("application/json"));
        Ok(())
    }
}
