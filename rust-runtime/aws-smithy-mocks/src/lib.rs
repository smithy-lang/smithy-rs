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
    BeforeDeserializationInterceptorContextMut, BeforeSerializationInterceptorContextMut,
    BeforeTransmitInterceptorContextMut, Error, FinalizerInterceptorContextMut, Input, Output,
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

// Store active rule and response in config bag
#[derive(Debug, Clone)]
struct ActiveRule(Rule, Option<ResponseEntry>);

impl Storable for ActiveRule {
    type Storer = StoreReplace<ActiveRule>;
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
                // Get the next response from the rule
                let response_entry = rule.next_response();
                // Store the rule and response entry in the config bag
                cfg.interceptor_state()
                    .store_put(ActiveRule(rule, response_entry));
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
            // If the response is an HTTP response, store the function in the request extensions
            if let Some(entry) = &active_rule.1 {
                if let MockOutput::HttpResponse(output_fn) = &entry.output {
                    // Store the function which is cloneable (needed by Extensions), not the result
                    context
                        .request_mut()
                        .add_extension(MockHttpResponse(output_fn.clone()));
                }
            }
        }
        Ok(())
    }

    fn modify_before_deserialization(
        &self,
        context: &mut BeforeDeserializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(active_rule) = cfg.load::<ActiveRule>() {
            let rule = &active_rule.0;
            let response_entry = &active_rule.1;

            if let Some(entry) = response_entry {
                if let MockOutput::HttpResponse(output_fn) = &entry.output {
                    let result = output_fn();
                    rule.record_usage();

                    match result {
                        Ok(http_response) => *context.response_mut() = http_response,
                        Err(e) => context
                            .inner_mut()
                            .set_output_or_error(Err(OrchestratorError::response(e))),
                    }
                }
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
        if let Some(active_rule) = cfg.load::<ActiveRule>() {
            let rule = &active_rule.0;
            let response_entry = &active_rule.1;

            if let Some(entry) = response_entry {
                if let MockOutput::ModeledResponse(output_fn) = &entry.output {
                    let result = output_fn();
                    rule.record_usage();

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
            .status(200)
            .body(SdkBody::from("Mock HTTP client dummy response"))
            .unwrap()
    })
}
