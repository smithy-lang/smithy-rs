/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use self::auth::orchestrate_auth;
use self::auth::orchestrate_auth;
use crate::client::orchestrator::endpoints::orchestrate_endpoint;
use crate::client::orchestrator::endpoints::orchestrate_endpoint;
use crate::client::orchestrator::http::read_body;
use crate::client::orchestrator::http::read_body;
use crate::client::orchestrator::phase::Phase;
use crate::client::orchestrator::phase::Phase;
use crate::client::timeout::{MaybeTimeout, ProvideMaybeTimeoutConfig, TimeoutKind};
use aws_smithy_http::result::SdkError;
use aws_smithy_runtime_api::client::interceptors::context::{Error, Input, Output};
use aws_smithy_runtime_api::client::interceptors::{InterceptorContext, Interceptors};
use aws_smithy_runtime_api::client::orchestrator::{BoxError, ConfigBagAccessors, HttpResponse};
use aws_smithy_runtime_api::client::retries::ShouldAttempt;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugins;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use tracing::{debug_span, Instrument};

mod auth;
/// Defines types that implement a trait for endpoint resolution
pub mod endpoints;
mod http;
pub(self) mod phase;

pub async fn invoke(
    input: Input,
    runtime_plugins: &RuntimePlugins,
) -> Result<Output, SdkError<Error, HttpResponse>> {
    invoke_pre_config(input, runtime_plugins)
        .instrument(debug_span!("invoke"))
        .await
}

async fn invoke_pre_config(
    input: Input,
    runtime_plugins: &RuntimePlugins,
) -> Result<Output, SdkError<Error, HttpResponse>> {
    let mut cfg = ConfigBag::base();
    let cfg = &mut cfg;

    let mut interceptors = Interceptors::new();

    let context = Phase::construction(InterceptorContext::new(input))
        // Client configuration
        .include(|_| runtime_plugins.apply_client_configuration(cfg, &mut interceptors))?
        .include(|ctx| interceptors.client_read_before_execution(ctx, cfg))?
        // Operation configuration
        .include(|_| runtime_plugins.apply_operation_configuration(cfg, &mut interceptors))?
        .include(|ctx| interceptors.operation_read_before_execution(ctx, cfg))?
        .finish();

    let operation_timeout_config = cfg.maybe_timeout_config(TimeoutKind::Operation);
    invoke_post_config(cfg, context, interceptors)
        .maybe_timeout_with_config(operation_timeout_config)
        .await
}

async fn invoke_post_config(
    cfg: &mut ConfigBag,
    context: InterceptorContext,
    interceptors: Interceptors,
) -> Result<Output, SdkError<Error, HttpResponse>> {
    let context = Phase::construction(context)
        // Before serialization
        .include(|ctx| interceptors.read_before_serialization(ctx, cfg))?
        .include_mut(|ctx| interceptors.modify_before_serialization(ctx, cfg))?
        // Serialization
        .include_mut(|ctx| {
            let request_serializer = cfg.request_serializer();
            let request = request_serializer
                .serialize_input(ctx.take_input().expect("input set at this point"))?;
            ctx.set_request(request);
            Result::<(), BoxError>::Ok(())
        })?
        // After serialization
        .include(|ctx| interceptors.read_after_serialization(ctx, cfg))?
        // Before retry loop
        .include_mut(|ctx| interceptors.modify_before_retry_loop(ctx, cfg))?
        .finish();

    {
        let retry_strategy = cfg.retry_strategy();
        match retry_strategy.should_attempt_initial_request(cfg) {
            // Yes, let's make a request
            Ok(ShouldAttempt::Yes) => {}
            // No, this request shouldn't be sent
            Ok(ShouldAttempt::No) => {
                return Err(Phase::dispatch(context).fail(
                    "The retry strategy indicates that an initial request shouldn't be made, but it didn't specify why.",
                ));
            }
            // No, we shouldn't make a request because...
            Err(err) => return Err(Phase::dispatch(context).fail(err)),
            Ok(ShouldAttempt::YesAfterDelay(_)) => {
                unreachable!("Delaying the initial request is currently unsupported. If this feature is important to you, please file an issue in GitHub.")
            }
        }
    }

    let mut context = context;
    let handling_phase = loop {
        let attempt_timeout_config = cfg.maybe_timeout_config(TimeoutKind::OperationAttempt);
        let dispatch_phase = Phase::dispatch(context);
        context = make_an_attempt(dispatch_phase, cfg, &interceptors)
            .instrument(debug_span!("make_an_attempt"))
            .maybe_timeout_with_config(attempt_timeout_config)
            .await?
            .include(|ctx| interceptors.read_after_attempt(ctx, cfg))?
            .include_mut(|ctx| interceptors.modify_before_attempt_completion(ctx, cfg))?
            .finish();

        let retry_strategy = cfg.retry_strategy();
        match retry_strategy.should_attempt_retry(&context, cfg) {
            // Yes, let's retry the request
            Ok(ShouldAttempt::Yes) => continue,
            // No, this request shouldn't be retried
            Ok(ShouldAttempt::No) => {}
            Ok(ShouldAttempt::YesAfterDelay(_delay)) => {
                todo!("implement retries with an explicit delay.")
            }
            // I couldn't determine if the request should be retried because an error occurred.
            Err(err) => {
                return Err(Phase::response_handling(context).fail(err));
            }
        }

        let handling_phase = Phase::response_handling(context)
            .include_mut(|ctx| interceptors.modify_before_completion(ctx, cfg))?;
        cfg.trace_probe().dispatch_events();

        break handling_phase.include(|ctx| interceptors.read_after_execution(ctx, cfg))?;
    };

    handling_phase.finalize()
}

