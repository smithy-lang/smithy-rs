/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A table-driven regression net over every interceptor hook.
//!
//! Rather than one targeted test per bug, this module asserts the *expected invocation count of
//! every hook in every scenario*. A deviation in either direction fails: a hook that stops firing,
//! and a hook that starts firing when it should not. That is what makes it a regression net rather
//! than a fix-specific test.
//!
//! Four things are asserted for each run:
//!
//! 1. **Counts** — symbolic, because attempt-level and execution-level hooks have different
//!    cardinality. See [`Expect`].
//! 2. **Error fidelity** — what the finalizer hooks actually *observed*. A count-only matrix would
//!    accept a fix where all four finalizers fire but see an empty context, which would leave
//!    `modify_before_attempt_completion` and `modify_before_completion` unable to do their
//!    documented job.
//! 3. **Lifecycle ordering** — every step advances through `known_hooks!()` order, with one
//!    permitted exception: a new attempt resets to `read_before_attempt`.
//! 4. **Dispatch-path agreement** — whether a hook is called at all is gated by a bitmask.
//!    `overridden_hooks()` defaults to `all()`, so a plain `impl Intercept` only exercises the
//!    unconditional path. Interceptors using `#[dyn_dispatch_hint]` get a generated bitmask, and a
//!    bug there would silently stop a hook firing. Every scenario therefore runs twice, against
//!    recorders with identical bodies, and both must satisfy the same expectations.
//!
//! The hook list itself is checked against `known_hooks!()` so that adding a hook to the runtime
//! cannot silently leave it untested.

use crate::client::auth::no_auth::{NoAuthRuntimePluginV2, NO_AUTH_SCHEME_ID};
use crate::client::defaults::{default_sleep_impl_plugin, default_time_source_plugin};
use crate::client::orchestrator::endpoints::StaticUriEndpointResolver;
use crate::client::orchestrator::{invoke_with_stop_point, StopPoint};
use crate::client::retries::strategy::NeverRetryStrategy;
use crate::client::test_util::{
    deserializer::CannedResponseDeserializer, serializer::CannedRequestSerializer,
};
use aws_smithy_http_client::test_util::NeverClient;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::auth::static_resolver::StaticAuthSchemeOptionResolver;
use aws_smithy_runtime_api::client::auth::{
    AuthSchemeOptionResolverParams, SharedAuthSchemeOptionResolver,
};
use aws_smithy_runtime_api::client::endpoint::{EndpointResolverParams, SharedEndpointResolver};
use aws_smithy_runtime_api::client::http::{http_client_fn, HttpConnector, HttpConnectorFuture};
use aws_smithy_runtime_api::client::interceptors::context::{
    AfterDeserializationInterceptorContextRef, BeforeDeserializationInterceptorContextMut,
    BeforeDeserializationInterceptorContextRef, BeforeSerializationInterceptorContextMut,
    BeforeSerializationInterceptorContextRef, BeforeTransmitInterceptorContextMut,
    BeforeTransmitInterceptorContextRef, FinalizerInterceptorContextMut,
    FinalizerInterceptorContextRef, Input, InterceptorContext, Output,
};
use aws_smithy_runtime_api::client::interceptors::{
    dyn_dispatch_hint, known_hooks, Intercept, SharedInterceptor,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, OrchestratorError};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_runtime_api::client::retries::{
    RequestAttempts, RetryStrategy, SharedRetryStrategy, ShouldAttempt,
};
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponents, RuntimeComponentsBuilder,
};
use aws_smithy_runtime_api::client::runtime_plugin::{RuntimePlugin, RuntimePlugins};
use aws_smithy_runtime_api::client::ser_de::{SharedRequestSerializer, SharedResponseDeserializer};
use aws_smithy_runtime_api::shared::IntoShared;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::{ConfigBag, FrozenLayer, Layer};
use aws_smithy_types::timeout::TimeoutConfig;
use http_1x::{Response, StatusCode};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The hooks this matrix covers, in lifecycle order.
///
/// Asserted against `known_hooks!()` below, so adding a hook to the runtime forces a matrix update
/// rather than silently leaving the new hook uncovered. Spelled out rather than taken from
/// `known_hooks!()` directly so that a *reordering* is caught too, not only an addition.
const HOOKS: &[&str] = &[
    "read_before_execution",
    "modify_before_serialization",
    "read_before_serialization",
    "read_after_serialization",
    "modify_before_retry_loop",
    "read_before_attempt",
    "modify_before_signing",
    "read_before_signing",
    "read_after_signing",
    "modify_before_transmit",
    "read_before_transmit",
    "read_after_transmit",
    "modify_before_deserialization",
    "read_before_deserialization",
    "read_after_deserialization",
    "modify_before_attempt_completion",
    "read_after_attempt",
    "modify_before_completion",
    "read_after_execution",
];

