/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! This crate allows mocking of smithy clients.

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
/* End of automatically managed default lints */
use std::collections::VecDeque;
use std::fmt;
use std::fmt::Formatter;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use aws_smithy_http_client::test_util::infallible_client_fn;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::http::SharedHttpClient;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeSerializationInterceptorContextMut, BeforeTransmitInterceptorContextMut, Error,
    FinalizerInterceptorContextMut, Input, Output,
};
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::orchestrator::{HttpResponse, OrchestratorError};
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::http::{Response, StatusCode};
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};

// why do we need a macro for this?
// We want customers to be able to provide an ergonomic way to say the method they're looking for,
// `Client::list_buckets`, e.g. But there isn't enough information on that type to recover everything.
// This macro commits a small amount of crimes to recover that type information so we can construct
// a rule that can intercept these operations.

/// `mock!` macro that produces a [`RuleBuilder`] from a client invocation
///
/// See the `examples` folder of this crate for fully worked examples.
///
/// # Examples
/// **Mock and return a success response**:
/// ```rust,ignore
/// use aws_sdk_s3::operation::get_object::GetObjectOutput;
/// use aws_sdk_s3::Client;
/// use aws_smithy_types::byte_stream::ByteStream;
/// use aws_smithy_mocks::mock;
/// let get_object_happy_path = mock!(Client::get_object)
///   .match_requests(|req|req.bucket() == Some("test-bucket") && req.key() == Some("test-key"))
///   .then_output(||GetObjectOutput::builder().body(ByteStream::from_static(b"12345-abcde")).build());
/// ```
///
/// **Mock and return an error**:
/// ```rust,ignore
/// use aws_sdk_s3::operation::get_object::GetObjectError;
/// use aws_sdk_s3::types::error::NoSuchKey;
/// use aws_sdk_s3::Client;
/// use aws_smithy_mocks::mock;
/// let get_object_error_path = mock!(Client::get_object)
///   .then_error(||GetObjectError::NoSuchKey(NoSuchKey::builder().build()));
/// ```
#[macro_export]
macro_rules! mock {
    ($operation: expr) => {
        #[allow(unreachable_code)]
        {
            $crate::RuleBuilder::new(
                // We don't actually want to run this code, so we put it in a closure. The closure
                // has the types we want which makes this whole thing type-safe (and the IDE can even
                // figure out the right input/output types in inference!)
                // The code generated here is:
                // `Client::list_buckets(todo!())`
                || $operation(todo!()).as_input().clone().build().unwrap(),
                || $operation(todo!()).send(),
            )
        }
    };
}

// This could be obviated by a reasonable trait, since you can express it with SdkConfig if clients implement From<&SdkConfig>.

