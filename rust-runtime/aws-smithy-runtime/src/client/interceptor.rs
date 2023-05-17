/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::body::SdkBody;
use aws_smithy_runtime_api::client::interceptors::context::phase::BeforeTransmit;
use aws_smithy_runtime_api::client::interceptors::{BoxError, Interceptor, InterceptorContext};
use aws_smithy_runtime_api::config_bag::ConfigBag;
use std::fmt;
use std::marker::PhantomData;

pub struct RequestMapInterceptor<F, E> {
    f: F,
    _phantom: PhantomData<E>,
}

impl<F, E> fmt::Debug for RequestMapInterceptor<F, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RequestMapInterceptor")
    }
}

impl<F, E> RequestMapInterceptor<F, E> {
    pub fn new(f: F) -> Self {
        Self {
            f,
            _phantom: PhantomData,
        }
    }
}

impl<F, E> Interceptor for RequestMapInterceptor<F, E>
where
    F: Fn(&mut http::Request<SdkBody>) -> Result<(), E> + Send + Sync + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    fn modify_before_signing(
        &self,
        context: &mut InterceptorContext<BeforeTransmit>,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let request = context.request_mut();
        (self.f)(request)?;

        Ok(())
    }
}

pub struct RequestMutationInterceptor<F> {
    f: F,
}

impl<F> fmt::Debug for RequestMutationInterceptor<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RequestMutationInterceptor")
    }
}

impl<F> RequestMutationInterceptor<F> {
    pub fn new(f: F) -> Self {
        Self { f }
    }
}

impl<F> Interceptor for RequestMutationInterceptor<F>
where
    F: Fn(&mut http::Request<SdkBody>) + Send + Sync + 'static,
{
    fn modify_before_signing(
        &self,
        context: &mut InterceptorContext<BeforeTransmit>,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let request = context.request_mut();
        (self.f)(request);

        Ok(())
    }
}