// Making an HTTP request can fail for several reasons, but we still need to
// call lifecycle events when that happens. Therefore, we define this
// `make_an_attempt` function to make error handling simpler.
async fn make_an_attempt(
    dispatch_phase: Phase,
    cfg: &mut ConfigBag,
    interceptors: &Interceptors,
) -> Result<Phase, SdkError<Error, HttpResponse>> {
    let dispatch_phase = dispatch_phase
        .include(|ctx| interceptors.read_before_attempt(ctx, cfg))?
        .include_mut(|ctx| orchestrate_endpoint(ctx, cfg))?
        .include_mut(|ctx| interceptors.modify_before_signing(ctx, cfg))?
        .include(|ctx| interceptors.read_before_signing(ctx, cfg))?;

    let dispatch_phase = orchestrate_auth(dispatch_phase, cfg).await?;

    let mut context = dispatch_phase
        .include(|ctx| interceptors.read_after_signing(ctx, cfg))?
        .include_mut(|ctx| interceptors.modify_before_transmit(ctx, cfg))?
        .include(|ctx| interceptors.read_before_transmit(ctx, cfg))?
        .finish();

    // The connection consumes the request but we need to keep a copy of it
    // within the interceptor context, so we clone it here.
    let call_result = {
        let request = context.take_request().expect("request has been set");
        let connection = cfg.connection();
        connection.call(request).await
    };

    let mut context = Phase::dispatch(context)
        .include_mut(move |ctx| {
            ctx.set_response(call_result?);
            Result::<(), BoxError>::Ok(())
        })?
        .include(|ctx| interceptors.read_after_transmit(ctx, cfg))?
        .include_mut(|ctx| interceptors.modify_before_deserialization(ctx, cfg))?
        .include(|ctx| interceptors.read_before_deserialization(ctx, cfg))?
        .finish();

    let output_or_error = {
        let response = context.response_mut().expect("response has been set");
        let response_deserializer = cfg.response_deserializer();
        match response_deserializer.deserialize_streaming(response) {
            Some(output_or_error) => Ok(output_or_error),
            None => read_body(response)
                .instrument(debug_span!("read_body"))
                .await
                .map(|_| response_deserializer.deserialize_nonstreaming(response)),
        }
    };

    Phase::response_handling(context)
        .include_mut(move |ctx| {
            ctx.set_output_or_error(output_or_error?);
            Result::<(), BoxError>::Ok(())
        })?
        .include(|ctx| interceptors.read_after_deserialization(ctx, cfg))
}

