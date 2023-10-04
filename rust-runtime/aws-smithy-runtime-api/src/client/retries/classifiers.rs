/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Classifier for determining if a retry is necessary and related code.

use crate::client::interceptors::context::InterceptorContext;
use crate::impl_shared_conversions;
use aws_smithy_types::retry::ErrorKind;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// The result of running a [`ClassifyRetry`] on a [`InterceptorContext`].
#[non_exhaustive]
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum RetryClassifierResult {
    /// "A retryable error was received. This is what kind of error it was,
    /// in case that's important."
    Error(ErrorKind),
    /// "The server told us to wait this long before retrying the request."
    Explicit(Duration),
    /// "This response should not be retried."
    DontRetry,
}

impl fmt::Display for RetryClassifierResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error(kind) => write!(f, "retry ({kind})"),
            Self::Explicit(duration) => write!(f, "retry after {duration:?}"),
            Self::DontRetry => write!(f, "don't retry"),
        }
    }
}

/// The priority of a retry classifier. Classifiers with a higher priority will run before
/// classifiers with a lower priority. Classifiers with equal priorities make no guarantees
/// about which will run first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClassifierPriority {
    /// The default priority for the [`HttpStatusCodeClassifier`].
    HttpStatusCodeClassifier,
    /// The default priority for the [`ModeledAsRetryableClassifier`].
    ModeledAsRetryableClassifier,
    /// The default priority for the [`SmithyErrorClassifier`].
    SmithyErrorClassifier,
    /// The priority of some other classifier.
    Other(i8),
}

impl PartialOrd for RetryClassifierPriority {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(other.as_i8().cmp(&self.as_i8()))
    }
}

impl Ord for RetryClassifierPriority {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.as_i8().cmp(&self.as_i8())
    }
}

impl RetryClassifierPriority {
    /// Create a new `RetryClassifierPriority` that runs after the given priority.
    pub fn run_after(other: Self) -> Self {
        Self::Other(other.as_i8() - 1)
    }

    /// Create a new `RetryClassifierPriority` that runs before the given priority.
    pub fn run_before(other: Self) -> Self {
        Self::Other(other.as_i8() + 1)
    }

    fn as_i8(&self) -> i8 {
        match self {
            Self::HttpStatusCodeClassifier => 0,
            Self::ModeledAsRetryableClassifier => 10,
            Self::SmithyErrorClassifier => 20,
            Self::Other(i) => *i,
        }
    }
}

impl Default for RetryClassifierPriority {
    fn default() -> Self {
        Self::Other(0)
    }
}

/// Classifies what kind of retry is needed for a given [`InterceptorContext`].
pub trait ClassifyRetry: Send + Sync + fmt::Debug {
    /// Run this classifier on the [`InterceptorContext`] to determine if the previous request
    /// should be retried. Returns a [`RetryClassifierResult`].
    fn classify_retry(
        &self,
        ctx: &InterceptorContext,
        result_of_preceding_classifier: Option<RetryClassifierResult>,
    ) -> Option<RetryClassifierResult>;

    /// The name of this retry classifier.
    ///
    /// Used for debugging purposes
    fn name(&self) -> &'static str;

    /// The priority of this retry classifier. Classifiers with a higher priority will run before
    /// classifiers with a lower priority. Classifiers with equal priorities make no guarantees
    /// about which will run first.
    fn priority(&self) -> RetryClassifierPriority {
        RetryClassifierPriority::default()
    }
}

impl_shared_conversions!(convert SharedRetryClassifier from ClassifyRetry using SharedRetryClassifier::new);

#[derive(Debug, Clone)]
/// Retry classifier used by the retry strategy to classify responses as retryable or not.
pub struct SharedRetryClassifier(Arc<dyn ClassifyRetry>);

impl SharedRetryClassifier {
    /// Given a [`ClassifyRetry`] trait object, create a new `SharedRetryClassifier`.
    pub fn new(retry_classifier: impl ClassifyRetry + 'static) -> Self {
        Self(Arc::new(retry_classifier))
    }
}

impl ClassifyRetry for SharedRetryClassifier {
    fn classify_retry(
        &self,
        ctx: &InterceptorContext,
        result_of_preceding_classifier: Option<RetryClassifierResult>,
    ) -> Option<RetryClassifierResult> {
        self.0.classify_retry(ctx, result_of_preceding_classifier)
    }

    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn priority(&self) -> RetryClassifierPriority {
        RetryClassifierPriority::default()
    }
}

#[cfg(test)]
mod tests {
    use super::RetryClassifierPriority;

    #[test]
    fn test_classifier_priority_run_after() {
        let classifier_a = RetryClassifierPriority::default();
        let classifier_b = RetryClassifierPriority::run_after(classifier_a);
        let classifier_c = RetryClassifierPriority::run_after(classifier_b);

        let mut list = vec![classifier_b, classifier_a, classifier_c];
        list.sort();

        assert_eq!(vec![classifier_a, classifier_b, classifier_c], list);
    }

    #[test]
    fn test_classifier_priority_run_before() {
        let classifier_c = RetryClassifierPriority::default();
        let classifier_b = RetryClassifierPriority::run_before(classifier_c);
        let classifier_a = RetryClassifierPriority::run_before(classifier_b);

        let mut list = vec![classifier_b, classifier_c, classifier_a];
        list.sort();

        assert_eq!(vec![classifier_a, classifier_b, classifier_c], list);
    }
}
