/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Retry support for aws-hyper
//!
//! The actual retry policy implementation will likely be replaced
//! with the CRT implementation once the bindings exist. This
//! implementation is intended to be _correct_ but not especially long lasting.

use crate::{SdkError, SdkSuccess};
use smithy_http::operation;
use smithy_http::operation::Operation;
use smithy_http::retry::ClassifyResponse;
use smithy_types::retry::{ErrorKind, ProvideErrorKind, RetryKind};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

#[derive(Clone)]
pub struct RetryConfig {
    initial_retry_tokens: usize,
    retry_cost: usize,
    no_retry_increment: usize,
    timeout_retry_cost: usize,
    max_retries: u32,
    max_backoff: Duration,
    base: fn() -> f64,
}

impl RetryConfig {
    /// For deterministic tests, enable using a static base instead of random base for exponential backoff
    pub fn with_static_base(mut self, base: fn() -> f64) -> Self {
        self.base = base;
        self
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            initial_retry_tokens: INITIAL_RETRY_TOKENS,
            retry_cost: RETRY_COST,
            no_retry_increment: 1,
            timeout_retry_cost: 10,
            max_retries: MAX_RETRIES,
            max_backoff: Duration::from_secs(20),
            // by default, use a random base for exponential backoff
            base: fastrand::f64,
        }
    }
}

const MAX_RETRIES: u32 = 3;
const INITIAL_RETRY_TOKENS: usize = 500;
const RETRY_COST: usize = 5;

/// StandardRetryStrategy
///
/// `ctx` captures cross-request retry state, whereas `attempts` captures retry state local to this
/// request
#[derive(Clone)]
pub(crate) struct StandardRetryStrategy {
    attempts: u32,
    ctx: Arc<Mutex<RetryCtx>>,
}

impl StandardRetryStrategy {
    pub fn new(ctx: Arc<Mutex<RetryCtx>>) -> Self {
        Self { attempts: 0, ctx }
    }

    #[allow(dead_code)]
    pub fn ctx(&self) -> MutexGuard<'_, RetryCtx> {
        self.ctx.lock().unwrap()
    }

    pub fn do_retry(&self, retry_kind: Result<(), ErrorKind>) -> Option<(Self, Duration)> {
        let mut ctx = self.ctx.lock().unwrap();
        let can_retry = match retry_kind {
            Ok(_) => {
                ctx.retry_quota_release();
                return None;
            }
            Err(e) => {
                if self.attempts == ctx.config.max_retries - 1 {
                    return None;
                }
                ctx.get_retry_quota(e)
            }
        };
        if !can_retry {
            return None;
        };
        let b = (ctx.config.base)();
        let r: i32 = 2;
        let backoff = b * (r.pow(self.attempts) as f64);
        let backoff = Duration::from_secs_f64(backoff).min(ctx.config.max_backoff);
        let mut next = self.clone();
        next.attempts += 1;
        Some((next, backoff))
    }
}

pub(crate) struct RetryCtx {
    retry_quota: usize,
    last_retry: Option<usize>,
    config: RetryConfig,
}

impl RetryCtx {
    pub fn new(config: RetryConfig) -> Self {
        RetryCtx {
            retry_quota: config.initial_retry_tokens,
            last_retry: None,
            config,
        }
    }

    fn retry_quota_release(&mut self) {
        self.retry_quota += self.last_retry.unwrap_or(self.config.no_retry_increment);
    }

    fn get_retry_quota(&mut self, err: ErrorKind) -> bool {
        let retry_cost = if err == ErrorKind::TransientError {
            self.config.timeout_retry_cost
        } else {
            self.config.retry_cost
        };
        if retry_cost > self.retry_quota {
            false
        } else {
            self.last_retry = Some(retry_cost);
            self.retry_quota -= retry_cost;
            true
        }
    }

    #[cfg(test)]
    fn with_base_provider(mut self, base: fn() -> f64) -> Self {
        self.config.base = base;
        self
    }
}

struct UnknownError;

impl ProvideErrorKind for UnknownError {
    fn retryable_error_kind(&self) -> Option<ErrorKind> {
        None
    }

    fn code(&self) -> Option<&str> {
        None
    }
}

impl<Handler, R, T, E>
    tower::retry::Policy<operation::Operation<Handler, R>, SdkSuccess<T>, SdkError<E>>
    for StandardRetryStrategy