/// `mock_client!` macro produces a Client configured with a number of Rules and appropriate test default configuration.
///
/// # Examples
/// **Create a client that uses a mock failure and then a success**:
/// rust,ignore
/// use aws_sdk_s3::operation::get_object::{GetObjectOutput, GetObjectError};
/// use aws_sdk_s3::types::error::NoSuchKey;
/// use aws_sdk_s3::Client;
/// use aws_smithy_types::byte_stream::ByteStream;
/// use aws_smithy_mocks::{mock_client, mock, RuleMode};
/// let get_object_error_path = mock!(Client::get_object)
///   .then_error(||GetObjectError::NoSuchKey(NoSuchKey::builder().build()))
///   .build();
/// let get_object_happy_path = mock!(Client::get_object)
///   .match_requests(|req|req.bucket() == Some("test-bucket") && req.key() == Some("test-key"))
///   .then_output(||GetObjectOutput::builder().body(ByteStream::from_static(b"12345-abcde")).build())
///   .build();
/// let client = mock_client!(aws_sdk_s3, RuleMode::Sequential, &[&get_object_error_path, &get_object_happy_path]);
///
///
/// **Create a client but customize a specific setting**:
/// rust,ignore
/// use aws_sdk_s3::operation::get_object::GetObjectOutput;
/// use aws_sdk_s3::Client;
/// use aws_smithy_types::byte_stream::ByteStream;
/// use aws_smithy_mocks::{mock_client, mock, RuleMode};
/// let get_object_happy_path = mock!(Client::get_object)
///   .match_requests(|req|req.bucket() == Some("test-bucket") && req.key() == Some("test-key"))
///   .then_output(||GetObjectOutput::builder().body(ByteStream::from_static(b"12345-abcde")).build())
///   .build();
/// let client = mock_client!(
///     aws_sdk_s3,
///     RuleMode::Sequential,
///     &[&get_object_happy_path],
///     // Perhaps you need to force path style
///     |client_builder|client_builder.force_path_style(true)
/// );
///
#[macro_export]
macro_rules! mock_client {
    ($aws_crate: ident, $rules: expr) => {
        $crate::mock_client!($aws_crate, $crate::RuleMode::Sequential, $rules)
    };
    ($aws_crate: ident, $rule_mode: expr, $rules: expr) => {{
        $crate::mock_client!($aws_crate, $rule_mode, $rules, |conf| conf)
    }};
    ($aws_crate: ident, $rule_mode: expr, $rules: expr, $additional_configuration: expr) => {{
        let mut mock_response_interceptor =
            $crate::MockResponseInterceptor::new().rule_mode($rule_mode);
        for rule in $rules {
            mock_response_interceptor = mock_response_interceptor.with_rule(rule)
        }

        // Create a mock HTTP client
        let mock_http_client = $crate::create_mock_http_client();

        // Allow callers to avoid explicitly specifying the type
        fn coerce<T: Fn($aws_crate::config::Builder) -> $aws_crate::config::Builder>(f: T) -> T {
            f
        }

        $aws_crate::client::Client::from_conf(
            coerce($additional_configuration)(
                $aws_crate::config::Config::builder()
                    .with_test_defaults()
                    .region($aws_crate::config::Region::from_static("us-east-1"))
                    .http_client(mock_http_client)
                    .interceptor(mock_response_interceptor),
            )
            .build(),
        )
    }};
}

type MatchFn = Arc<dyn Fn(&Input) -> bool + Send + Sync>;
type OutputFn = Arc<dyn Fn() -> Result<Output, OrchestratorError<Error>> + Send + Sync>;
type HttpResponseFn = Arc<dyn Fn() -> Result<HttpResponse, BoxError> + Send + Sync>;

#[derive(Clone)]
enum MockOutput {
    HttpResponse(HttpResponseFn),
    ModeledResponse(OutputFn),
}

impl fmt::Debug for MockOutput {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MockOutput::HttpResponse(_) => write!(f, "MockOutput::HttpResponse"),
            MockOutput::ModeledResponse(_) => write!(f, "MockOutput::ModeledResponse"),
        }
    }
}

/// RuleMode describes how rules will be interpreted.
/// - In RuleMode::MatchAny, the first matching rule will be applied, and the rules will remain unchanged.
/// - In RuleMode::Sequential, the first matching rule will be applied, and that rule will be removed from the list of rules.
#[derive()]
pub enum RuleMode {
    MatchAny,
    Sequential,
}

pub struct RuleBuilder<I, O, E> {
    _ty: PhantomData<(I, O, E)>,
    input_filter: MatchFn,
    responses: Vec<ResponseEntry>,
}

