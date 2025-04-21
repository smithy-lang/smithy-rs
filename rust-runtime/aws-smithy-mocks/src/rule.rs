/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::MockResponse;
use aws_smithy_runtime_api::client::interceptors::context::{Error, Input, Output};
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::result::SdkError;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A function that matches requests.
type MatchFn = Arc<dyn Fn(&Input) -> bool + Send + Sync>;
type ServeFn = Arc<dyn Fn(usize) -> Option<MockResponse<Output, Error>> + Send + Sync>;

/// A rule for matching requests and providing mock responses.
///
/// Rules are created using the `mock!` macro or the `RuleBuilder`.
///
/// # Examples
///
/// ```rust,ignore
/// use aws_smithy_mocks::{mock, MockResponse};
/// use aws_sdk_s3::operation::get_object::GetObjectOutput;
/// use aws_sdk_s3::error::GetObjectError;
///
/// // Create a rule with a single response
/// let rule = mock!(aws_sdk_s3::Client::get_object)
///     .then_output(|| GetObjectOutput::builder().build());
///
/// // Create a rule with a sequence of responses using serve
/// let rule = mock!(aws_sdk_s3::Client::get_object)
///     .serve(|idx| match idx {
///         0 => Some(GetObjectOutput::builder().build()),
///         1 => Some(503),
///         _ => None,  // Rule is exhausted after 2 calls
///     });
/// ```
#[derive(Clone)]
pub struct Rule {
    /// Function that determines if this rule matches a request.
    pub(crate) matcher: MatchFn,

    /// Handler function that generates responses.
    pub(crate) response_handler: ServeFn,

    /// Number of times this rule has been called.
    pub(crate) call_count: Arc<AtomicUsize>,
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rule")
    }
}