/// Expected invocation count for a hook in a given scenario.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Expect {
    /// Never invoked.
    Never,
    /// Exactly once per execution.
    Once,
    /// Once per attempt actually started, measured by the `read_before_attempt` count.
    ///
    /// A literal would not do: a consumer detects a missing invocation with an equality against the
    /// attempt count, so a fix with the wrong cardinality would silently corrupt its metrics.
    PerAttempt,
    /// Once per attempt that received a response, else zero.
    PerResponse,
}
use Expect::*;

/// What a finalizer hook saw in `output_or_error` when it ran.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Observed {
    /// The slot was empty. The hook cannot tell whether anything went wrong, and the mutating
    /// finalizers have nothing to replace. This is the failure mode that a count-only matrix
    /// would miss.
    Nothing,
    Success,
    Timeout,
    ConnectorFailure,
    OtherError,
}

impl Observed {
    fn of<O, E>(outcome: Option<Result<&O, &OrchestratorError<E>>>) -> Self {
        match outcome {
            None => Observed::Nothing,
            Some(Ok(_)) => Observed::Success,
            Some(Err(err)) if err.is_timeout_error() => Observed::Timeout,
            Some(Err(err)) if err.is_connector_error() => Observed::ConnectorFailure,
            Some(Err(_)) => Observed::OtherError,
        }
    }
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

/// Shared recording state for one run.
#[derive(Clone, Debug, Default)]
struct HookLog {
    counts: Arc<Mutex<BTreeMap<&'static str, usize>>>,
    /// Invocation sequence, for the ordering assertion.
    sequence: Arc<Mutex<Vec<&'static str>>>,
    /// What each finalizer hook observed, in invocation order.
    observed: Arc<Mutex<Vec<(&'static str, Observed)>>>,
}

impl HookLog {
    fn record(&self, hook: &'static str) {
        *self.counts.lock().unwrap().entry(hook).or_insert(0) += 1;
        self.sequence.lock().unwrap().push(hook);
    }

    fn record_finalizer(&self, hook: &'static str, observed: Observed) {
        self.record(hook);
        self.observed.lock().unwrap().push((hook, observed));
    }

    fn count(&self, hook: &str) -> usize {
        self.counts.lock().unwrap().get(hook).copied().unwrap_or(0)
    }
}

/// Generates a non-finalizer hook body that just records the call.
macro_rules! recording_hook {
    ($name:ident, $ctx:ident) => {
        fn $name(
            &self,
            _ctx: &$ctx<'_>,
            _components: &RuntimeComponents,
            _cfg: &mut ConfigBag,
        ) -> Result<(), BoxError> {
            self.0.record(stringify!($name));
            Ok(())
        }
    };
    (mut $name:ident, $ctx:ident) => {
        fn $name(
            &self,
            _ctx: &mut $ctx<'_>,
            _components: &RuntimeComponents,
            _cfg: &mut ConfigBag,
        ) -> Result<(), BoxError> {
            self.0.record(stringify!($name));
            Ok(())
        }
    };
}

/// Recorder exercising the unconditional dispatch path.
///
/// `overridden_hooks()` is left to its default of `all()`, so every hook is dispatched without
/// consulting a bitmask.
#[derive(Clone, Debug, Default)]
struct PlainRecorder(HookLog);

impl Intercept for PlainRecorder {
    fn name(&self) -> &'static str {
        "PlainRecorder"
    }