where
    E: ProvideErrorKind,
    Handler: Clone,
    R: ClassifyResponse<SdkSuccess<T>, SdkError<E>>,
{
    type Future = Pin<Box<dyn Future<Output = Self>>>;

    fn retry(
        &self,
        req: &Operation<Handler, R>,
        result: Result<&SdkSuccess<T>, &SdkError<E>>,
    ) -> Option<Self::Future> {
        let policy = req.retry_policy();
        let retry = policy.classify(result);
        let (next, fut) = match retry {
            RetryKind::Explicit(dur) => (self.clone(), dur),
            RetryKind::NotRetryable => return None,
            RetryKind::Error(err) => self.do_retry(Err(err))?,
            _ => return None,
        };
        let fut = async move {
            tokio::time::sleep(fut).await;
            next
        };
        Some(Box::pin(fut))
    }

    fn clone_request(&self, req: &Operation<Handler, R>) -> Option<Operation<Handler, R>> {
        req.try_clone()
    }
}

#[cfg(test)]
mod test {
    use crate::retry::{RetryConfig, RetryCtx, StandardRetryStrategy};
    use smithy_types::retry::ErrorKind;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn eventual_success() {
        let ctx = RetryCtx::new(RetryConfig::default()).with_base_provider(|| 1_f64);
        let ctx = Arc::new(Mutex::new(ctx));
        let strategy = StandardRetryStrategy::new(ctx.clone());
        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(1));
        assert_eq!(strategy.ctx().retry_quota, 495);

        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(2));
        assert_eq!(strategy.ctx().retry_quota, 490);

        let no_retry = strategy.do_retry(Ok(()));
        assert!(no_retry.is_none());
        assert_eq!(strategy.ctx().retry_quota, 495);
    }

    #[test]
    fn no_more_attempts() {
        let ctx = RetryCtx::new(RetryConfig::default()).with_base_provider(|| 1_f64);
        let ctx = Arc::new(Mutex::new(ctx));
        let strategy = StandardRetryStrategy::new(ctx.clone());
        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(1));
        assert_eq!(strategy.ctx().retry_quota, 495);

        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(2));
        assert_eq!(strategy.ctx().retry_quota, 490);

        let no_retry = strategy.do_retry(Err(ErrorKind::ServerError));
        assert!(no_retry.is_none());
        assert_eq!(strategy.ctx().retry_quota, 490);
    }

    #[test]
    fn no_quota() {
        let mut conf = RetryConfig::default();
        conf.initial_retry_tokens = 5;
        let ctx = RetryCtx::new(conf).with_base_provider(|| 1_f64);
        let strategy = StandardRetryStrategy::new(Arc::new(Mutex::new(ctx)));
        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(1));
        assert_eq!(strategy.ctx().retry_quota, 0);
        let no_retry = strategy.do_retry(Err(ErrorKind::ServerError));
        assert!(no_retry.is_none());
        assert_eq!(strategy.ctx().retry_quota, 0);
    }

    #[test]
    fn backoff_timing() {
        let mut conf = RetryConfig::default();
        conf.max_retries = 5;
        let ctx = RetryCtx::new(conf).with_base_provider(|| 1_f64);
        let strategy = StandardRetryStrategy::new(Arc::new(Mutex::new(ctx)));
        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(1));
        assert_eq!(strategy.ctx().retry_quota, 495);

        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(2));
        assert_eq!(strategy.ctx().retry_quota, 490);

        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(4));
        assert_eq!(strategy.ctx().retry_quota, 485);

        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(8));
        assert_eq!(strategy.ctx().retry_quota, 480);

        let no_retry = strategy.do_retry(Err(ErrorKind::ServerError));
        assert!(no_retry.is_none());
        assert_eq!(strategy.ctx().retry_quota, 480);
    }

    #[test]
    fn max_backoff_time() {
        let mut conf = RetryConfig::default();
        conf.max_retries = 5;
        conf.max_backoff = Duration::from_secs(3);
        let ctx = RetryCtx::new(conf).with_base_provider(|| 1_f64);
        let strategy = StandardRetryStrategy::new(Arc::new(Mutex::new(ctx)));
        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(1));
        assert_eq!(strategy.ctx().retry_quota, 495);

        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(2));
        assert_eq!(strategy.ctx().retry_quota, 490);

        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(3));
        assert_eq!(strategy.ctx().retry_quota, 485);

        let (strategy, dur) = strategy
            .do_retry(Err(ErrorKind::ServerError))
            .expect("should retry");
        assert_eq!(dur, Duration::from_secs(3));
        assert_eq!(strategy.ctx().retry_quota, 480);

        let no_retry = strategy.do_retry(Err(ErrorKind::ServerError));
        assert!(no_retry.is_none());
        assert_eq!(strategy.ctx().retry_quota, 480);
    }
}
