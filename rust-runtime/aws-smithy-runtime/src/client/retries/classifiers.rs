/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
use aws_smithy_runtime_api::client::retries::classifiers::{
    ClassifyRetry, RetryClassifierPriority, RetryClassifierResult, SharedRetryClassifier,
};
use aws_smithy_types::retry::{ErrorKind, ProvideErrorKind};
use std::borrow::Cow;
use std::error::Error as StdError;
use std::marker::PhantomData;

/// A retry classifier for checking if an error is modeled as retryable.
#[derive(Debug, Default)]
pub struct ModeledAsRetryableClassifier<E> {
    _inner: PhantomData<E>,
}

impl<E> ModeledAsRetryableClassifier<E> {
    /// Create a new `ModeledAsRetryableClassifier`
    pub fn new() -> Self {
        Self {
            _inner: PhantomData,
        }
    }

    /// Return the priority of this retry classifier.
    pub fn priority() -> RetryClassifierPriority {
        RetryClassifierPriority::ModeledAsRetryableClassifier
    }
}

impl<E> ClassifyRetry for ModeledAsRetryableClassifier<E>
where
    E: StdError + ProvideErrorKind + Send + Sync + 'static,
{
    fn classify_retry(
        &self,
        ctx: &InterceptorContext,
        previous_result: Option<RetryClassifierResult>,
    ) -> Option<RetryClassifierResult> {
        if previous_result.is_some() {
            // Never second-guess a result from a higher-priority classifier
            return previous_result;
        }

        // Check for a result
        let output_or_error = ctx.output_or_error()?;
        // Check for an error
        let error = match output_or_error {
            Ok(_) => return None,
            Err(err) => err,
        };
        // Check that the error is an operation error
        let error = error.as_operation_error()?;
        // Downcast the error
        let error = error.downcast_ref::<E>()?;
        // Check if the error is retryable
        error
            .retryable_error_kind()
            .map(RetryClassifierResult::Error)
    }

    fn name(&self) -> &'static str {
        "Errors Modeled As Retryable"
    }

    fn priority(&self) -> RetryClassifierPriority {
        Self::priority()
    }
}

/// Classifies response, timeout, and connector errors as retryable or not.
#[derive(Debug, Default)]
pub struct SmithyErrorClassifier<E> {
    _inner: PhantomData<E>,
}

impl<E> SmithyErrorClassifier<E> {
    /// Create a new `SmithyErrorClassifier`
    pub fn new() -> Self {
        Self {
            _inner: PhantomData,
        }
    }

    /// Return the priority of this retry classifier.
    pub fn priority() -> RetryClassifierPriority {
        RetryClassifierPriority::SmithyErrorClassifier
    }
}

impl<E> ClassifyRetry for SmithyErrorClassifier<E>
where
    E: StdError + Send + Sync + 'static,
{
    fn classify_retry(
        &self,
        ctx: &InterceptorContext,
        previous_result: Option<RetryClassifierResult>,
    ) -> Option<RetryClassifierResult> {
        if previous_result.is_some() {
            // Never second-guess a result from a higher-priority classifier
            return previous_result;
        }

        let output_or_error = ctx.output_or_error()?;
        // Check for an error
        let error = match output_or_error {
            Ok(_) => return None,
            Err(err) => err,
        };

        if error.is_response_error() || error.is_timeout_error() {
            Some(RetryClassifierResult::Error(ErrorKind::TransientError))
        } else if let Some(error) = error.as_connector_error() {
            if error.is_timeout() || error.is_io() {
                Some(RetryClassifierResult::Error(ErrorKind::TransientError))
            } else {
                error.as_other().map(RetryClassifierResult::Error)
            }
        } else {
            None
        }
    }

    fn name(&self) -> &'static str {
        "Retryable Smithy Errors"
    }

    fn priority(&self) -> RetryClassifierPriority {
        Self::priority()
    }
}

const TRANSIENT_ERROR_STATUS_CODES: &[u16] = &[500, 502, 503, 504];

/// A retry classifier that will treat HTTP response with those status codes as retryable.
/// The `Default` version will retry 500, 502, 503, and 504 errors.
#[derive(Debug)]
pub struct HttpStatusCodeClassifier {
    retryable_status_codes: Cow<'static, [u16]>,
}

impl Default for HttpStatusCodeClassifier {
    fn default() -> Self {
        Self::new_from_codes(TRANSIENT_ERROR_STATUS_CODES.to_owned())
    }
}

impl HttpStatusCodeClassifier {
    /// Given a `Vec<u16>` where the `u16`s represent status codes, create a `HttpStatusCodeClassifier`
    /// that will treat HTTP response with those status codes as retryable. The `Default` version
    /// will retry 500, 502, 503, and 504 errors.
    pub fn new_from_codes(retryable_status_codes: impl Into<Cow<'static, [u16]>>) -> Self {
        Self {
            retryable_status_codes: retryable_status_codes.into(),
        }
    }

