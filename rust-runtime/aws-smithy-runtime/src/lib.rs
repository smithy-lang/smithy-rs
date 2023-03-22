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

use aws_smithy_http::result::SdkError;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use aws_smithy_runtime_api::interceptors::context::{Error, Input, Output, OutputOrError};
use aws_smithy_runtime_api::interceptors::{InterceptorContext, Interceptors};
use aws_smithy_runtime_api::runtime_plugin::RuntimePlugins;
use phase::Phase;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;

mod phase;

pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type BoxFallibleFut<T> = Pin<Box<dyn Future<Output = Result<T, BoxError>>>>;

pub trait TraceProbe: Send + Sync + Debug {
    fn dispatch_events(&self, cfg: &ConfigBag) -> BoxFallibleFut<()>;
}

pub trait RequestSerializer<TxReq>: Send + Sync + Debug {
    fn serialize_input(&self, input: &Input, cfg: &ConfigBag) -> Result<TxReq, BoxError>;
}

pub trait ResponseDeserializer<TxRes>: Send + Sync + Debug {
    fn deserialize_output(
        &self,
        response: &mut TxRes,
        cfg: &ConfigBag,
    ) -> Result<OutputOrError, BoxError>;
}

pub trait Connection<TxReq, TxRes>: Send + Sync + Debug {
    fn call(&self, req: &mut TxReq, cfg: &ConfigBag) -> BoxFallibleFut<TxRes>;
}

pub trait RetryStrategy<Request, Response>: Send + Sync + Debug {
    fn should_attempt_initial_request(&self, cfg: &ConfigBag) -> Result<(), BoxError>;

    fn should_attempt_retry(
        &self,
        context: &InterceptorContext<Request, Response>,
        cfg: &ConfigBag,
    ) -> Result<bool, BoxError>;
}

pub trait AuthOrchestrator<Req>: Send + Sync + Debug {
    fn auth_request(&self, req: &mut Req, cfg: &ConfigBag) -> Result<(), BoxError>;
}

pub trait EndpointOrchestrator<Req>: Send + Sync + Debug {
    fn resolve_and_apply_endpoint(&self, req: &mut Req, cfg: &ConfigBag) -> Result<(), BoxError>;
    // TODO(jdisanti) The EP Orc and Auth Orc need to share info on auth schemes but I'm not sure how that should happen
    fn resolve_auth_schemes(&self) -> Result<Vec<String>, BoxError>;
}

trait ConfigBagAccessors<Request, Response> {
    fn retry_strategy(&self) -> &dyn RetryStrategy<Request, Response>;
    fn endpoint_orchestrator(&self) -> &dyn EndpointOrchestrator<Request>;
    fn auth_orchestrator(&self) -> &dyn AuthOrchestrator<Request>;
    fn connection(&self) -> &dyn Connection<Request, Response>;
    fn response_deserializer(&self) -> &dyn ResponseDeserializer<Response>;
    fn trace_probe(&self) -> &dyn TraceProbe;
}

impl<Request, Response> ConfigBagAccessors<Request, Response> for ConfigBag
where
    Request: 'static,
    Response: 'static,
{
    fn retry_strategy(&self) -> &dyn RetryStrategy<Request, Response> {
        &**self
            .get::<Box<dyn RetryStrategy<Request, Response>>>()
            .expect("a retry strategy must be set")
    }

    fn endpoint_orchestrator(&self) -> &dyn EndpointOrchestrator<Request> {
        &**self
            .get::<Box<dyn EndpointOrchestrator<Request>>>()
            .expect("an endpoint orchestrator must be set")
    }

    fn auth_orchestrator(&self) -> &dyn AuthOrchestrator<Request> {
        &**self
            .get::<Box<dyn AuthOrchestrator<Request>>>()
            .expect("an auth orchestrator must be set")
    }

    fn connection(&self) -> &dyn Connection<Request, Response> {
        &**self
            .get::<Box<dyn Connection<Request, Response>>>()
            .expect("missing connector")
    }

    fn response_deserializer(&self) -> &dyn ResponseDeserializer<Response> {
        &**self
            .get::<Box<dyn ResponseDeserializer<Response>>>()
            .expect("missing response deserializer")
    }

    fn trace_probe(&self) -> &dyn TraceProbe {
        &**self
            .get::<Box<dyn TraceProbe>>()
            .expect("missing trace probe")
    }
}

