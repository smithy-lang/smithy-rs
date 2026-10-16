/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
//! Connectors, retry strategy and runtime plugins the scenarios are built from.

use crate::client::auth::no_auth::NO_AUTH_SCHEME_ID;
use crate::client::orchestrator::endpoints::StaticUriEndpointResolver;
use crate::client::retries::strategy::NeverRetryStrategy;
use crate::client::test_util::{
    deserializer::CannedResponseDeserializer, serializer::CannedRequestSerializer,
};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::auth::static_resolver::StaticAuthSchemeOptionResolver;
use aws_smithy_runtime_api::client::auth::{
    AuthSchemeOptionResolverParams, SharedAuthSchemeOptionResolver,
};
use aws_smithy_runtime_api::client::endpoint::{EndpointResolverParams, SharedEndpointResolver};
use aws_smithy_runtime_api::client::http::{http_client_fn, HttpConnector, HttpConnectorFuture};
use aws_smithy_runtime_api::client::interceptors::context::{InterceptorContext, Output};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, OrchestratorError};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_runtime_api::client::retries::{
    RequestAttempts, RetryStrategy, SharedRetryStrategy, ShouldAttempt,
};
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponents, RuntimeComponentsBuilder,
};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::client::ser_de::{SharedRequestSerializer, SharedResponseDeserializer};
use aws_smithy_runtime_api::shared::IntoShared;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::{ConfigBag, FrozenLayer, Layer};
use aws_smithy_types::timeout::TimeoutConfig;
use http_1x::{Response, StatusCode};
use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Responds 200, except for the first `failures` calls which fail at the transport layer.
///
/// `failures: 0` always succeeds, `usize::MAX` always fails, and `1` makes `PerAttempt` and
/// `PerResponse` diverge on a retry.
#[derive(Debug)]
pub(super) struct ScriptedConnector {
    pub(super) calls: Arc<AtomicUsize>,
    pub(super) failures: usize,
}

impl HttpConnector for ScriptedConnector {
    fn call(&self, _request: HttpRequest) -> HttpConnectorFuture {
        if self.calls.fetch_add(1, Ordering::SeqCst) < self.failures {
            return HttpConnectorFuture::ready(Err(ConnectorError::io("connector failed".into())));
        }
        HttpConnectorFuture::ready(Ok(http_1x::Response::builder()
            .status(200)
            .body(SdkBody::empty())
            .expect("valid response")
            .try_into()
            .unwrap()))
    }
}

/// Retries up to `max_attempts`, optionally asking for a delay so that an operation timeout can
/// land during backoff rather than mid-attempt.
///
/// `initial_delay` additionally stalls the *initial* request. The orchestrator awaits that delay
/// after the prologue but before `read_before_attempt`, which is how a timeout can be made to land
/// with no attempt ever started.
#[derive(Debug)]
pub(super) struct BoundedRetryStrategy {
    pub(super) max_attempts: u32,
    pub(super) delay: Option<Duration>,
    pub(super) initial_delay: Option<Duration>,
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
pub(super) struct BaseRuntimePlugin(pub(super) RuntimeComponentsBuilder);

impl BaseRuntimePlugin {
    pub(super) fn new() -> Self {
        Self(
            RuntimeComponentsBuilder::for_tests()
                // Scenarios override these via `ScenarioPlugin` where they need something else.
                .with_retry_strategy(Some(SharedRetryStrategy::new(NeverRetryStrategy::new())))
                .with_http_client(Some(http_client_fn(|_, _| {
                    ScriptedConnector {
                        calls: Arc::new(AtomicUsize::new(0)),
                        failures: 0,
                    }
                    .into_shared()
                })))
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
pub(super) struct ScenarioPlugin {
    pub(super) components: RuntimeComponentsBuilder,
    pub(super) timeout: Option<TimeoutConfig>,
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