impl<I, O, E> RuleBuilder<I, O, E>
where
    I: Send + Sync + fmt::Debug + 'static,
    O: Send + Sync + fmt::Debug + 'static,
    E: Send + Sync + fmt::Debug + std::error::Error + 'static,
{
    /// Creates a new [`RuleBuilder`]. This is normally constructed with the [`mock!`] macro
    pub fn new<F, R>(_input_hint: impl Fn() -> I, _output_hint: impl Fn() -> F) -> Self
    where
        F: Future<Output = Result<O, SdkError<E, R>>>,
    {
        Self {
            _ty: Default::default(),
            input_filter: Arc::new(|i: &Input| i.downcast_ref::<I>().is_some()),
            responses: Vec::new(),
        }
    }

    /// Add an additional filter to constrain which inputs match this rule.
    ///
    /// For examples, see the examples directory of this repository.
    pub fn match_requests(mut self, filter: impl Fn(&I) -> bool + Send + Sync + 'static) -> Self {
        self.input_filter = Arc::new(move |i: &Input| match i.downcast_ref::<I>() {
            Some(typed_input) => filter(typed_input),
            _ => false,
        });
        self
    }

    /// If the rule matches, then return a specific HTTP response.
    ///
    /// This is the recommended way of testing error behavior.
    pub fn then_http_response(
        mut self,
        response: impl Fn() -> HttpResponse + Send + Sync + 'static,
    ) -> Self {
        self.responses.push(ResponseEntry::new(
            MockOutput::HttpResponse(Arc::new(move || Ok(response()))),
            0,
        ));
        self
    }

    /// If a rule matches, then return a specific output
    pub fn then_output(mut self, output: impl Fn() -> O + Send + Sync + 'static) -> Self {
        self.responses.push(ResponseEntry::new(
            MockOutput::ModeledResponse(Arc::new(move || Ok(Output::erase(output())))),
            0,
        ));
        self
    }

    /// If a rule matches, then return a specific error
    ///
    /// Although this _basically_ works, using `then_http_response` is strongly recommended to
    /// create a higher fidelity mock. Error handling is quite complex in practice and returning errors
    /// directly often will not perfectly capture the way the error is actually returned to the SDK.
    pub fn then_error(mut self, output: impl Fn() -> E + Send + Sync + 'static) -> Self {
        self.responses.push(ResponseEntry::new(
            MockOutput::ModeledResponse(Arc::new(move || {
                Err(OrchestratorError::operation(Error::erase(output())))
            })),
            0,
        ));
        self
    }

    /// Repeat the last response in the sequence a specified number of times.
    ///
    /// This is useful for testing retry behavior. For example, you can return a 503 error
    /// response multiple times before returning a success response.
    ///
    /// # Examples
    ///
    /// rust
    /// use aws_sdk_s3::operation::get_object::GetObjectOutput;
    /// use aws_sdk_s3::Client;
    /// use aws_smithy_types::byte_stream::ByteStream;
    /// use aws_smithy_mocks::mock;
    /// let get_object_output = mock!(Client::get_object)
    ///   .match_requests(|req|req.bucket() == Some("test-bucket") && req.key() == Some("test-key"))
    ///   .then_http_response(||
    ///       http::Response::builder()
    ///           .status(503)
    ///           .body(SdkBody::from(&b""[..]))
    ///           .unwrap()
    ///   )
    ///   .repeat(2)  // Return the 503 response 3 times total (original + 2 repeats)
    ///   .then_output(||GetObjectOutput::builder().body(ByteStream::from_static(b"12345-abcde")).build())
    ///   .build();
    ///
    pub fn repeat(mut self, count: usize) -> Self {
        if let Some(last) = self.responses.last_mut() {
            last.repeat_count = count;
        } else {
            panic!("Cannot repeat: no response has been added yet");
        }
        self
    }

    /// Build the rule.
    ///
    /// This method creates a Rule from the RuleBuilder.
    pub fn build(self) -> Rule {
        if self.responses.is_empty() {
            panic!("Cannot build a rule with no responses");
        }
        Rule::new(self.input_filter, self.responses)
    }
}

#[derive(Debug, Clone)]
struct ResponseEntry {
    output: MockOutput,
    repeat_count: usize,
    current_repeat: Arc<AtomicUsize>,
}