    // `read_before_execution` is the one hook that takes no `RuntimeComponents`.
    fn read_before_execution(
        &self,
        _ctx: &BeforeSerializationInterceptorContextRef<'_>,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_before_execution");
        Ok(())
    }

    recording_hook!(
        mut modify_before_serialization,
        BeforeSerializationInterceptorContextMut
    );
    recording_hook!(
        read_before_serialization,
        BeforeSerializationInterceptorContextRef
    );
    recording_hook!(
        read_after_serialization,
        BeforeTransmitInterceptorContextRef
    );
    recording_hook!(
        mut modify_before_retry_loop,
        BeforeTransmitInterceptorContextMut
    );
    recording_hook!(read_before_attempt, BeforeTransmitInterceptorContextRef);
    recording_hook!(
        mut modify_before_signing,
        BeforeTransmitInterceptorContextMut
    );
    recording_hook!(read_before_signing, BeforeTransmitInterceptorContextRef);
    recording_hook!(read_after_signing, BeforeTransmitInterceptorContextRef);
    recording_hook!(
        mut modify_before_transmit,
        BeforeTransmitInterceptorContextMut
    );
    recording_hook!(read_before_transmit, BeforeTransmitInterceptorContextRef);
    recording_hook!(
        read_after_transmit,
        BeforeDeserializationInterceptorContextRef
    );
    recording_hook!(
        mut modify_before_deserialization,
        BeforeDeserializationInterceptorContextMut
    );
    recording_hook!(
        read_before_deserialization,
        BeforeDeserializationInterceptorContextRef
    );
    recording_hook!(
        read_after_deserialization,
        AfterDeserializationInterceptorContextRef
    );

    // The four finalizers additionally record what they observed.
    fn modify_before_attempt_completion(
        &self,
        ctx: &mut FinalizerInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record_finalizer(
            "modify_before_attempt_completion",
            Observed::of(ctx.output_or_error()),
        );
        Ok(())
    }

    fn read_after_attempt(
        &self,
        ctx: &FinalizerInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0
            .record_finalizer("read_after_attempt", Observed::of(ctx.output_or_error()));
        Ok(())
    }

    fn modify_before_completion(
        &self,
        ctx: &mut FinalizerInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record_finalizer(
            "modify_before_completion",
            Observed::of(ctx.output_or_error()),
        );
        Ok(())
    }

    fn read_after_execution(
        &self,
        ctx: &FinalizerInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0
            .record_finalizer("read_after_execution", Observed::of(ctx.output_or_error()));
        Ok(())
    }
}

/// Recorder exercising the bitmask dispatch path.
///
/// Identical behaviour to [`PlainRecorder`], but `overridden_hooks()` is generated by
/// `#[dyn_dispatch_hint]`.
///
/// Every method is written out longhand on purpose. The attribute parses the impl block *before*
/// declarative macros expand, so a hook defined by a macro appears as an `ImplItem::Macro` rather
/// than an `ImplItem::Fn`, gets omitted from the generated bitmask, and is then never dispatched —
/// implemented but never invoked, with no warning. Do not refactor these into `recording_hook!`.
#[derive(Clone, Debug, Default)]
struct HintedRecorder(HookLog);

#[dyn_dispatch_hint]
impl Intercept for HintedRecorder {
    fn name(&self) -> &'static str {
        "HintedRecorder"
    }

    fn read_before_execution(
        &self,
        _ctx: &BeforeSerializationInterceptorContextRef<'_>,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_before_execution");
        Ok(())
    }

    fn modify_before_serialization(
        &self,
        _ctx: &mut BeforeSerializationInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("modify_before_serialization");
        Ok(())
    }

