/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Classifier for determining if a retry is necessary and related code.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use aws_smithy_types::retry::ErrorKind;

use crate::client::interceptors::context::InterceptorContext;
use crate::impl_shared_conversions;

/// The result of running a [`ClassifyRetry`] on a [`InterceptorContext`].
#[non_exhaustive]
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum RetryAction {
    /// When a classifier doesn't have something to classify, this action is returned.
    ///
    /// For example, if a classifier requires a parsed response and response parsing failed,
    /// this action is returned. If all classifiers return this action, no retry should be
    /// attempted.
    DontCare,
    /// When an error is received that should be retried, this action is returned.
    Retry(ErrorKind),
    /// When the server tells us to retry after a specific time has elapsed, this action is returned.
    RetryAfter(Duration),
    /// When a response must not be retried, this action is returned. This action stops retry
    /// classification immediately, skipping any following classifiers.
    RetryForbidden,
}

impl fmt::Display for RetryAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DontCare => write!(f, "don't care"),
            Self::Retry(kind) => write!(f, "retry ({kind})"),
            Self::RetryAfter(duration) => write!(f, "retry after {duration:?}"),
            Self::RetryForbidden => write!(f, "retry forbidden"),
        }
    }
}

/// The priority of a retry classifier. Classifiers with a higher priority will run before
/// classifiers with a lower priority. Classifiers with equal priorities make no guarantees
/// about which will run first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryClassifierPriority {
    inner: Inner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inner {
    // The default priority for the `HttpStatusCodeClassifier`.
    HttpStatusCodeClassifier,
    // The default priority for the `ModeledAsRetryableClassifier`.
    ModeledAsRetryableClassifier,
    // The default priority for the `TransientErrorClassifier`.
    TransientErrorClassifier,
    // The priority of some other classifier.
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
    /// Create a new `RetryClassifierPriority` with the default priority for the `HttpStatusCodeClassifier`.
    pub fn http_status_code_classifier() -> Self {
        Self {
            inner: Inner::HttpStatusCodeClassifier,
        }
    }

    /// Create a new `RetryClassifierPriority` with the default priority for the `ModeledAsRetryableClassifier`.
    pub fn modeled_as_retryable_classifier() -> Self {
        Self {
            inner: Inner::ModeledAsRetryableClassifier,
        }
    }

    /// Create a new `RetryClassifierPriority` with the default priority for the `TransientErrorClassifier`.
    pub fn transient_error_classifier() -> Self {
        Self {
            inner: Inner::TransientErrorClassifier,
        }
    }

    /// Create a new `RetryClassifierPriority` with lower priority than the given priority.
    pub fn with_lower_priority_than(other: Self) -> Self {
        Self {
            inner: Inner::Other(other.as_i8() + 1),
        }
    }

    /// Create a new `RetryClassifierPriority` with higher priority than the given priority.
    pub fn with_higher_priority_than(other: Self) -> Self {
        Self {
            inner: Inner::Other(other.as_i8() - 1),
        }
    }

    fn as_i8(&self) -> i8 {
        match self.inner {
            Inner::HttpStatusCodeClassifier => 0,
            Inner::ModeledAsRetryableClassifier => 10,
            Inner::TransientErrorClassifier => 20,
            Inner::Other(i) => i,
        }
    }
}

impl Default for RetryClassifierPriority {
    fn default() -> Self {
        Self {
            inner: Inner::Other(0),
        }
    }
}

/// Classifies what kind of retry is needed for a given [`InterceptorContext`].
pub trait ClassifyRetry: Send + Sync + fmt::Debug {
    /// Run this classifier on the [`InterceptorContext`] to determine if the previous request
    /// should be retried. Returns a [`RetryAction`].
    fn classify_retry(&self, ctx: &InterceptorContext) -> RetryAction;

    /// The name of this retry classifier.
    ///
    /// Used for debugging purposes.
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
    fn classify_retry(&self, ctx: &InterceptorContext) -> RetryAction {
        self.0.classify_retry(ctx)
    }

    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn priority(&self) -> RetryClassifierPriority {
        self.0.priority()
    }
}

#[cfg(test)]
mod tests {
    use super::RetryClassifierPriority;

    #[test]
    fn test_classifier_lower_priority_than() {
        let classifier_a = RetryClassifierPriority::default();
        let classifier_b = RetryClassifierPriority::with_lower_priority_than(classifier_a);
        let classifier_c = RetryClassifierPriority::with_lower_priority_than(classifier_b);

        let mut list = vec![classifier_b, classifier_a, classifier_c];
        list.sort();

        assert_eq!(vec![classifier_c, classifier_b, classifier_a], list);
    }

    #[test]
    fn test_classifier_higher_priority_than() {
        let classifier_c = RetryClassifierPriority::default();
        let classifier_b = RetryClassifierPriority::with_higher_priority_than(classifier_c);
        let classifier_a = RetryClassifierPriority::with_higher_priority_than(classifier_b);

        let mut list = vec![classifier_b, classifier_c, classifier_a];
        list.sort();

        assert_eq!(vec![classifier_c, classifier_b, classifier_a], list);
    }
}
