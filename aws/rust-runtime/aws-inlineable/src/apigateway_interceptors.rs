/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(dead_code)]

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;
use http_1x::header::ACCEPT;
use http_1x::HeaderValue;

/// Interceptor that adds an Accept header to API Gateway requests.
#[derive(Debug, Default)]
pub(crate) struct AcceptHeaderInterceptor {
    _priv: (),
}

impl Intercept for AcceptHeaderInterceptor {
    fn name(&self) -> &'static str {
        "AcceptHeaderInterceptor"
    }

    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        context
            .request_mut()
            .headers_mut()
            .insert(ACCEPT, HeaderValue::from_static("application/json"));
        Ok(())
    }
}