    fn read_before_serialization(
        &self,
        _ctx: &BeforeSerializationInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_before_serialization");
        Ok(())
    }

    fn read_after_serialization(
        &self,
        _ctx: &BeforeTransmitInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_after_serialization");
        Ok(())
    }

    fn modify_before_retry_loop(
        &self,
        _ctx: &mut BeforeTransmitInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("modify_before_retry_loop");
        Ok(())
    }

    fn read_before_attempt(
        &self,
        _ctx: &BeforeTransmitInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_before_attempt");
        Ok(())
    }

    fn modify_before_signing(
        &self,
        _ctx: &mut BeforeTransmitInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("modify_before_signing");
        Ok(())
    }

    fn read_before_signing(
        &self,
        _ctx: &BeforeTransmitInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_before_signing");
        Ok(())
    }

    fn read_after_signing(
        &self,
        _ctx: &BeforeTransmitInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_after_signing");
        Ok(())
    }

    fn modify_before_transmit(
        &self,
        _ctx: &mut BeforeTransmitInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("modify_before_transmit");
        Ok(())
    }

    fn read_before_transmit(
        &self,
        _ctx: &BeforeTransmitInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_before_transmit");
        Ok(())
    }

    fn read_after_transmit(
        &self,
        _ctx: &BeforeDeserializationInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_after_transmit");
        Ok(())
    }

    fn modify_before_deserialization(
        &self,
        _ctx: &mut BeforeDeserializationInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("modify_before_deserialization");
        Ok(())
    }

    fn read_before_deserialization(
        &self,
        _ctx: &BeforeDeserializationInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_before_deserialization");
        Ok(())
    }

    fn read_after_deserialization(
        &self,
        _ctx: &AfterDeserializationInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record("read_after_deserialization");
        Ok(())
    }

    fn modify_before_attempt_completion(
        &self,
        ctx: &mut FinalizerInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record_finalizer(
            "modify_before_attempt_completion",
            Observed::of(ctx.output_or_error()),
        );
        Ok(())
    }

    fn read_after_attempt(
        &self,
        ctx: &FinalizerInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0
            .record_finalizer("read_after_attempt", Observed::of(ctx.output_or_error()));
        Ok(())
    }

    fn modify_before_completion(
        &self,
        ctx: &mut FinalizerInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.record_finalizer(
            "modify_before_completion",
            Observed::of(ctx.output_or_error()),
        );
        Ok(())
    }

    fn read_after_execution(
        &self,
        ctx: &FinalizerInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0
            .record_finalizer("read_after_execution", Observed::of(ctx.output_or_error()));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct OkConnector;
impl HttpConnector for OkConnector {
    fn call(&self, _request: HttpRequest) -> HttpConnectorFuture {
        HttpConnectorFuture::ready(Ok(http_1x::Response::builder()
            .status(200)
            .body(SdkBody::empty())
            .expect("valid response")
            .try_into()
            .unwrap()))
    }
}

#[derive(Debug, Default)]
struct FailingConnector;
impl HttpConnector for FailingConnector {
    fn call(&self, _request: HttpRequest) -> HttpConnectorFuture {
        HttpConnectorFuture::ready(Err(ConnectorError::io("connector failed".into())))
    }
}

/// Fails the first attempt, succeeds afterwards, so that `PerAttempt` and `PerResponse` diverge.
#[derive(Debug)]
struct FailThenSucceedConnector(Arc<AtomicUsize>);
impl HttpConnector for FailThenSucceedConnector {
    fn call(&self, _request: HttpRequest) -> HttpConnectorFuture {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            HttpConnectorFuture::ready(Err(ConnectorError::io("first attempt failed".into())))
        } else {
            OkConnector.call(_request)
        }
    }
}

/// Retries up to `max_attempts`, optionally asking for a delay so that an operation timeout can
/// land during backoff rather than mid-attempt.
///
/// `initial_delay` additionally stalls the *initial* request. The orchestrator awaits that delay
/// after the prologue but before `read_before_attempt`, which is how a timeout can be made to land
/// with no attempt ever started.
#[derive(Debug)]
struct BoundedRetryStrategy {
    max_attempts: u32,
    delay: Option<Duration>,
    initial_delay: Option<Duration>,
}

impl BoundedRetryStrategy {
    fn new(max_attempts: u32, delay: Option<Duration>) -> Self {
        Self {
            max_attempts,
            delay,
            initial_delay: None,
        }
    }

