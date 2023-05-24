/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::interceptors::{
    BeforeTransmitInterceptorContextRef, BoxError, Interceptor,
};
use aws_smithy_runtime_api::config_bag::ConfigBag;

#[derive(Debug, Clone, Copy)]
pub struct RequestAttempts {
    attempts: u32,
}

impl RequestAttempts {
    #[cfg(any(feature = "test-util", test))]
    pub fn new(attempts: u32) -> Self {
        Self { attempts }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    fn increment(&mut self) {
        self.attempts += 1;
    }
}

#[derive(Debug, Default)]
pub struct RequestAttemptsInterceptor {}

impl RequestAttemptsInterceptor {
    pub fn new() -> Self {
        Self {}
    }
}

impl Interceptor for RequestAttemptsInterceptor {
    fn read_before_attempt(
        &self,
        _ctx: &BeforeTransmitInterceptorContextRef<'_>,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let mut request_attempts: RequestAttempts = cfg
            .get()
            .cloned()
            .unwrap_or_else(|| RequestAttempts { attempts: 0 });
        request_attempts.increment();
        cfg.put(request_attempts);

        Ok(())
    }
}
