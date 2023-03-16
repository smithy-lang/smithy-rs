/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![warn(
    // missing_docs,
    // rustdoc::missing_crate_level_docs,
    unreachable_pub,
    rust_2018_idioms
)]

use crate::http::read_body;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::result::SdkError;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use aws_smithy_runtime_api::interceptors::context::{Error, Input, Output, OutputOrError};
use aws_smithy_runtime_api::interceptors::{InterceptorContext, Interceptors};
use aws_smithy_runtime_api::runtime_plugin::RuntimePlugins;
use phase::Phase;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use tracing::{debug_span, Instrument};

pub mod http;
mod phase;

pub type HttpRequest = ::http::Request<SdkBody>;
pub type HttpResponse = ::http::Response<SdkBody>;
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type BoxFallibleFut<T> = Pin<Box<dyn Future<Output = Result<T, BoxError>>>>;

pub trait TraceProbe: Send + Sync + Debug {
    fn dispatch_events(&self, cfg: &ConfigBag) -> BoxFallibleFut<()>;
}

pub trait RequestSerializer: Send + Sync + Debug {
    fn serialize_input(&self, input: &Input, cfg: &ConfigBag) -> Result<HttpRequest, BoxError>;
}

pub trait ResponseDeserializer: Send + Sync + Debug {
    fn deserialize_streaming(&self, response: &mut HttpResponse) -> Option<OutputOrError> {
        let _ = response;
        None
    }

    fn deserialize_nonstreaming(&self, response: &HttpResponse) -> OutputOrError;
}

pub trait Connection: Send + Sync + Debug {
    fn call(&self, request: &mut HttpRequest, cfg: &ConfigBag) -> BoxFallibleFut<HttpResponse>;
}

pub trait RetryStrategy: Send + Sync + Debug {
    fn should_attempt_initial_request(&self, cfg: &ConfigBag) -> Result<(), BoxError>;

    fn should_attempt_retry(
        &self,
        context: &InterceptorContext<HttpRequest, HttpResponse>,
        cfg: &ConfigBag,
    ) -> Result<bool, BoxError>;
}

pub trait AuthOrchestrator: Send + Sync + Debug {
    fn auth_request(&self, request: &mut HttpRequest, cfg: &ConfigBag) -> Result<(), BoxError>;
}

pub trait EndpointOrchestrator: Send + Sync + Debug {
    fn resolve_and_apply_endpoint(
        &self,
        request: &mut HttpRequest,
        cfg: &ConfigBag,
    ) -> Result<(), BoxError>;

    // TODO(jdisanti) The EP Orc and Auth Orc need to share info on auth schemes but I'm not sure how that should happen
    fn resolve_auth_schemes(&self) -> Result<Vec<String>, BoxError>;
}

trait ConfigBagAccessors {
    fn retry_strategy(&self) -> &dyn RetryStrategy;
    fn endpoint_orchestrator(&self) -> &dyn EndpointOrchestrator;
    fn auth_orchestrator(&self) -> &dyn AuthOrchestrator;
    fn connection(&self) -> &dyn Connection;
    fn request_serializer(&self) -> &dyn RequestSerializer;
    fn response_deserializer(&self) -> &dyn ResponseDeserializer;
    fn trace_probe(&self) -> &dyn TraceProbe;
}

impl ConfigBagAccessors for ConfigBag {
    fn retry_strategy(&self) -> &dyn RetryStrategy {
        &**self
            .get::<Box<dyn RetryStrategy>>()
            .expect("a retry strategy must be set")
    }

    fn endpoint_orchestrator(&self) -> &dyn EndpointOrchestrator {
        &**self
            .get::<Box<dyn EndpointOrchestrator>>()
            .expect("an endpoint orchestrator must be set")
    }

    fn auth_orchestrator(&self) -> &dyn AuthOrchestrator {
        &**self
            .get::<Box<dyn AuthOrchestrator>>()
            .expect("an auth orchestrator must be set")
    }

    fn connection(&self) -> &dyn Connection {
        &**self
            .get::<Box<dyn Connection>>()
            .expect("missing connector")
    }

    fn request_serializer(&self) -> &dyn RequestSerializer {
        &**self
            .get::<Box<dyn RequestSerializer>>()
            .expect("missing request serializer")
    }

    fn response_deserializer(&self) -> &dyn ResponseDeserializer {
        &**self
            .get::<Box<dyn ResponseDeserializer>>()
            .expect("missing response deserializer")
    }