impl Rule {
    /// Creates a new rule with the given matcher and response handler.
    pub(crate) fn new<O, E>(
        matcher: MatchFn,
        response_handler: Arc<dyn Fn(usize) -> Option<MockResponse<O, E>> + Send + Sync>,
    ) -> Self
    where
        O: fmt::Debug + Send + Sync + 'static,
        E: fmt::Debug + Send + Sync + std::error::Error + 'static,
    {
        Rule {
            matcher,
            response_handler: Arc::new(move |idx: usize| {
                response_handler(idx).map(|resp| match resp {
                    MockResponse::Output(o) => MockResponse::Output(Output::erase(o)),
                    MockResponse::Error(e) => MockResponse::Error(Error::erase(e)),
                    MockResponse::Http(http_resp) => MockResponse::http(http_resp),
                })
            }),
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Gets the next response for the given input.
    ///
    /// This increments the call count and returns the response from the handler.
    pub(crate) fn next_response(&self) -> Option<MockResponse<Output, Error>> {
        let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
        match (self.response_handler)(idx) {
            None => {
                self.call_count.fetch_sub(1, Ordering::SeqCst);
                None
            }
            Some(resp) => Some(resp),
        }
    }

    /// Returns the number of times this rule has been called.
    pub fn num_calls(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    // TODO - evaluate if we need/want this
    /// Resets the call count to zero.
    pub fn reset(&self) {
        self.call_count.store(0, Ordering::SeqCst);
    }
}

/// RuleMode describes how rules will be interpreted.
/// - In RuleMode::MatchAny, the first matching rule will be applied, and the rules will remain unchanged.
/// - In RuleMode::Sequential, the first matching rule will be applied, and that rule will be removed from the list of rules **once it is exhausted**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleMode {
    /// Match rules in the order they were added. The first matching rule will be applied and the
    /// rules will remain unchanged
    Sequential,
    /// The first matching rule will be applied, and that rule will be removed from the list of rules
    /// **once it is exhausted**. Each rule can have multiple responses, and all responses in a rule
    /// will be consumed before moving to the next rule.
    MatchAny,
}

/// A builder for creating rules.
///
/// This builder provides a fluent API for creating rules with different response types.
///
/// # Examples
///
/// ```rust,ignore
/// use aws_smithy_mocks::{mock, mock_response};
/// use aws_sdk_s3::operation::get_object::GetObjectOutput;
/// use aws_sdk_s3::error::GetObjectError;
///
/// // Create a rule with a single response
/// let rule = mock!(aws_sdk_s3::Client::get_object)
///     .then_output(|| GetObjectOutput::builder().build());
///
/// // Create a rule with a sequence of responses using serve
/// let rule = mock!(aws_sdk_s3::Client::get_object)
///     .serve(|idx| match idx {
///         0 => Some(mock_response!(GetObjectOutput::builder().build())),
///         1 => Some(mock_response!(status: 503)),
///         2 => Some(mock_response!(error: GetObjectError::NoSuchKey(Default::default()))),
///  => None,  // Rule is exhausted after 3 calls
///     });
/// ```
///
pub struct RuleBuilder<I, O, E> {
    /// Function that determines if this rule matches a request.
    pub(crate) input_filter: MatchFn,

    /// Phantom data for the input type.
    pub(crate) _ty: std::marker::PhantomData<(I, O, E)>,
}

impl<I, O, E> RuleBuilder<I, O, E>
where
    I: fmt::Debug + Send + Sync + 'static,
    O: fmt::Debug + Send + Sync + 'static,
    E: fmt::Debug + Send + Sync + std::error::Error + 'static,
{
    /// Creates a new [`RuleBuilder`]
    #[doc(hidden)]
    pub fn new() -> Self {
        RuleBuilder {
            input_filter: Arc::new(|i: &Input| i.downcast_ref::<I>().is_some()),
            _ty: std::marker::PhantomData,
        }
    }

    /// Creates a new [`RuleBuilder`]. This is normally constructed with the [`mock!`] macro
    #[doc(hidden)]
    pub fn new_from_mock<F, R>(_input_hint: impl Fn() -> I, _output_hint: impl Fn() -> F) -> Self
    where
        F: Future<Output = Result<O, SdkError<E, R>>>,
    {
        Self {
            input_filter: Arc::new(|i: &Input| i.downcast_ref::<I>().is_some()),
            _ty: Default::default(),
        }
    }

    /// Sets the function that determines if this rule matches a request.
    pub fn match_requests<F>(mut self, filter: F) -> Self
    where
        F: Fn(&I) -> bool + Send + Sync + 'static,
    {
        self.input_filter = Arc::new(move |i: &Input| match i.downcast_ref::<I>() {
            Some(typed_input) => filter(typed_input),
            _ => false,
        });
        self
    }

    /// Helper function for single-response rules.
    fn serve_once<R>(self, response: R) -> Rule
    where
        R: Into<MockResponse<O, E>> + Send + Sync + 'static,
    {
        let mu = Arc::new(std::sync::Mutex::new(Some(response)));
        self.serve(move |idx| {
            if idx == 0 {
                let response = mu.lock().unwrap().take().expect("response already taken");
                Some(response.into())
            } else {
                None
            }
        })
    }

    /// Creates a rule that returns a modeled output.
    pub fn then_output<F>(self, output_fn: F) -> Rule
    where
        F: Fn() -> O + Send + Sync + 'static,
    {
        self.serve_once(MockResponse::Output(output_fn()))
    }

    /// Creates a rule that returns a modeled error.
    pub fn then_error<F>(self, error_fn: F) -> Rule
    where
        F: Fn() -> E + Send + Sync + 'static,
    {
        self.serve_once(MockResponse::Error(error_fn()))
    }

    /// Creates a rule that returns an HTTP response.
    pub fn then_http_response<F>(self, response_fn: F) -> Rule
    where
        F: Fn() -> HttpResponse + Send + Sync + 'static,
    {
        self.serve_once(MockResponse::Http(response_fn()))
    }

    /// Creates a rule that returns responses based on the call index.
    ///
    /// This method allows for complex response patterns based on the call sequence.
    /// The handler function takes the call index and returns an optional response.
    /// If the handler returns None, the rule is considered exhausted.
    ///
    /// This is particularly useful for testing retry behavior, where you might want to return
    /// error responses for the first few attempts and then succeed.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use aws_smithy_mocks::{mock, mock_response};
    /// use aws_sdk_s3::operation::get_object::GetObjectOutput;
    /// use aws_sdk_s3::error::GetObjectError;
    ///
    /// // Create a rule that returns 503 errors for the first two calls, then succeeds
    /// let rule = mock!(aws_sdk_s3::Client::get_object)
    ///     .serve(|idx| {
    ///         if idx < 2 {
    ///             // First two calls return 503
    ///             Some(mock_response!(status: 503))
    ///         } else {
    ///             // Subsequent calls succeed
    ///             Some(mock_response!(GetObjectOutput::builder().build()))
    ///         }
    ///     });
    /// ```
    ///
    /// You can also use pattern matching for more complex scenarios:
    ///
    /// ```rust,ignore
    /// let rule = mock!(Client::get_object)
    ///     .serve(|idx| match idx {
    ///         0 => Some(mock_response!(TestOutput::new("first output"))),
    ///         1 => Some(mock_response!(error: TestError::new("expected error"))),
    ///         2 => Some(mock_response!(http: HttpResponse::new(
    ///             StatusCode::try_from(200).unwrap(),
    ///             SdkBody::from("http response")
    ///         ))),
    ///         _ => None  // No more responses after the third call
    ///     });
    /// ```
    ///
    pub fn serve<F>(self, handler: F) -> Rule
    where
        F: Fn(usize) -> Option<MockResponse<O, E>> + Send + Sync + 'static,
    {
        Rule::new::<O, E>(self.input_filter, Arc::new(handler))
    }
}