/// `Request`: The transport request message e.g. `http::Request<SmithyBody>`
/// `Response`: The transport response message e.g. `http::Response<SmithyBody>`
pub async fn invoke<Request, Response>(
    input: Input,
    interceptors: &mut Interceptors<Request, Response>,
    runtime_plugins: &RuntimePlugins,
    cfg: &mut ConfigBag,
) -> Result<Output, SdkError<Error, Response>>
where
    Request: 'static,
    Response: 'static,
{
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
            let request_serializer = cfg
                .get::<Box<dyn RequestSerializer<Request>>>()
                .ok_or("missing serializer")?;
            let req = request_serializer.serialize_input(ctx.input(), cfg)?;
            ctx.set_tx_request(req);
            Result::<(), BoxError>::Ok(())
        })?
        // After serialization
        .include(|ctx| interceptors.read_after_serialization(ctx, cfg))?
        // Before retry loop
        .include_mut(|ctx| interceptors.modify_before_retry_loop(ctx, cfg))?
        .finish();

    {
        let retry_strategy = ConfigBagAccessors::<Request, Response>::retry_strategy(cfg);
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

        let retry_strategy = ConfigBagAccessors::<Request, Response>::retry_strategy(cfg);
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
        let trace_probe = ConfigBagAccessors::<Request, Response>::trace_probe(cfg);
        trace_probe.dispatch_events(cfg);

        break handling_phase.include(|ctx| interceptors.read_after_execution(ctx, cfg))?;
    };

    handling_phase.finalize()
}

// Making an HTTP request can fail for several reasons, but we still need to
// call lifecycle events when that happens. Therefore, we define this
// `make_an_attempt` function to make error handling simpler.
async fn make_an_attempt<Request, Response>(
    dispatch_phase: Phase<Request, Response>,
    cfg: &mut ConfigBag,
    interceptors: &mut Interceptors<Request, Response>,
) -> Result<Phase<Request, Response>, SdkError<Error, Response>>
where
    Request: 'static,
    Response: 'static,
{
    let mut context = dispatch_phase
        .include(|ctx| interceptors.read_before_attempt(ctx, cfg))?
        .include_mut(|ctx| {
            let tx_req_mut = ctx.tx_request_mut().expect("tx_request has been set");

            let endpoint_orchestrator =
                ConfigBagAccessors::<Request, Response>::endpoint_orchestrator(cfg);
            endpoint_orchestrator.resolve_and_apply_endpoint(tx_req_mut, cfg)
        })?
        .include_mut(|ctx| interceptors.modify_before_signing(ctx, cfg))?
        .include(|ctx| interceptors.read_before_signing(ctx, cfg))?
        .include_mut(|ctx| {
            let tx_req_mut = ctx.tx_request_mut().expect("tx_request has been set");
            let auth_orchestrator = ConfigBagAccessors::<Request, Response>::auth_orchestrator(cfg);
            auth_orchestrator.auth_request(tx_req_mut, cfg)
        })?
        .include(|ctx| interceptors.read_after_signing(ctx, cfg))?
        .include_mut(|ctx| interceptors.modify_before_transmit(ctx, cfg))?
        .include(|ctx| interceptors.read_before_transmit(ctx, cfg))?
        .finish();

    // The connection consumes the request but we need to keep a copy of it
    // within the interceptor context, so we clone it here.
    let call_result = {
        let tx_req = context.tx_request_mut().expect("tx_request has been set");
        let connection = ConfigBagAccessors::<Request, Response>::connection(cfg);
        connection.call(tx_req, cfg).await
    };

    Phase::dispatch(context)
        .include_mut(move |ctx| {
            ctx.set_tx_response(call_result?);
            Result::<(), BoxError>::Ok(())
        })?
        .include(|ctx| interceptors.read_after_transmit(ctx, cfg))?
        .include_mut(|ctx| interceptors.modify_before_deserialization(ctx, cfg))?
        .include(|ctx| interceptors.read_before_deserialization(ctx, cfg))?
        .include_mut(|ctx| {
            let tx_res = ctx.tx_response_mut().expect("tx_response has been set");
            let response_deserializer =
                ConfigBagAccessors::<Request, Response>::response_deserializer(cfg);
            let output_or_error = response_deserializer.deserialize_output(tx_res, cfg)?;
            ctx.set_output_or_error(output_or_error);
            Result::<(), BoxError>::Ok(())
        })?
        .include(|ctx| interceptors.read_after_deserialization(ctx, cfg))
}