    /// Stalls the initial request long enough for an operation timeout to cancel it.
    fn stalling_before_first_attempt(delay: Duration) -> Self {
        Self {
            max_attempts: 1,
            delay: None,
            initial_delay: Some(delay),
        }
    }
}

impl RetryStrategy for BoundedRetryStrategy {
    fn should_attempt_initial_request(
        &self,
        _components: &RuntimeComponents,
        _cfg: &ConfigBag,
    ) -> Result<ShouldAttempt, BoxError> {
        Ok(match self.initial_delay {
            Some(delay) => ShouldAttempt::YesAfterDelay(delay),
            None => ShouldAttempt::Yes,
        })
    }

    fn should_attempt_retry(
        &self,
        _ctx: &InterceptorContext,
        _components: &RuntimeComponents,
        cfg: &ConfigBag,
    ) -> Result<ShouldAttempt, BoxError> {
        let attempts = cfg
            .load::<RequestAttempts>()
            .map(|a| a.attempts())
            .unwrap_or(1);
        if attempts >= self.max_attempts {
            return Ok(ShouldAttempt::No);
        }
        Ok(match self.delay {
            Some(delay) => ShouldAttempt::YesAfterDelay(delay),
            None => ShouldAttempt::Yes,
        })
    }
}

/// Supplies the serializer, deserializer, endpoint and auth that every run needs.
#[derive(Debug)]
struct BaseRuntimePlugin(RuntimeComponentsBuilder);

impl BaseRuntimePlugin {
    fn new() -> Self {
        Self(
            RuntimeComponentsBuilder::for_tests()
                // Scenarios override these via `ScenarioPlugin` where they need something else.
                .with_retry_strategy(Some(SharedRetryStrategy::new(NeverRetryStrategy::new())))
                .with_http_client(Some(http_client_fn(|_, _| OkConnector.into_shared())))
                .with_endpoint_resolver(Some(SharedEndpointResolver::new(
                    StaticUriEndpointResolver::http_localhost(8080),
                )))
                .with_auth_scheme_option_resolver(Some(SharedAuthSchemeOptionResolver::new(
                    StaticAuthSchemeOptionResolver::new(vec![NO_AUTH_SCHEME_ID]),
                ))),
        )
    }
}

impl RuntimePlugin for BaseRuntimePlugin {
    fn config(&self) -> Option<FrozenLayer> {
        let mut layer = Layer::new("BaseRuntimePlugin");
        layer.store_put(AuthSchemeOptionResolverParams::new("dontcare"));
        layer.store_put(EndpointResolverParams::new("dontcare"));
        layer.store_put(SharedRequestSerializer::new(
            CannedRequestSerializer::success(HttpRequest::empty()),
        ));
        layer.store_put(SharedResponseDeserializer::new(
            CannedResponseDeserializer::new(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(SdkBody::empty())
                    .map_err(|err| OrchestratorError::other(Box::new(err)))
                    .map(Output::erase),
            ),
        ));
        Some(layer.freeze())
    }

    fn runtime_components(
        &self,
        _: &RuntimeComponentsBuilder,
    ) -> Cow<'_, RuntimeComponentsBuilder> {
        Cow::Borrowed(&self.0)
    }
}

/// Carries the per-scenario components and timeout config.
#[derive(Debug)]
struct ScenarioPlugin {
    components: RuntimeComponentsBuilder,
    timeout: Option<TimeoutConfig>,
}