#[cfg(all(test, feature = "test-util"))]
mod tests {
    use super::invoke;
    use crate::client::orchestrator::endpoints::StaticUriEndpointResolver;
    use crate::client::retries::strategy::NeverRetryStrategy;
    use crate::client::test_util::auth::{
        http_auth_schemes_for_testing, identity_resolvers_for_testing,
        EmptyAuthOptionResolverParams, ANONYMOUS_AUTH_SCHEME_ID,
    };
    use crate::client::test_util::{
        connector::OkConnector, deserializer::CannedResponseDeserializer,
        endpoints::EmptyEndpointResolverParams, serializer::CannedRequestSerializer,
        trace_probe::NoOpTraceProbe,
    };
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_runtime_api::client::auth::option_resolver::StaticAuthOptionResolver;
    use aws_smithy_runtime_api::client::interceptors::{
        Interceptor, InterceptorContext, Interceptors,
    };
    use aws_smithy_runtime_api::client::orchestrator::ConfigBagAccessors;
    use aws_smithy_runtime_api::client::runtime_plugin::{BoxError, RuntimePlugin, RuntimePlugins};
    use aws_smithy_runtime_api::config_bag::ConfigBag;
    use aws_smithy_runtime_api::type_erasure::TypeErasedBox;
    use http::StatusCode;
    use std::sync::Arc;
    use tracing_test::traced_test;

    fn new_request_serializer() -> CannedRequestSerializer {
        CannedRequestSerializer::success(
            http::Request::builder()
                .body(SdkBody::empty())
                .expect("request is valid"),
        )
    }

    fn new_response_deserializer() -> CannedResponseDeserializer {
        CannedResponseDeserializer::new(
            http::Response::builder()
                .status(StatusCode::OK)
                .body(SdkBody::empty())
                .map_err(|err| TypeErasedBox::new(Box::new(err)))
                .map(|res| TypeErasedBox::new(Box::new(res))),
        )
    }

    struct TestOperationRuntimePlugin;

    impl RuntimePlugin for TestOperationRuntimePlugin {
        fn configure(
            &self,
            cfg: &mut ConfigBag,
            _interceptors: &mut Interceptors,
        ) -> Result<(), BoxError> {
            cfg.set_request_serializer(new_request_serializer());
            cfg.set_response_deserializer(new_response_deserializer());
            cfg.set_retry_strategy(NeverRetryStrategy::new());
            cfg.set_endpoint_resolver(StaticUriEndpointResolver::http_localhost(8080));
            cfg.set_endpoint_resolver_params(EmptyEndpointResolverParams::new().into());
            cfg.set_auth_option_resolver_params(EmptyAuthOptionResolverParams::new().into());
            cfg.set_auth_option_resolver(StaticAuthOptionResolver::new(vec![
                ANONYMOUS_AUTH_SCHEME_ID,
            ]));
            cfg.set_identity_resolvers(identity_resolvers_for_testing());
            cfg.set_http_auth_schemes(http_auth_schemes_for_testing());
            cfg.set_connection(OkConnector::new());
            cfg.set_trace_probe(NoOpTraceProbe::new());

            Ok(())
        }
    }

