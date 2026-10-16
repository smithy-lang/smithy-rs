/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
//! Recording interceptor used by the matrix, and the vocabulary it records into.

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::{
    AfterDeserializationInterceptorContextRef, BeforeDeserializationInterceptorContextMut,
    BeforeDeserializationInterceptorContextRef, BeforeSerializationInterceptorContextMut,
    BeforeSerializationInterceptorContextRef, BeforeTransmitInterceptorContextMut,
    BeforeTransmitInterceptorContextRef, FinalizerInterceptorContextMut,
    FinalizerInterceptorContextRef,
};
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::orchestrator::OrchestratorError;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// What a finalizer hook saw in `output_or_error` when it ran.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Observed {
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
    pub(super) fn of<O, E>(outcome: Option<Result<&O, &OrchestratorError<E>>>) -> Self {
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
pub(super) struct HookLog {
    pub(super) counts: Arc<Mutex<BTreeMap<&'static str, usize>>>,
    /// Invocation sequence, for the ordering assertion.
    pub(super) sequence: Arc<Mutex<Vec<&'static str>>>,
    /// What each finalizer hook observed, in invocation order.
    pub(super) observed: Arc<Mutex<Vec<(&'static str, Observed)>>>,
}

impl HookLog {
    pub(super) fn record(&self, hook: &'static str) {
        *self.counts.lock().unwrap().entry(hook).or_insert(0) += 1;
        self.sequence.lock().unwrap().push(hook);
    }

    pub(super) fn record_finalizer(&self, hook: &'static str, observed: Observed) {
        self.record(hook);
        self.observed.lock().unwrap().push((hook, observed));
    }

    pub(super) fn count(&self, hook: &str) -> usize {
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
    // Finalizers additionally record what they observed in `output_or_error`.
    (finalizer $name:ident, $ctx:ident) => {
        fn $name(
            &self,
            ctx: &$ctx<'_>,
            _components: &RuntimeComponents,
            _cfg: &mut ConfigBag,
        ) -> Result<(), BoxError> {
            self.0
                .record_finalizer(stringify!($name), Observed::of(ctx.output_or_error()));
            Ok(())
        }
    };
    (finalizer mut $name:ident, $ctx:ident) => {
        fn $name(
            &self,
            ctx: &mut $ctx<'_>,
            _components: &RuntimeComponents,
            _cfg: &mut ConfigBag,
        ) -> Result<(), BoxError> {
            self.0
                .record_finalizer(stringify!($name), Observed::of(ctx.output_or_error()));
            Ok(())
        }
    };
}

/// Recorder exercising the unconditional dispatch path.
///
/// `overridden_hooks()` is left to its default of `all()`, so every hook is dispatched without
/// consulting a bitmask.
#[derive(Clone, Debug, Default)]
pub(super) struct PlainRecorder(pub(super) HookLog);

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

    recording_hook!(finalizer mut modify_before_attempt_completion, FinalizerInterceptorContextMut);
    recording_hook!(finalizer read_after_attempt, FinalizerInterceptorContextRef);
    recording_hook!(finalizer mut modify_before_completion, FinalizerInterceptorContextMut);
    recording_hook!(finalizer read_after_execution, FinalizerInterceptorContextRef);
}