impl ResponseEntry {
    fn new(output: MockOutput, repeat_count: usize) -> Self {
        Self {
            output,
            repeat_count,
            current_repeat: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[derive(Clone)]
pub struct Rule {
    matcher: MatchFn,
    responses: Vec<ResponseEntry>,
    current_index: Arc<AtomicUsize>,
    used_count: Arc<AtomicUsize>,
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rule")
    }
}

impl Rule {
    fn new(matcher: MatchFn, responses: Vec<ResponseEntry>) -> Self {
        Self {
            matcher,
            responses,
            current_index: Arc::new(AtomicUsize::new(0)),
            used_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn record_usage(&self) {
        self.used_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the number of times this rule has been hit.
    pub fn num_calls(&self) -> usize {
        self.used_count.load(Ordering::Relaxed)
    }

    /// Get the next response in the sequence
    fn next_response(&self) -> Option<ResponseEntry> {
        let index = self.current_index.load(Ordering::Relaxed);
        if index >= self.responses.len() {
            return None;
        }

        let entry = &self.responses[index];
        let repeat_count = entry.repeat_count;
        let current_repeat = entry.current_repeat.fetch_add(1, Ordering::Relaxed);

        // If we've repeated enough times, move to the next response
        if current_repeat >= repeat_count {
            self.current_index.fetch_add(1, Ordering::Relaxed);
            entry.current_repeat.store(0, Ordering::Relaxed);
        }

        Some(entry.clone())
    }

    // Check if the sequence is exhausted
    fn is_exhausted(&self) -> bool {
        self.current_index.load(Ordering::Relaxed) >= self.responses.len()
    }
}

// Store active rule in config bag
#[derive(Debug, Clone)]
struct ActiveRule(Rule);

impl Storable for ActiveRule {
    type Storer = StoreReplace<ActiveRule>;
}

// Store response entry from the rule
#[derive(Debug, Clone)]
struct ActiveResponseEntry(ResponseEntry);

impl Storable for ActiveResponseEntry {
    type Storer = StoreReplace<ActiveResponseEntry>;
}

/// Interceptor which produces mock responses based on a list of rules
pub struct MockResponseInterceptor {
    rules: Arc<Mutex<VecDeque<Rule>>>,
    rule_mode: RuleMode,
    must_match: bool,
}

impl fmt::Debug for MockResponseInterceptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} rules", self.rules.lock().unwrap().len())
    }
}

impl Default for MockResponseInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

impl MockResponseInterceptor {
    pub fn new() -> Self {
        Self {
            rules: Default::default(),
            rule_mode: RuleMode::MatchAny,
            must_match: true,
        }
    }
    /// Add a rule to the Interceptor
    ///
    /// Rules are matched in order—this rule will only apply if all previous rules do not match.
    pub fn with_rule(self, rule: &Rule) -> Self {
        self.rules.lock().unwrap().push_back(rule.clone());
        self
    }

    /// Set the RuleMode to use when evaluating rules.
    ///
    /// See `RuleMode` enum for modes and how they are applied.
    pub fn rule_mode(mut self, rule_mode: RuleMode) -> Self {
        self.rule_mode = rule_mode;
        self
    }

    /// Allow passthrough for unmatched requests.
    ///
    /// By default, if a request doesn't match any rule, the interceptor will panic.
    /// This method allows unmatched requests to pass through.
    pub fn allow_passthrough(mut self) -> Self {
        self.must_match = false;
        self
    }
}

impl Intercept for MockResponseInterceptor {
    fn name(&self) -> &'static str {
        "MockResponseInterceptor"
    }

    fn modify_before_serialization(
        &self,
        context: &mut BeforeSerializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let mut rules = self.rules.lock().unwrap();
        let rule = match self.rule_mode {
            RuleMode::Sequential => {
                let rule = rules
                    .pop_front()
                    .expect("no more rules but a new request was received");
                if !(rule.matcher)(context.input()) {
                    panic!(
                        "In order matching was enforced but the next rule did not match {:?}",
                        context.input()
                    );
                }
                Some(rule)
            }
            RuleMode::MatchAny => rules
                .iter()
                .find(|rule| (rule.matcher)(context.input()))
                .cloned(),
        };

        match rule {
            Some(rule) => {
                // Store the rule in the config bag
                cfg.interceptor_state().store_put(ActiveRule(rule));
            }
            None => {
                if self.must_match {
                    panic!(
                        "must_match was enabled but no rules matches {:?}",
                        context.input()
                    );
                }
            }
        }

        Ok(())
    }

    fn modify_before_transmit(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(active_rule) = cfg.load::<ActiveRule>() {
            if let Some(entry) = active_rule.0.next_response() {
                active_rule.0.record_usage();
                if let MockOutput::HttpResponse(output_fn) = &entry.output {
                    // Store the function as an extension and the HTTP client will use it
                    context
                        .request_mut()
                        .add_extension(MockHttpResponse(output_fn.clone()));
                }

                // Store the response entry in the config bag for this attempt
                cfg.interceptor_state()
                    .store_put(ActiveResponseEntry(entry));
            } else {
                // TODO - error if not match/passthrough?
            }
        }
        Ok(())
    }

    fn modify_before_attempt_completion(
        &self,
        context: &mut FinalizerInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(entry) = cfg.load::<ActiveResponseEntry>() {
            if let MockOutput::ModeledResponse(output_fn) = &entry.0.output {
                let result = output_fn();
                if result.is_err() {
                    // The orchestrator will panic if no response is present
                    context.inner_mut().set_response(Response::new(
                        StatusCode::try_from(500).unwrap(),
                        SdkBody::from("stubbed error response"),
                    ))
                }

                context.inner_mut().set_output_or_error(result);
            }
        }

        Ok(())
    }
}

/// Extension for storing mock HTTP responses in request extensions
#[derive(Clone)]
struct MockHttpResponse(HttpResponseFn);

/// Create a mock HTTP client that works with the interceptor using existing utilities
pub fn create_mock_http_client() -> SharedHttpClient {
    infallible_client_fn(|req| {
        // Try to get the mock HTTP response generator from the extensions
        if let Some(mock_response_gen) = req.extensions().get::<MockHttpResponse>() {
            // Invoke the function to get the actual response
            match (mock_response_gen.0)() {
                Ok(response) => return response.try_into_http1x().unwrap(),
                Err(e) => panic!(
                    "Error generating mock HTTP response from provided closure: {}",
                    e
                ),
            }
        }

        // Default dummy response if no mock response is defined
        http::Response::builder()
            .status(418)
            .body(SdkBody::from("Mock HTTP client dummy response"))
            .unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};
    use aws_smithy_runtime::client::orchestrator::operation::Operation;
    use aws_smithy_runtime::client::retries::classifiers::HttpStatusCodeClassifier;
    use aws_smithy_runtime_api::client::interceptors::context::Input;
    use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
    use aws_smithy_types::body::SdkBody;
    use aws_smithy_types::retry::RetryConfig;
    use aws_smithy_types::timeout::TimeoutConfig;

    use std::time::Duration;

    // Simple test input and output types
    #[derive(Debug)]
    struct TestInput {
        bucket: String,
        key: String,
    }
    impl TestInput {
        fn new(bucket: &str, key: &str) -> Self {
            Self {
                bucket: bucket.to_string(),
                key: key.to_string(),
            }
        }
    }

    #[derive(Debug, PartialEq)]
    struct TestOutput {
        content: String,
    }

    impl TestOutput {
        fn new(content: &str) -> Self {
            Self {
                content: content.to_string()
            }
        }
    }

    #[derive(Debug)]
    struct TestError {
        message: String,
    }

    impl TestError {
        fn new(message: &str) -> Self {
            Self {
                message: message.to_string(),
            }
        }
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl std::error::Error for TestError {}


    // Helper function to create a RuleBuilder with proper type hints
    fn create_rule_builder() -> RuleBuilder<TestInput, TestOutput, TestError> {
        RuleBuilder::new(
            || TestInput { bucket: "".to_string(), key: "".to_string() },
            || {
                let fut: std::future::Ready<Result<TestOutput, SdkError<TestError, HttpResponse>>> =
                    std::future::ready(Ok(TestOutput { content: "".to_string() }));
                fut
            }
        )
    }

    // Helper function to create an Operation with common configuration
    fn create_test_operation(
        interceptor: MockResponseInterceptor,
        enable_retries: bool
    ) -> Operation<TestInput, TestOutput, TestError> {
        let builder = Operation::builder()
            .service_name("test")
            .operation_name("test")
            .http_client(create_mock_http_client())
            .endpoint_url("http://localhost:1234")
            .no_auth()
            .sleep_impl(SharedAsyncSleep::new(TokioSleep::new()))
            .timeout_config(TimeoutConfig::disabled())
            .interceptor(interceptor)
            .serializer(|input: TestInput| {
                let mut request = HttpRequest::new(SdkBody::empty());
                request.set_uri(format!("/{}/{}", input.bucket, input.key)).expect("valid URI");
                Ok(request)
            })
            .deserializer::<TestOutput, TestError>(|response| {
                if response.status().is_success() {
                    let body = std::str::from_utf8(response.body().bytes().unwrap())
                        .unwrap_or("empty body")
                        .to_string();
                    Ok(TestOutput { content: body })
                } else {
                    Err(OrchestratorError::operation(TestError {
                        message: format!("Error: {}", response.status()),
                    }))
                }
            });

        if enable_retries {
            let retry_config = RetryConfig::standard()
                .with_max_attempts(5)
                .with_initial_backoff(Duration::from_millis(1))
                .with_max_backoff(Duration::from_millis(5));

            builder.retry_classifier(HttpStatusCodeClassifier::default())
                .standard_retry(&retry_config)
                .build()
        }else {
            builder.no_retry().build()
        }
    }

    #[test]
    fn test_rule_builder_sequence() {
        // Create a RuleBuilder with a sequence of responses
        let rule = create_rule_builder()
            .match_requests(|input| input.bucket == "test-bucket" && input.key == "test-key")
            .then_output(|| TestOutput::new("first response"))
            .then_http_response(|| {
                HttpResponse::new(
                    StatusCode::try_from(503).unwrap(),
                    SdkBody::from("service unavailable"),
                )
            })
            .then_output(|| TestOutput::new("second response"))
            .build();

        // Verify the rule has the expected structure
        assert_eq!(rule.responses.len(), 3);

        // Test that the matcher works
        let matching_input = TestInput::new("test-bucket", "test-key");
        let non_matching_input = TestInput::new("wrong-bucket", "test-key");

        let input_matches = (rule.matcher)(&Input::erase(matching_input));
        let input_does_not_match = (rule.matcher)(&Input::erase(non_matching_input));

        assert!(input_matches);
        assert!(!input_does_not_match);
    }

    #[test]
    fn test_rule_builder_repetition() {
        // Create a RuleBuilder with a repeated response
        let rule = create_rule_builder()
            .then_http_response(|| {
                HttpResponse::new(
                    StatusCode::try_from(503).unwrap(),
                    SdkBody::from("service unavailable"),
                )
            })
            .repeat(2) // Repeat 3 times total (original + 2 repeats)
            .then_output(|| TestOutput::new("success"))
            .build();

        // Verify the rule has the expected structure
        assert_eq!(rule.responses.len(), 2);
        assert_eq!(rule.responses[0].repeat_count, 2);
        assert_eq!(rule.responses[1].repeat_count, 0);
    }

    #[tokio::test]
    async fn test_retry_with_repeat() {
        // Create a rule with repeated error responses followed by success
        let rule = create_rule_builder()
            .match_requests(|input| input.bucket == "test-bucket" && input.key == "test-key")
            // First response: 503 Service Unavailable repeated 3 times (should trigger retries)
            .then_http_response(|| {
                HttpResponse::new(
                    StatusCode::try_from(503).unwrap(),
                    SdkBody::from("service unavailable"),
                )
            })
            .repeat(2) // Repeat 3 times total (original + 2 repeats)
            // Final response: 200 OK with success output
            .then_output(|| TestOutput::new("success after retries"))
            .build();

        // Create an interceptor with the rule
        let interceptor = MockResponseInterceptor::new()
            .rule_mode(RuleMode::Sequential)
            .with_rule(&rule);

        let operation = create_test_operation(interceptor, true);

        // Make a single request - it should automatically retry through the sequence
        let result = operation
            .invoke(TestInput::new("test-bucket", "test-key"))
            .await;

        // Should succeed with the final output after retries
        assert!(
            result.is_ok(),
            "Expected success but got error: {:?}",
            result.err()
        );
        assert_eq!(
            result.unwrap(),
            TestOutput {
                content: "success after retries".to_string()
            }
        );

        // Verify the rule was used the expected number of times (all 4 responses: 3 errors + 1 success)
        assert_eq!(rule.num_calls(), 4);
    }

    #[should_panic(expected = "Cannot build a rule with no responses")]
    #[test]
    fn test_empty_rule_builder_panics() {
        // Creating a rule with no responses should panic
        let _rule = create_rule_builder().build();
    }

    #[test]
    fn test_zero_repetition() {
        // Test that setting repeat(0) doesn't change behavior
        let rule = create_rule_builder()
            .then_output(|| TestOutput::new("first"))
            .repeat(0)  // Should have no effect
            .then_output(|| TestOutput::new("second"))
            .build();

        // Verify the rule has the expected structure
        assert_eq!(rule.responses.len(), 2);
        assert_eq!(rule.responses[0].repeat_count, 0);
    }

    #[should_panic(expected = "Cannot repeat: no response has been added yet")]
    #[test]
    fn test_repeat_without_response_panics() {
        // Calling repeat() before adding any responses should panic
        let _rule = create_rule_builder().repeat(2).build();
    }

    #[should_panic(expected = "no more rules but a new request was received")]
    #[tokio::test]
    async fn test_exhausted_rules() {
        // Create a rule with a single response
        let rule = create_rule_builder()
            .then_output(|| TestOutput::new("only response"))
            .build();

        // Create an interceptor with the rule
        let interceptor = MockResponseInterceptor::new()
            .rule_mode(RuleMode::Sequential)
            .with_rule(&rule);

        let operation = create_test_operation(interceptor, false);

        // First call should succeed
        let result1 = operation
            .invoke(TestInput::new("test-bucket", "test-key"))
            .await;
        assert!(result1.is_ok());

        // Second call should panic because the rules are exhausted
        let _result2 = operation
            .invoke(TestInput::new("test-bucket", "test-key"))
            .await;
    }

    #[tokio::test]
    async fn test_rule_mode_match_any() {
        // Create two rules with different matchers
        let rule1 = create_rule_builder()
            .match_requests(|input| input.bucket == "bucket1")
            .then_output(|| TestOutput::new("response1"))
            .build();

        let rule2 = create_rule_builder()
            .match_requests(|input| input.bucket == "bucket2")
            .then_output(|| TestOutput::new("response2"))
            .build();


        // Create an interceptor with both rules in MatchAny mode
        let interceptor = MockResponseInterceptor::new()
            .rule_mode(RuleMode::MatchAny)
            .with_rule(&rule1)
            .with_rule(&rule2);

        let operation = create_test_operation(interceptor, false);

        // Call with bucket1 should match rule1
        let result1 = operation
            .invoke(TestInput::new("bucket1", "test-key"))
            .await;
        assert!(result1.is_ok());
        assert_eq!(
            result1.unwrap(),
            TestOutput::new("response1")
        );

        // Call with bucket2 should match rule2
        let result2 = operation
            .invoke(TestInput::new("bucket2", "test-key"))
            .await;
        assert!(result2.is_ok());
        assert_eq!(
            result2.unwrap(),
            TestOutput::new("response2")
        );

        // Verify the rules were used the expected number of times
        assert_eq!(rule1.num_calls(), 1);
        assert_eq!(rule2.num_calls(), 1);
    }
}
