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
    /// # Examples
    ///
    /// ```rust,ignore
    /// use aws_smithy_mocks::{mock, mock_response};
    /// use aws_sdk_s3::operation::get_object::GetObjectOutput;
    /// use aws_sdk_s3::error::GetObjectError;
    ///
    /// let rule = mock!(aws_sdk_s3::Client::get_object)
    ///     .serve(|idx| {
    ///         if idx < 2 {
    ///             // First two calls for non-important keys return 503
    ///             Some(mock_response!(status: 503))
    ///         } else {
    ///             // Subsequent calls succeed
    ///             Some(mock_response!(GetObjectOutput::builder().build()))
    ///         }
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

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use std::error::Error as StdError;
//     use std::fmt;
//     use aws_smithy_runtime_api::http::StatusCode;
//
//     // Simple test types
//     #[derive(Debug, Clone, PartialEq)]
//     struct TestInput {
//         bucket: String,
//         key: String,
//     }
//
//     impl TestInput {
//         fn new(bucket: &str, key: &str) -> Self {
//             Self {
//                 bucket: bucket.to_string(),
//                 key: key.to_string(),
//             }
//         }
//     }
//
//     #[derive(Debug, Clone, PartialEq)]
//     struct TestOutput {
//         content: String,
//     }
//
//     impl TestOutput {
//         fn new(content: &str) -> Self {
//             Self {
//                 content: content.to_string(),
//             }
//         }
//     }
//
//     #[derive(Debug, Clone, PartialEq)]
//     struct TestError {
//         message: String,
//     }
//
//     impl TestError {
//         fn new(message: &str) -> Self {
//             Self {
//                 message: message.to_string(),
//             }
//         }
//     }
//
//     impl fmt::Display for TestError {
//         fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//             write!(f, "{}", self.message)
//         }
//     }
//
//     impl StdError for TestError {}
//
//     #[test]
//     fn test_rule_next_response() {
//         // Create a rule with a sequence of responses
//         let rule = Rule::new(
//             Arc::new(|_| true),
//             Arc::new(|_, idx| match idx {
//                 0 => Some(MockResponse::Output(TestOutput::new("first"))),
//                 1 => Some(MockResponse::Error(TestError::new("error"))),
//                 2 => Some(MockResponse::status(503)),
//                 _ => None,
//             }),
//         );
//
//         // Test that responses are returned in order
//         let input = TestInput::new("test-bucket", "test-key");
//
//         // First call should return the first output
//         let response = rule.next_response(&input);
//         assert!(response.is_some());
//         match response.unwrap() {
//             MockResponse::Output(output) => assert_eq!(output, TestOutput::new("first")),
//             _ => panic!("Expected Output variant"),
//         }
//
//         // Second call should return the error
//         let response = rule.next_response(&input);
//         assert!(response.is_some());
//         match response.unwrap() {
//             MockResponse::Error(error) => assert_eq!(error, TestError::new("error")),
//             _ => panic!("Expected Error variant"),
//         }
//
//         // Third call should return the HTTP response
//         let response = rule.next_response(&input);
//         assert!(response.is_some());
//         match response.unwrap() {
//             MockResponse::Http(http_response) => {
//                 assert_eq!(http_response.status().as_u16(), 503);
//             },
//             _ => panic!("Expected Http variant"),
//         }
//
//         // Fourth call should return None (rule is exhausted)
//         let response = rule.next_response(&input);
//         assert!(response.is_none());
//     }
//
//     #[test]
//     fn test_rule_num_calls() {
//         // Create a rule with a sequence of responses
//         let rule = Rule::new(
//             Arc::new(|_| true),
//             Arc::new(|_, idx| match idx {
//                 0 => Some(MockResponse::Output(TestOutput::new("first"))),
//                 1 => Some(MockResponse::Error(TestError::new("error"))),
//                 _ => None,
//             }),
//         );
//
//         // Test that num_calls returns the correct value
//         assert_eq!(rule.num_calls(), 0);
//
//         let input = Input::erase(TestInput::new("test-bucket", "test-key"));
//
//         // First call
//         rule.next_response(&input);
//         assert_eq!(rule.num_calls(), 1);
//
//         // Second call
//         rule.next_response(&input);
//         assert_eq!(rule.num_calls(), 2);
//
//         // Third call (rule is exhausted)
//         rule.next_response(&input);
//         assert_eq!(rule.num_calls(), 3);
//     }
//
//     // #[test]
//     // fn test_rule_reset() {
//     //     // Create a rule with a sequence of responses
//     //     let rule = Rule::new(
//     //         Arc::new(|_| true),
//     //         Arc::new(|_, idx| match idx {
//     //             0 => Some(MockResponse::Output(TestOutput::new("first"))),
//     //             _ => None,
//     //         }),
//     //         true,
//     //     );
//     //
//     //     let input = TestInput::new("test-bucket", "test-key");
//     //
//     //     // Call next_response to increment the counter
//     //     rule.next_response(&input);
//     //     assert_eq!(rule.num_calls(), 1);
//     //
//     //     // Reset the rule
//     //     rule.reset();
//     //     assert_eq!(rule.num_calls(), 0);
//     //
//     //     // After reset, we should get the first response again
//     //     let response = rule.next_response(&input);
//     //     assert!(response.is_some());
//     //     match response.unwrap() {
//     //         MockResponse::Output(output) => assert_eq!(output, TestOutput::new("first")),
//     //         _ => panic!("Expected Output variant"),
//     //     }
//     // }
//
//
//     #[test]
//     fn test_rule_builder_then_output() {
//         // Create a rule with a single output
//         let rule = RuleBuilder::<TestInput, TestOutput, TestError>::new()
//             .then_output(|| TestOutput::new("test"));
//
//         // Test that the rule returns the output
//         let input = TestInput::new("test-bucket", "test-key");
//         let response = rule.next_response(&input);
//         assert!(response.is_some());
//         match response.unwrap() {
//             MockResponse::Output(output) => assert_eq!(output, TestOutput::new("test")),
//             _ => panic!("Expected Output variant"),
//         }
//
//         // Test that the rule is exhausted after one call
//         let response = rule.next_response(&input);
//         assert!(response.is_none());
//     }
//
//     #[test]
//     fn test_rule_builder_then_error() {
//         // Create a rule with a single error
//         let rule = RuleBuilder::<TestInput, TestOutput, TestError>::new()
//             .then_error(|| TestError::new("test error"));
//
//         // Test that the rule returns the error
//         let input = TestInput::new("test-bucket", "test-key");
//         let response = rule.next_response(&input);
//         assert!(response.is_some());
//         match response.unwrap() {
//             MockResponse::Error(error) => assert_eq!(error, TestError::new("test error")),
//             _ => panic!("Expected Error variant"),
//         }
//
//         // Test that the rule is exhausted after one call
//         let response = rule.next_response(&input);
//         assert!(response.is_none());
//     }
//
//     #[test]
//     fn test_rule_builder_then_http_response() {
//         // Create a rule with a single HTTP response
//         let rule = RuleBuilder::<TestInput, TestOutput, TestError>::new()
//             .then_http_response(|| {
//                 HttpResponse::new(
//                     StatusCode::try_from(200).unwrap(),
//                     aws_smithy_types::body::SdkBody::from("test body"),
//                 )
//             });
//
//         // Test that the rule returns the HTTP response
//         let input = TestInput::new("test-bucket", "test-key");
//         let response = rule.next_response(&input);
//         assert!(response.is_some());
//         match response.unwrap() {
//             MockResponse::Http(http_response) => {
//                 assert_eq!(http_response.status().as_u16(), 200);
//                 assert_eq!(http_response.body().bytes().unwrap(), b"test body");
//             },
//             _ => panic!("Expected Http variant"),
//         }
//
//         // Test that the rule is exhausted after one call
//         let response = rule.next_response(&input);
//         assert!(response.is_none());
//     }
//
//     #[test]
//     fn test_rule_builder_serve() {
//         // Create a rule with a sequence of responses
//         let rule = RuleBuilder::<TestInput, TestOutput, TestError>::new()
//             .serve(|_, idx| match idx {
//                 0 => Some(mock_response!(TestOutput::new("first"))),
//                 1 => Some(mock_response!(error: TestError::new("error"))),
//                 2 => Some(mock_response!(status: 503)),
//                 _ => None,
//             });
//
//         // Test that responses are returned in order
//         let input = TestInput::new("test-bucket", "test-key");
//
//         // First call should return the first output
//         let response = rule.next_response(&input);
//         assert!(response.is_some());
//         match response.unwrap() {
//             MockResponse::Output(output) => assert_eq!(output, TestOutput::new("first")),
//             _ => panic!("Expected Output variant"),
//         }
//
//         // Second call should return the error
//         let response = rule.next_response(&input);
//         assert!(response.is_some());
//         match response.unwrap() {
//             MockResponse::Error(error) => assert_eq!(error, TestError::new("error")),
//             _ => panic!("Expected Error variant"),
//         }
//
//         // Third call should return the HTTP response
//         let response = rule.next_response(&input);
//         assert!(response.is_some());
//         match response.unwrap() {
//             MockResponse::Http(http_response) => {
//                 assert_eq!(http_response.status().as_u16(), 503);
//             },
//             _ => panic!("Expected Http variant"),
//         }
//
//         // Fourth call should return None (rule is exhausted)
//         let response = rule.next_response(&input);
//         assert!(response.is_none());
//     }
//
//     #[test]
//     fn test_rule_builder_serve_with_input() {
//         // Create a rule that uses the input to determine the response
//         let rule = RuleBuilder::<TestInput, TestOutput, TestError>::new()
//             .serve(|input, _| {
//                 if input.bucket == "special-bucket" {
//                     Some(mock_response!(TestOutput::new("special")))
//                 } else {
//                     Some(mock_response!(TestOutput::new("normal")))
//                 }
//             });
//
//         // Test that the rule returns different responses based on the input
//         let input1 = TestInput::new("special-bucket", "test-key");
//         let response1 = rule.next_response(&input1);
//         assert!(response1.is_some());
//         match response1.unwrap() {
//             MockResponse::Output(output) => assert_eq!(output, TestOutput::new("special")),
//             _ => panic!("Expected Output variant"),
//         }
//
//         let input2 = TestInput::new("normal-bucket", "test-key");
//         let response2 = rule.next_response(&input2);
//         assert!(response2.is_some());
//         match response2.unwrap() {
//             MockResponse::Output(output) => assert_eq!(output, TestOutput::new("normal")),
//             _ => panic!("Expected Output variant"),
//         }
//     }
//
//     #[test]
//     fn test_rule_builder_match_requests() {
//         // Create a rule that only matches specific inputs
//         let rule = RuleBuilder::<TestInput, TestOutput, TestError>::new()
//             .match_requests(|input| input.bucket == "matched-bucket")
//             .serve(|_, _| Some(mock_response!(TestOutput::new("matched"))));
//
//         // Test that the rule matcher works
//         let input1 = TestInput::new("matched-bucket", "test-key");
//         let input2 = TestInput::new("unmatched-bucket", "test-key");
//
//         let input_matches = (rule.matcher)(&Input::erase(input1.clone()));
//         let input_does_not_match = (rule.matcher)(&Input::erase(input2.clone()));
//
//         assert!(input_matches);
//         assert!(!input_does_not_match);
//     }
//
// }
//
