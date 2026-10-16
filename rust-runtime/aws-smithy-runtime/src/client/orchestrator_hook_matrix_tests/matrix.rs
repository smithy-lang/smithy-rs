/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
//! The runner and the assertions.

use super::fixtures::{BaseRuntimePlugin, BoundedRetryStrategy, ScenarioPlugin, ScriptedConnector};
use super::recorder::{HookLog, PlainRecorder};
use super::scenarios::Expect::*;
use super::scenarios::{scenarios, Connector, Scenario, HOOKS};
use crate::client::auth::no_auth::NoAuthRuntimePluginV2;
use crate::client::defaults::{default_sleep_impl_plugin, default_time_source_plugin};
use crate::client::orchestrator::{invoke_with_stop_point, StopPoint};
use aws_smithy_http_client::test_util::NeverClient;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::http::http_client_fn;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeTransmitInterceptorContextRef, Input,
};
use aws_smithy_runtime_api::client::interceptors::{
    dyn_dispatch_hint, known_hooks, Intercept, SharedInterceptor,
};
use aws_smithy_runtime_api::client::retries::SharedRetryStrategy;
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponents, RuntimeComponentsBuilder,
};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugins;
use aws_smithy_runtime_api::shared::IntoShared;
use aws_smithy_types::config_bag::ConfigBag;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

async fn run(scenario: &Scenario) -> HookLog {
    let log = HookLog::default();
    let interceptor = SharedInterceptor::new(PlainRecorder(log.clone()));

    let mut components = RuntimeComponentsBuilder::new("matrix").with_interceptor(interceptor);
    components = match scenario.connector {
        Connector::Never => components.with_http_client(Some(NeverClient::new())),
        other => {
            let failures = match other {
                Connector::Ok => 0,
                Connector::FailThenOk => 1,
                _ => usize::MAX,
            };
            let calls = Arc::new(AtomicUsize::new(0));
            components.with_http_client(Some(http_client_fn(move |_, _| {
                ScriptedConnector {
                    calls: calls.clone(),
                    failures,
                }
                .into_shared()
            })))
        }
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

        {
            let log = run(scenario).await;
            let label = scenario.name;

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
        }
    }

    assert!(
        failures.is_empty(),
        "{} hook matrix mismatch(es):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// An interceptor that overrides exactly one hook must still have that hook dispatched.
///
/// Dispatch is gated on `overridden_hooks()`, and the matrix above cannot exercise that gate: its
/// recorder implements all 19 hooks, so the generated bitmask is all-ones and every flag check
/// passes regardless of which flag it consults. A sparse bitmask is the only way to catch a hook
/// wired to the wrong `OverriddenHooks` constant -- verified by mispairing `read_before_signing`
/// with `READ_AFTER_SIGNING`, which leaves this test's hook uninvoked while the matrix stays green.
#[tokio::test]
async fn one_overridden_hook_still_dispatches() {
    #[derive(Clone, Debug, Default)]
    struct SingleHookRecorder(HookLog);

    #[dyn_dispatch_hint]
    impl Intercept for SingleHookRecorder {
        fn name(&self) -> &'static str {
            "SingleHookRecorder"
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
    }

    let log = HookLog::default();
    let components = RuntimeComponentsBuilder::new("single_hook")
        .with_interceptor(SharedInterceptor::new(SingleHookRecorder(log.clone())));
    let plugins = RuntimePlugins::new()
        .with_client_plugin(BaseRuntimePlugin::new())
        .with_operation_plugin(default_sleep_impl_plugin().expect("sleep impl available"))
        .with_operation_plugin(default_time_source_plugin().expect("time source available"))
        .with_operation_plugin(NoAuthRuntimePluginV2::new())
        .with_operation_plugin(ScenarioPlugin {
            components,
            timeout: None,
        });

    let _ = invoke_with_stop_point(
        "test",
        "test",
        Input::doesnt_matter(),
        &plugins,
        StopPoint::None,
    )
    .await;

    assert_eq!(
        log.count("read_before_signing"),
        1,
        "the single overridden hook must be dispatched"
    );
    // Nothing else was overridden, so nothing else may run.
    assert_eq!(
        log.counts.lock().unwrap().len(),
        1,
        "only the overridden hook may be dispatched, saw {:?}",
        log.counts.lock().unwrap()
    );
}