    macro_rules! interceptor_error_handling_test {
        ($interceptor:ident, $ctx:ty, $expected:expr) => {
            #[derive(Debug)]
            struct FailingInterceptorA;
            impl Interceptor for FailingInterceptorA {
                fn $interceptor(&self, _ctx: $ctx, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
                    tracing::debug!("FailingInterceptorA called!");
                    Err("FailingInterceptorA".into())
                }
            }

            #[derive(Debug)]
            struct FailingInterceptorB;
            impl Interceptor for FailingInterceptorB {
                fn $interceptor(&self, _ctx: $ctx, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
                    tracing::debug!("FailingInterceptorB called!");
                    Err("FailingInterceptorB".into())
                }
            }

            #[derive(Debug)]
            struct FailingInterceptorC;
            impl Interceptor for FailingInterceptorC {
                fn $interceptor(&self, _ctx: $ctx, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
                    tracing::debug!("FailingInterceptorC called!");
                    Err("FailingInterceptorC".into())
                }
            }

            struct FailingInterceptorsOperationRuntimePlugin;

            impl RuntimePlugin for FailingInterceptorsOperationRuntimePlugin {
                fn configure(
                    &self,
                    _cfg: &mut ConfigBag,
                    interceptors: &mut Interceptors,
                ) -> Result<(), BoxError> {
                    interceptors.register_client_interceptor(Arc::new(FailingInterceptorA));
                    interceptors.register_operation_interceptor(Arc::new(FailingInterceptorB));
                    interceptors.register_operation_interceptor(Arc::new(FailingInterceptorC));

                    Ok(())
                }
            }

            let input = TypeErasedBox::new(Box::new(()));
            let runtime_plugins = RuntimePlugins::new()
                .with_operation_plugin(TestOperationRuntimePlugin)
                .with_operation_plugin(FailingInterceptorsOperationRuntimePlugin);
            let actual = invoke(input, &runtime_plugins)
                .await
                .expect_err("should error");
            let actual = format!("{:?}", actual);
            assert_eq!($expected, format!("{:?}", actual));

            assert!(logs_contain("FailingInterceptorA called!"));
            assert!(logs_contain("FailingInterceptorB called!"));
            assert!(logs_contain("FailingInterceptorC called!"));
        };
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_before_execution_error_handling() {
        let expected = r#""ConstructionFailure(ConstructionFailure { source: InterceptorError { kind: ReadBeforeExecution, source: Some(\"FailingInterceptorC\") } })""#.to_string();
        interceptor_error_handling_test!(read_before_execution, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_modify_before_serialization_error_handling() {
        let expected = r#""ConstructionFailure(ConstructionFailure { source: InterceptorError { kind: ModifyBeforeSerialization, source: Some(\"FailingInterceptorC\") } })""#.to_string();
        interceptor_error_handling_test!(
            modify_before_serialization,
            &mut InterceptorContext,
            expected
        );
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_before_serialization_error_handling() {
        let expected = r#""ConstructionFailure(ConstructionFailure { source: InterceptorError { kind: ReadBeforeSerialization, source: Some(\"FailingInterceptorC\") } })""#.to_string();
        interceptor_error_handling_test!(read_before_serialization, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_after_serialization_error_handling() {
        let expected = r#""ConstructionFailure(ConstructionFailure { source: InterceptorError { kind: ReadAfterSerialization, source: Some(\"FailingInterceptorC\") } })""#.to_string();
        interceptor_error_handling_test!(read_after_serialization, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_modify_before_retry_loop_error_handling() {
        let expected = r#""ConstructionFailure(ConstructionFailure { source: InterceptorError { kind: ModifyBeforeRetryLoop, source: Some(\"FailingInterceptorC\") } })""#.to_string();
        interceptor_error_handling_test!(
            modify_before_retry_loop,
            &mut InterceptorContext,
            expected
        );
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_before_attempt_error_handling() {
        let expected = r#""DispatchFailure(DispatchFailure { source: ConnectorError { kind: Other(None), source: InterceptorError { kind: ReadBeforeAttempt, source: Some(\"FailingInterceptorC\") }, connection: Unknown } })""#.to_string();
        interceptor_error_handling_test!(read_before_attempt, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_modify_before_signing_error_handling() {
        let expected = r#""DispatchFailure(DispatchFailure { source: ConnectorError { kind: Other(None), source: InterceptorError { kind: ModifyBeforeSigning, source: Some(\"FailingInterceptorC\") }, connection: Unknown } })""#.to_string();
        interceptor_error_handling_test!(modify_before_signing, &mut InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_before_signing_error_handling() {
        let expected = r#""DispatchFailure(DispatchFailure { source: ConnectorError { kind: Other(None), source: InterceptorError { kind: ReadBeforeSigning, source: Some(\"FailingInterceptorC\") }, connection: Unknown } })""#.to_string();
        interceptor_error_handling_test!(read_before_signing, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_after_signing_error_handling() {
        let expected = r#""DispatchFailure(DispatchFailure { source: ConnectorError { kind: Other(None), source: InterceptorError { kind: ReadAfterSigning, source: Some(\"FailingInterceptorC\") }, connection: Unknown } })""#.to_string();
        interceptor_error_handling_test!(read_after_signing, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_modify_before_transmit_error_handling() {
        let expected = r#""DispatchFailure(DispatchFailure { source: ConnectorError { kind: Other(None), source: InterceptorError { kind: ModifyBeforeTransmit, source: Some(\"FailingInterceptorC\") }, connection: Unknown } })""#.to_string();
        interceptor_error_handling_test!(modify_before_transmit, &mut InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_before_transmit_error_handling() {
        let expected = r#""DispatchFailure(DispatchFailure { source: ConnectorError { kind: Other(None), source: InterceptorError { kind: ReadBeforeTransmit, source: Some(\"FailingInterceptorC\") }, connection: Unknown } })""#.to_string();
        interceptor_error_handling_test!(read_before_transmit, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_after_transmit_error_handling() {
        let expected = r#""ResponseError(ResponseError { source: InterceptorError { kind: ReadAfterTransmit, source: Some(\"FailingInterceptorC\") }, raw: Response { status: 200, version: HTTP/1.1, headers: {}, body: SdkBody { inner: Once(None), retryable: true } } })""#.to_string();
        interceptor_error_handling_test!(read_after_transmit, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_modify_before_deserialization_error_handling() {
        let expected = r#""ResponseError(ResponseError { source: InterceptorError { kind: ModifyBeforeDeserialization, source: Some(\"FailingInterceptorC\") }, raw: Response { status: 200, version: HTTP/1.1, headers: {}, body: SdkBody { inner: Once(None), retryable: true } } })""#.to_string();
        interceptor_error_handling_test!(
            modify_before_deserialization,
            &mut InterceptorContext,
            expected
        );
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_before_deserialization_error_handling() {
        let expected = r#""ResponseError(ResponseError { source: InterceptorError { kind: ReadBeforeDeserialization, source: Some(\"FailingInterceptorC\") }, raw: Response { status: 200, version: HTTP/1.1, headers: {}, body: SdkBody { inner: Once(None), retryable: true } } })""#.to_string();
        interceptor_error_handling_test!(
            read_before_deserialization,
            &InterceptorContext,
            expected
        );
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_after_deserialization_error_handling() {
        let expected = r#""ResponseError(ResponseError { source: InterceptorError { kind: ReadAfterDeserialization, source: Some(\"FailingInterceptorC\") }, raw: Response { status: 200, version: HTTP/1.1, headers: {}, body: SdkBody { inner: Once(Some(b\"\")), retryable: true } } })""#.to_string();
        interceptor_error_handling_test!(read_after_deserialization, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_modify_before_attempt_completion_error_handling() {
        let expected = r#""ResponseError(ResponseError { source: InterceptorError { kind: ModifyBeforeAttemptCompletion, source: Some(\"FailingInterceptorC\") }, raw: Response { status: 200, version: HTTP/1.1, headers: {}, body: SdkBody { inner: Once(Some(b\"\")), retryable: true } } })""#.to_string();
        interceptor_error_handling_test!(
            modify_before_attempt_completion,
            &mut InterceptorContext,
            expected
        );
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_after_attempt_error_handling() {
        let expected = r#""ResponseError(ResponseError { source: InterceptorError { kind: ReadAfterAttempt, source: Some(\"FailingInterceptorC\") }, raw: Response { status: 200, version: HTTP/1.1, headers: {}, body: SdkBody { inner: Once(Some(b\"\")), retryable: true } } })""#.to_string();
        interceptor_error_handling_test!(read_after_attempt, &InterceptorContext, expected);
    }

    #[tokio::test]
    #[traced_test]
    async fn test_modify_before_completion_error_handling() {
        let expected = r#""ResponseError(ResponseError { source: InterceptorError { kind: ModifyBeforeCompletion, source: Some(\"FailingInterceptorC\") }, raw: Response { status: 200, version: HTTP/1.1, headers: {}, body: SdkBody { inner: Once(Some(b\"\")), retryable: true } } })""#.to_string();
        interceptor_error_handling_test!(
            modify_before_completion,
            &mut InterceptorContext,
            expected
        );
    }

    #[tokio::test]
    #[traced_test]
    async fn test_read_after_execution_error_handling() {
        let expected = r#""ResponseError(ResponseError { source: InterceptorError { kind: ReadAfterExecution, source: Some(\"FailingInterceptorC\") }, raw: Response { status: 200, version: HTTP/1.1, headers: {}, body: SdkBody { inner: Once(Some(b\"\")), retryable: true } } })""#.to_string();
        interceptor_error_handling_test!(read_after_execution, &InterceptorContext, expected);
    }
}