    fn trace_probe(&self) -> &dyn TraceProbe {
        &**self
            .get::<Box<dyn TraceProbe>>()
            .expect("missing trace probe")
    }
}

pub async fn invoke(
    input: Input,
    interceptors: &mut Interceptors<HttpRequest, HttpResponse>,
    runtime_plugins: &RuntimePlugins,
    cfg: &mut ConfigBag,
) -> Result<Output, SdkError<Error, HttpResponse>> {
    let context = Phase::construction(InterceptorContext::new(input))
        // Client configuration
        .include(|_| runtime_plugins.apply_client_configuration(cfg))?
        .include(|ctx| interceptors.client_read_before_execution(ctx, cfg))?
        // Operation configuration
        .include(|_| runtime_plugins.apply_operation_configuration(cfg))?
        .include(|ctx| interceptors.operation_read_before_execution(ctx, cfg))?
        // Before serialization
        .include(|ctx| interceptors.read_before_serialization(ctx, cfg))?
        .include_mut(|ctx| interceptors.modify_before_serialization(ctx, cfg))?
        // Serialization
        .include_mut(|ctx| {
            let request_serializer = cfg.request_serializer();
            let request = request_serializer.serialize_input(ctx.input(), cfg)?;
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
            Ok(_) => {}
            // No, we shouldn't make a request because...
            Err(err) => return Err(Phase::dispatch(context).fail(err)),
        }
    }

    let mut context = context;
    let handling_phase = loop {
        let dispatch_phase = Phase::dispatch(context);
        context = make_an_attempt(dispatch_phase, cfg, interceptors)
            .await?
            .include(|ctx| interceptors.read_after_attempt(ctx, cfg))?
            .include_mut(|ctx| interceptors.modify_before_attempt_completion(ctx, cfg))?
            .finish();

        let retry_strategy = cfg.retry_strategy();
        match retry_strategy.should_attempt_retry(&context, cfg) {
            // Yes, let's retry the request
            Ok(true) => continue,
            // No, this request shouldn't be retried
            Ok(false) => {}
            // I couldn't determine if the request should be retried because an error occurred.
            Err(err) => {
                return Err(Phase::response_handling(context).fail(err));
            }
        }

        let handling_phase = Phase::response_handling(context)
            .include_mut(|ctx| interceptors.modify_before_completion(ctx, cfg))?;
        let trace_probe = cfg.trace_probe();
        trace_probe.dispatch_events(cfg);

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
    interceptors: &mut Interceptors<HttpRequest, HttpResponse>,
) -> Result<Phase, SdkError<Error, HttpResponse>> {
    let mut context = dispatch_phase
        .include(|ctx| interceptors.read_before_attempt(ctx, cfg))?
        .include_mut(|ctx| {
            let tx_req_mut = ctx.request_mut().expect("request has been set");

            let endpoint_orchestrator = cfg.endpoint_orchestrator();
            endpoint_orchestrator.resolve_and_apply_endpoint(tx_req_mut, cfg)
        })?
        .include_mut(|ctx| interceptors.modify_before_signing(ctx, cfg))?
        .include(|ctx| interceptors.read_before_signing(ctx, cfg))?
        .include_mut(|ctx| {
            let tx_req_mut = ctx.request_mut().expect("request has been set");
            let auth_orchestrator = cfg.auth_orchestrator();
            auth_orchestrator.auth_request(tx_req_mut, cfg)
        })?
        .include(|ctx| interceptors.read_after_signing(ctx, cfg))?
        .include_mut(|ctx| interceptors.modify_before_transmit(ctx, cfg))?
        .include(|ctx| interceptors.read_before_transmit(ctx, cfg))?
        .finish();

    // The connection consumes the request but we need to keep a copy of it
    // within the interceptor context, so we clone it here.
    let call_result = {
        let tx_req = context.request_mut().expect("request has been set");
        let connection = cfg.connection();
        connection.call(tx_req, cfg).await
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
                .and_then(|_| Ok(response_deserializer.deserialize_nonstreaming(response))),
        }
    };

    Phase::response_handling(context)
        .include_mut(move |ctx| {
            ctx.set_output_or_error(output_or_error?);
            Result::<(), BoxError>::Ok(())
        })?
        .include(|ctx| interceptors.read_after_deserialization(ctx, cfg))
}
