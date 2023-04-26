/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::interceptors::{BoxError, Interceptor, InterceptorContext};
use aws_smithy_runtime_api::config_bag::ConfigBag;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RequestAttempts {
    attempts: u32,
}

impl RequestAttempts {
    pub fn new() -> Self {
        Self { attempts: 0 }
    }

    // There is no legitimate reason to set this unless you're testing things.
    // Therefore, this is only available for tests.
    #[cfg(test)]
    pub fn new_with_attempts(attempts: u32) -> Self {
        Self { attempts }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    fn increment(mut self) -> Self {
        self.attempts += 1;
        self
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct RequestAttemptsInterceptor {}

impl RequestAttemptsInterceptor {
    pub fn new() -> Self {
        Self {}
    }
}

impl Interceptor<HttpRequest, HttpResponse> for RequestAttemptsInterceptor {
    fn modify_before_retry_loop(
        &self,
        _ctx: &mut InterceptorContext<HttpRequest, HttpResponse>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        cfg.put(RequestAttempts::new());
        Ok(())
    }

    fn modify_before_transmit(
        &self,
        _ctx: &mut InterceptorContext<HttpRequest, HttpResponse>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(request_attempts) = cfg.get::<RequestAttempts>().cloned() {
            cfg.put(request_attempts.increment());
        }
        Ok(())
    }
}