impl RuntimePlugin for ScenarioPlugin {
    fn config(&self) -> Option<FrozenLayer> {
        let timeout = self.timeout.clone()?;
        let mut layer = Layer::new("ScenarioPlugin");
        layer.store_put(timeout);
        Some(layer.freeze())
    }

    fn runtime_components(
        &self,
        _: &RuntimeComponentsBuilder,
    ) -> Cow<'_, RuntimeComponentsBuilder> {
        Cow::Borrowed(&self.components)
    }
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

#[derive(Copy, Clone)]
enum Connector {
    Ok,
    Fail,
    FailThenOk,
    /// Never responds, so a timeout is the only way the attempt ends.
    Never,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DispatchPath {
    Plain,
    Hinted,
}

struct Scenario {
    name: &'static str,
    connector: Connector,
    timeout: Option<TimeoutConfig>,
    retry: Option<BoundedRetryStrategy>,
    /// Abandon the future instead of letting the operation finish, to pin caller-initiated
    /// cancellation behaviour.
    caller_abandons: bool,
    /// Attempts expected to start, i.e. the `read_before_attempt` count.
    attempts: usize,
    /// Attempts expected to receive a response.
    responses: usize,
    /// Expected count for each hook in [`HOOKS`] order.
    expected: [Expect; 19],
    /// Expected `Observed` for every finalizer invocation, in invocation order.
    expected_observations: Vec<(&'static str, Observed)>,
}

/// Builds an expectation row from the per-stage groups, so the scenarios read as a table.
fn row(
    prologue: Expect,
    attempt: Expect,
    deserialization: Expect,
    attempt_finalizers: Expect,
    execution_finalizers: Expect,
) -> [Expect; 19] {
    [
        // prologue: read_before_execution .. modify_before_retry_loop
        prologue,
        prologue,
        prologue,
        prologue,
        prologue,
        // attempt: read_before_attempt .. read_before_transmit
        attempt,
        attempt,
        attempt,
        attempt,
        attempt,
        attempt,
        // deserialization: read_after_transmit .. read_after_deserialization
        deserialization,
        deserialization,
        deserialization,
        deserialization,
        // finalizers
        attempt_finalizers,
        attempt_finalizers,
        execution_finalizers,
        execution_finalizers,
    ]
}

fn attempt_timeout(millis: u64) -> Option<TimeoutConfig> {
    Some(
        TimeoutConfig::builder()
            .operation_attempt_timeout(Duration::from_millis(millis))
            .build(),
    )
}

fn operation_timeout(millis: u64) -> Option<TimeoutConfig> {
    Some(
        TimeoutConfig::builder()
            .operation_timeout(Duration::from_millis(millis))
            .build(),
    )
}

/// Every finalizer in one attempt plus the execution, all observing the same thing.
fn observations(attempt: Observed, execution: Observed) -> Vec<(&'static str, Observed)> {
    vec![
        ("modify_before_attempt_completion", attempt),
        ("read_after_attempt", attempt),
        ("modify_before_completion", execution),
        ("read_after_execution", execution),
    ]
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "success",
            connector: Connector::Ok,
            timeout: None,
            retry: None,
            caller_abandons: false,
            attempts: 1,
            responses: 1,
            expected: row(Once, PerAttempt, PerResponse, PerAttempt, Once),
            expected_observations: observations(Observed::Success, Observed::Success),
        },
        Scenario {
            name: "retry then success",
            connector: Connector::FailThenOk,
            timeout: None,
            retry: Some(BoundedRetryStrategy::new(2, None)),
            caller_abandons: false,
            attempts: 2,
            responses: 1,
            expected: row(Once, PerAttempt, PerResponse, PerAttempt, Once),
            // The first attempt genuinely failed; the second succeeded.
            expected_observations: vec![
                (
                    "modify_before_attempt_completion",
                    Observed::ConnectorFailure,
                ),
                ("read_after_attempt", Observed::ConnectorFailure),
                ("modify_before_attempt_completion", Observed::Success),
                ("read_after_attempt", Observed::Success),
                ("modify_before_completion", Observed::Success),
                ("read_after_execution", Observed::Success),
            ],
        },
        Scenario {
            name: "transport error",
            connector: Connector::Fail,
            timeout: None,
            retry: None,
            caller_abandons: false,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, PerAttempt, Once),
            expected_observations: observations(
                Observed::ConnectorFailure,
                Observed::ConnectorFailure,
            ),
        },
        Scenario {
            name: "attempt timeout",
            connector: Connector::Never,
            timeout: attempt_timeout(200),
            retry: None,
            caller_abandons: false,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, PerAttempt, Once),
            expected_observations: observations(Observed::Timeout, Observed::Timeout),
        },
        Scenario {
            name: "operation timeout mid-attempt",
            connector: Connector::Never,
            timeout: operation_timeout(200),
            retry: None,
            caller_abandons: false,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, PerAttempt, Once),
            expected_observations: observations(Observed::Timeout, Observed::Timeout),
        },
        Scenario {
            name: "operation timeout during retry backoff",
            connector: Connector::Fail,
            timeout: operation_timeout(300),
            retry: Some(BoundedRetryStrategy::new(10, Some(Duration::from_secs(2)))),
            caller_abandons: false,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, PerAttempt, Once),
            // The attempt itself failed on the connector before the operation budget expired, so
            // the attempt pair sees the real error and only the execution pair sees the timeout.
            expected_observations: observations(Observed::ConnectorFailure, Observed::Timeout),
        },
        Scenario {
            // The negative control for the in-flight marker: the operation budget expires while
            // the initial request is still stalled, so no attempt ever starts. The attempt-level
            // finalizers must NOT fire -- `read_after_attempt` is owed only once
            // `read_before_attempt` has run -- while the execution-level pair still must. A fix
            // that settled the orphaned attempt unconditionally instead of consulting the marker
            // would fire them here and be caught.
            name: "operation timeout before any attempt starts",
            connector: Connector::Never,
            timeout: operation_timeout(200),
            retry: Some(BoundedRetryStrategy::stalling_before_first_attempt(
                Duration::from_secs(2),
            )),
            caller_abandons: false,
            attempts: 0,
            responses: 0,
            expected: row(Once, Never, Never, Never, Once),
            expected_observations: vec![
                ("modify_before_completion", Observed::Timeout),
                ("read_after_execution", Observed::Timeout),
            ],
        },
        Scenario {
            // A dropped future cannot run anything: `finally_attempt` and `finally_op` live inside
            // the `async move` block the caller just dropped, and Rust has no async drop. These
            // zeros are structural rather than a gap left to fix later -- the only way to change
            // them is to run the finalizers from `Drop`, which this design rejected. If this row
            // fails, that architectural choice changed.
            name: "caller abandons the future",
            connector: Connector::Never,
            timeout: None,
            retry: None,
            caller_abandons: true,
            attempts: 1,
            responses: 0,
            expected: row(Once, PerAttempt, Never, Never, Never),
            expected_observations: vec![],
        },
    ]
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

