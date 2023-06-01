/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_http::user_agent::AwsUserAgent;
use aws_runtime::invocation_id::InvocationId;
use aws_smithy_async::test_util::StaticTimeSource;
use aws_smithy_runtime_api::client::interceptors::{
    BeforeTransmitInterceptorContextMut, Interceptor, InterceptorRegistrar,
};
use aws_smithy_runtime_api::client::orchestrator::ConfigBagAccessors;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::config_bag::ConfigBag;
use http::header::USER_AGENT;
use http::{HeaderName, HeaderValue};
use std::time::SystemTime;

pub const X_AMZ_USER_AGENT: HeaderName = HeaderName::from_static("x-amz-user-agent");

#[derive(Debug)]
pub struct FixupPlugin;
impl RuntimePlugin for FixupPlugin {
    fn configure(
        &self,
        cfg: &mut ConfigBag,
        _interceptors: &mut InterceptorRegistrar,
    ) -> Result<(), aws_smithy_runtime_api::client::runtime_plugin::BoxError> {
        cfg.put(InvocationId::for_tests());
        Ok(())
    }
}

#[derive(Debug)]
pub struct TestUserAgentInterceptor;
impl Interceptor for TestUserAgentInterceptor {
    fn modify_before_signing(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _cfg: &mut ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::client::interceptors::BoxError> {
        let headers = context.request_mut().headers_mut();
        let user_agent = AwsUserAgent::for_tests();
        // Overwrite user agent header values provided by `UserAgentInterceptor`
        headers.insert(USER_AGENT, HeaderValue::try_from(user_agent.ua_header())?);
        headers.insert(
            X_AMZ_USER_AGENT,
            HeaderValue::try_from(user_agent.aws_ua_header())?,
        );

        Ok(())
    }
}
