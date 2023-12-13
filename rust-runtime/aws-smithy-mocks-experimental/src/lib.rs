/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
/* End of automatically managed default lints */
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeDeserializationInterceptorContextMut, BeforeSerializationInterceptorContextMut, Error,
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

type MatchFn = Arc<dyn Fn(&Input) -> bool + Send + Sync>;
type OutputFn = Arc<dyn Fn() -> Result<Output, OrchestratorError<Error>> + Send + Sync>;

impl Debug for MockResponseInterceptor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} functions", self.rules.len())
    }
}

#[derive(Clone)]
pub enum MockOutput {
    HttpResponse(Arc<dyn Fn() -> Result<HttpResponse, BoxError> + Send + Sync>),
    ModeledResponse(OutputFn),
}

/// Interceptor which produces mock responses based on a list of rules
pub struct MockResponseInterceptor {
    rules: Vec<Rule>,
}

pub struct RuleBuilder<I, O, E> {
    _ty: PhantomData<(I, O, E)>,
    input_filter: MatchFn,
}

impl<I, O, E> RuleBuilder<I, O, E>
where
    I: Send + Sync + Debug + 'static,
    O: Send + Sync + Debug + 'static,
    E: Send + Sync + Debug + std::error::Error + 'static,
{
    pub fn new<F, R>(_input_hint: impl Fn() -> I, _output_hint: impl Fn() -> F) -> Self
    where
        F: Future<Output = Result<O, SdkError<E, R>>>,
    {
        Self {
            _ty: Default::default(),
            input_filter: Arc::new(|i: &Input| i.downcast_ref::<I>().is_some()),
        }
    }

    /// Add an additional filter to constrain which inputs match this rule
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
        self,
        response: impl Fn() -> HttpResponse + Send + Sync + 'static,
    ) -> Rule {
        Rule::new(
            self.input_filter,
            MockOutput::HttpResponse(Arc::new(move || Ok(response()))),
        )
    }

    /// If a rule matches, then return a specific output
    pub fn then_output(self, output: impl Fn() -> O + Send + Sync + 'static) -> Rule {
        Rule::new(
            self.input_filter,
            MockOutput::ModeledResponse(Arc::new(move || Ok(Output::erase(output())))),
        )
    }

    /// If a rule matches, then return a specific error
    pub fn then_error(self, output: impl Fn() -> E + Send + Sync + 'static) -> Rule {
        Rule::new(
            self.input_filter,
            MockOutput::ModeledResponse(Arc::new(move || {
                Err(OrchestratorError::operation(Error::erase(output())))
            })),
        )
    }
}

#[derive(Clone)]
pub struct Rule {
    matcher: MatchFn,
    output: MockOutput,
    used_count: Arc<AtomicUsize>,
}

impl Rule {
    fn new(matcher: MatchFn, output: MockOutput) -> Self {
        Self {
            matcher,
            output,
            used_count: Default::default(),
        }
    }
    fn record_usage(&self) {
        self.used_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the number of times this rule has been hit.
    pub fn num_calls(&self) -> usize {
        self.used_count.load(Ordering::Relaxed)
    }
}

#[derive(Debug)]
struct RunIndex(usize);
impl Storable for RunIndex {
    type Storer = StoreReplace<RunIndex>;
}

impl MockResponseInterceptor {
    pub fn new() -> Self {
        Self { rules: vec![] }
    }
    pub fn with_rule(mut self, rule: &Rule) -> Self {
        self.rules.push(rule.clone());
        self
    }
}

impl Intercept for MockResponseInterceptor {
    fn name(&self) -> &'static str {
        "test"
    }

    fn modify_before_serialization(
        &self,
        context: &mut BeforeSerializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        for (idx, rule) in self.rules.iter().enumerate() {
            if (rule.matcher)(context.inner().input().unwrap()) {
                cfg.interceptor_state().store_put(RunIndex(idx));
                return Ok(());
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
        if let Some(idx) = cfg.load::<RunIndex>() {
            let rule = &self.rules[idx.0];
            let result = match &rule.output {
                MockOutput::HttpResponse(output_fn) => output_fn(),
                _ => return Ok(()),
            };
            rule.record_usage();
            match result {
                Ok(http_response) => *context.response_mut() = http_response,
                Err(e) => context
                    .inner_mut()
                    .set_output_or_error(Err(OrchestratorError::response(e))),
            }
        }
        Ok(())
    }

    fn modify_before_attempt_completion(
        &self,
        context: &mut FinalizerInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(idx) = _cfg.load::<RunIndex>() {
            let rule = &self.rules[idx.0];
            let result = match &rule.output {
                MockOutput::ModeledResponse(output_fn) => output_fn(),
                _ => return Ok(()),
            };

            rule.record_usage();
            if result.is_err() {
                context.inner_mut().set_response(Response::new(
                    StatusCode::try_from(500).unwrap(),
                    SdkBody::from("stubbed error response"),
                ))
            }
            context.inner_mut().set_output_or_error(result);
        }
        Ok(())
    }
}