async fn run(scenario: &Scenario, path: DispatchPath) -> HookLog {
    let log = HookLog::default();
    let interceptor = match path {
        DispatchPath::Plain => SharedInterceptor::new(PlainRecorder(log.clone())),
        DispatchPath::Hinted => SharedInterceptor::new(HintedRecorder(log.clone())),
    };

    let mut components = RuntimeComponentsBuilder::new("matrix").with_interceptor(interceptor);
    components = match scenario.connector {
        Connector::Ok => components,
        Connector::Fail => {
            components.with_http_client(Some(http_client_fn(|_, _| FailingConnector.into_shared())))
        }
        Connector::FailThenOk => {
            let calls = Arc::new(AtomicUsize::new(0));
            components.with_http_client(Some(http_client_fn(move |_, _| {
                FailThenSucceedConnector(calls.clone()).into_shared()
            })))
        }
        Connector::Never => components.with_http_client(Some(NeverClient::new())),
    };
    if let Some(retry) = &scenario.retry {
        components =
            components.with_retry_strategy(Some(SharedRetryStrategy::new(BoundedRetryStrategy {
                max_attempts: retry.max_attempts,
                delay: retry.delay,
                initial_delay: retry.initial_delay,
            })));
    }

    let plugins = RuntimePlugins::new()
        .with_client_plugin(BaseRuntimePlugin::new())
        // Registered as operation plugins so they override the panicking fake sleep that
        // `RuntimeComponentsBuilder::for_tests()` installs.
        .with_operation_plugin(default_sleep_impl_plugin().expect("sleep impl available"))
        .with_operation_plugin(default_time_source_plugin().expect("time source available"))
        .with_operation_plugin(NoAuthRuntimePluginV2::new())
        .with_operation_plugin(ScenarioPlugin {
            components,
            timeout: scenario.timeout.clone(),
        });

    let operation = invoke_with_stop_point(
        "test",
        "test",
        Input::doesnt_matter(),
        &plugins,
        StopPoint::None,
    );

    if scenario.caller_abandons {
        // Drop the future mid-flight.
        let _ = tokio::time::timeout(Duration::from_millis(150), operation).await;
    } else {
        let _ = operation.await;
    }

    log
}

