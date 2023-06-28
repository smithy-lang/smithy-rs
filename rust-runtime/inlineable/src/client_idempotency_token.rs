/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::idempotency_token::IdempotencyTokenProvider;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeSerializationInterceptorContextMut, Input,
};
use aws_smithy_runtime_api::client::interceptors::{
    Interceptor, InterceptorRegistrar, SharedInterceptor,
};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::config_bag::ConfigBag;
use std::fmt;

#[derive(Debug)]
pub(crate) struct IdempotencyTokenRuntimePlugin {
    interceptor: SharedInterceptor,
}

impl IdempotencyTokenRuntimePlugin {
    pub(crate) fn new<S>(set_token: S) -> Self
    where
        S: Fn(IdempotencyTokenProvider, &mut Input) + Send + Sync + 'static,
    {
        Self {
            interceptor: SharedInterceptor::new(IdempotencyTokenInterceptor { set_token }),
        }
    }
}

impl RuntimePlugin for IdempotencyTokenRuntimePlugin {
    fn interceptors(&self, interceptors: &mut InterceptorRegistrar) {
        interceptors.register(self.interceptor.clone());
    }
}

struct IdempotencyTokenInterceptor<S> {
    set_token: S,
}

impl<S> fmt::Debug for IdempotencyTokenInterceptor<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdempotencyTokenInterceptor").finish()
    }
}

impl<S> Interceptor for IdempotencyTokenInterceptor<S>
where
    S: Fn(IdempotencyTokenProvider, &mut Input) + Send + Sync,
{
    fn modify_before_serialization(
        &self,
        context: &mut BeforeSerializationInterceptorContextMut<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let token_provider = cfg
            .load::<IdempotencyTokenProvider>()
            .expect("the idempotency provider must be set")
            .clone();
        (self.set_token)(token_provider, context.input_mut());
        Ok(())
    }
}
