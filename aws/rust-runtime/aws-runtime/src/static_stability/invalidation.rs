/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Auth-failure detection for static-stability credential invalidation.

use aws_smithy_runtime::client::orchestrator::InvalidateResolvedIdentity;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::AfterDeserializationInterceptorContextRef;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::error::metadata::ProvideErrorMetadata;
use std::error::Error as StdError;
use std::fmt;
use std::marker::PhantomData;

/// Detects a credential/token auth failure (`ExpiredToken` / `InvalidToken`) on the operation
/// response and signals the orchestrator — via the data-free [`InvalidateResolvedIdentity`] config
/// marker — to invalidate the resolved identity.
///
/// This is **detection only**: the interceptor has no identity (the signed request is already gone
/// post-transmit), so the orchestrator makes the actual `invalidate` call with the in-scope signing
/// identity. Registered per-operation by AWS codegen (like `AwsErrorCodeClassifier<E>`).
pub struct CredentialAuthFailureInterceptor<E> {
    _marker: PhantomData<fn() -> E>,
}

impl<E> CredentialAuthFailureInterceptor<E> {
    /// Creates a new [`CredentialAuthFailureInterceptor`].
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<E> Default for CredentialAuthFailureInterceptor<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E> fmt::Debug for CredentialAuthFailureInterceptor<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CredentialAuthFailureInterceptor")
    }
}

impl<E> Intercept for CredentialAuthFailureInterceptor<E>
where
    E: StdError + ProvideErrorMetadata + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        "CredentialAuthFailure"
    }

    fn read_after_deserialization(
        &self,
        context: &AfterDeserializationInterceptorContextRef<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let is_auth_failure = context
            .output_or_error()
            .err()
            .and_then(|err| err.as_operation_error())
            .and_then(|err| err.downcast_ref::<E>())
            .and_then(|err| err.code())
            // NOT AccessDenied — that's authorization, not credential validity.
            .is_some_and(|code| matches!(code, "ExpiredToken" | "InvalidToken"));
        if is_auth_failure {
            cfg.interceptor_state()
                .store_put(InvalidateResolvedIdentity);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_runtime_api::client::interceptors::context::{Error, Input, InterceptorContext};
    use aws_smithy_runtime_api::client::orchestrator::OrchestratorError;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
    use aws_smithy_types::error::ErrorMetadata;

    // A minimal operation error carrying a modeled error code.
    #[derive(Debug)]
    struct CodedError {
        metadata: ErrorMetadata,
    }

    impl CodedError {
        fn new(code: &'static str) -> Self {
            Self {
                metadata: ErrorMetadata::builder().code(code).build(),
            }
        }
    }

    impl fmt::Display for CodedError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "coded error")
        }
    }

    impl StdError for CodedError {}

    impl ProvideErrorMetadata for CodedError {
        fn meta(&self) -> &ErrorMetadata {
            &self.metadata
        }
    }

    // Runs the interceptor against a deserialized operation error with the given code and reports
    // whether it set the `InvalidateResolvedIdentity` marker.
    fn sets_invalidate_marker(code: &'static str) -> bool {
        let interceptor = CredentialAuthFailureInterceptor::<CodedError>::new();
        let rc = RuntimeComponentsBuilder::for_tests().build().unwrap();
        let mut cfg = ConfigBag::base();
        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.set_output_or_error(Err(OrchestratorError::operation(Error::erase(
            CodedError::new(code),
        ))));
        let ctx_ref = AfterDeserializationInterceptorContextRef::from(&ctx);
        interceptor
            .read_after_deserialization(&ctx_ref, &rc, &mut cfg)
            .unwrap();
        cfg.load::<InvalidateResolvedIdentity>().is_some()
    }

    #[test]
    fn triggers_on_expired_and_invalid_token_only() {
        assert!(
            sets_invalidate_marker("ExpiredToken"),
            "ExpiredToken must trigger invalidation"
        );
        assert!(
            sets_invalidate_marker("InvalidToken"),
            "InvalidToken must trigger invalidation"
        );
        // AccessDenied is authorization, not credential validity — must NOT invalidate.
        assert!(
            !sets_invalidate_marker("AccessDenied"),
            "AccessDenied is authz, not credential validity"
        );
        assert!(
            !sets_invalidate_marker("ThrottlingException"),
            "unrelated errors must not trigger invalidation"
        );
    }
}