#[tokio::test]
async fn hook_expectation_matrix() {
    // Adding a hook to the runtime, or reordering the list, must force this matrix to be updated.
    assert_eq!(
        HOOKS,
        known_hooks!(),
        "hook list drifted from KNOWN_HOOKS; add the new hook to HOOKS and to every scenario row"
    );

    let position = |hook: &str| {
        HOOKS
            .iter()
            .position(|h| *h == hook)
            .unwrap_or_else(|| panic!("recorded unknown hook {hook}"))
    };
    let attempt_restart = position("read_before_attempt");

    let mut failures: Vec<String> = Vec::new();

    for scenario in &scenarios() {
        assert_eq!(
            scenario.expected.len(),
            HOOKS.len(),
            "{}: expectation row width does not match hook count",
            scenario.name
        );

        for path in [DispatchPath::Plain, DispatchPath::Hinted] {
            let log = run(scenario, path).await;
            let label = format!("{} [{:?}]", scenario.name, path);

            // The attempt count anchors `PerAttempt`, so verify it first -- otherwise a wrong
            // attempt count would silently satisfy the per-attempt rows.
            let started = log.count("read_before_attempt");
            if started != scenario.attempts {
                failures.push(format!(
                    "{label}: {started} attempts started, expected {}",
                    scenario.attempts
                ));
            }

            // 1. Counts.
            for (index, hook) in HOOKS.iter().enumerate() {
                let expected = match scenario.expected[index] {
                    Never => 0,
                    Once => 1,
                    PerAttempt => scenario.attempts,
                    PerResponse => scenario.responses,
                };
                let actual = log.count(hook);
                if actual != expected {
                    failures.push(format!(
                        "{label}: {hook} invoked {actual} times, expected {expected} ({:?})",
                        scenario.expected[index]
                    ));
                }
            }

            // 2. Error fidelity: a finalizer that fires but sees nothing cannot do its job.
            let observed = log.observed.lock().unwrap().clone();
            if observed != scenario.expected_observations {
                failures.push(format!(
                    "{label}: finalizers observed {observed:?}, expected {:?}",
                    scenario.expected_observations
                ));
            }

            // 3. Lifecycle ordering.
            for window in log.sequence.lock().unwrap().windows(2) {
                let (from, to) = (position(window[0]), position(window[1]));
                if !(to > from || to == attempt_restart) {
                    failures.push(format!(
                        "{label}: {} ran before {}, which is out of lifecycle order",
                        window[0], window[1]
                    ));
                }
            }

            // 4. Dispatch-path agreement is covered by both paths being held to the same
            //    expectations above.
        }
    }

    assert!(
        failures.is_empty(),
        "{} hook matrix mismatch(es):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