    /// Return the priority of this retry classifier.
    pub fn priority() -> RetryClassifierPriority {
        RetryClassifierPriority::HttpStatusCodeClassifier
    }
}

impl ClassifyRetry for HttpStatusCodeClassifier {
    fn classify_retry(
        &self,
        ctx: &InterceptorContext,
        previous_result: Option<RetryClassifierResult>,
    ) -> Option<RetryClassifierResult> {
        if previous_result.is_some() {
            // Never second-guess a result from a higher-priority classifier
            return previous_result;
        }

        ctx.response()
            .map(|res| res.status().as_u16())
            .map(|status| self.retryable_status_codes.contains(&status))
            .unwrap_or_default()
            .then_some(RetryClassifierResult::Error(ErrorKind::TransientError))
    }

    fn name(&self) -> &'static str {
        "HTTP Status Code"
    }

    fn priority(&self) -> RetryClassifierPriority {
        Self::priority()
    }
}

/// Given an iterator of retry classifiers and an interceptor context, run retry classifiers on the
/// context. Each classifier is passed the classification result from the previous classifier (the
/// 'root' classifier is passed `None`.)
pub fn run_classifiers_on_ctx(
    classifiers: impl Iterator<Item = SharedRetryClassifier>,
    ctx: &InterceptorContext,
) -> RetryClassifierResult {
    let mut result = None;

    for classifier in classifiers {
        result = classifier.classify_retry(ctx, result);
        match result.as_ref() {
            Some(result) => tracing::debug!(
                "running retry classifier '{}' returned result '{result}'",
                classifier.name()
            ),
            None => tracing::trace!(
                "running retry classifier '{}' returned no result",
                classifier.name()
            ),
        }
    }

    result.unwrap_or(RetryClassifierResult::DontRetry)
}

#[cfg(test)]
mod test {
    use crate::client::retries::classifiers::{
        HttpStatusCodeClassifier, ModeledAsRetryableClassifier,
    };
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_runtime_api::client::interceptors::context::{Error, Input, InterceptorContext};
    use aws_smithy_runtime_api::client::orchestrator::OrchestratorError;
    use aws_smithy_runtime_api::client::retries::classifiers::{
        ClassifyRetry, RetryClassifierResult,
    };
    use aws_smithy_types::retry::{ErrorKind, ProvideErrorKind};
    use std::fmt;

    use super::SmithyErrorClassifier;

    #[derive(Debug, PartialEq, Eq, Clone)]
    struct UnmodeledError;

    impl fmt::Display for UnmodeledError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "UnmodeledError")
        }
    }

    impl std::error::Error for UnmodeledError {}

    #[test]
    fn classify_by_response_status() {
        let policy = HttpStatusCodeClassifier::default();
        let res = http::Response::builder()
            .status(500)
            .body("error!")
            .unwrap()
            .map(SdkBody::from);
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_response(res);
        assert_eq!(
            policy.classify_retry(&ctx, None),
            Some(RetryClassifierResult::Error(ErrorKind::TransientError))
        );
    }

    #[test]
    fn classify_by_response_status_not_retryable() {
        let policy = HttpStatusCodeClassifier::default();
        let res = http::Response::builder()
            .status(408)
            .body("error!")
            .unwrap()
            .map(SdkBody::from);
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_response(res);
        assert_eq!(policy.classify_retry(&ctx, None), None);
    }

    #[test]
    fn classify_by_error_kind() {
        #[derive(Debug)]
        struct RetryableError;

        impl fmt::Display for RetryableError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "Some retryable error")
            }
        }

        impl ProvideErrorKind for RetryableError {
            fn retryable_error_kind(&self) -> Option<ErrorKind> {
                Some(ErrorKind::ClientError)
            }

            fn code(&self) -> Option<&str> {
                // code should not be called when `error_kind` is provided
                unimplemented!()
            }
        }

        impl std::error::Error for RetryableError {}

        let policy = ModeledAsRetryableClassifier::<RetryableError>::new();
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::operation(Error::erase(
            RetryableError,
        ))));

        assert_eq!(
            policy.classify_retry(&ctx, None),
            Some(RetryClassifierResult::Error(ErrorKind::ClientError)),
        );
    }

    #[test]
    fn classify_response_error() {
        let policy = SmithyErrorClassifier::<UnmodeledError>::new();
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::response(
            "I am a response error".into(),
        )));
        assert_eq!(
            policy.classify_retry(&ctx, None),
            Some(RetryClassifierResult::Error(ErrorKind::TransientError)),
        );
    }

    #[test]
    fn test_timeout_error() {
        let policy = SmithyErrorClassifier::<UnmodeledError>::new();
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::timeout(
            "I am a timeout error".into(),
        )));
        assert_eq!(
            policy.classify_retry(&ctx, None),
            Some(RetryClassifierResult::Error(ErrorKind::TransientError)),
        );
    }
}
