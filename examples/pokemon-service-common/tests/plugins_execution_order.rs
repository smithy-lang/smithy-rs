/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    ops::Deref,
    sync::Arc,
    sync::Mutex,
    task::{Context, Poll},
};

use aws_smithy_http::body::SdkBody;
use aws_smithy_http_server::{
    operation::Operation,
    plugin::{Plugin, PluginPipeline},
};
use tower::layer::util::Stack;
use tower::{Layer, Service};

use pokemon_service_client::{operation::do_nothing::DoNothingInput, Config};
use pokemon_service_common::do_nothing;

trait OperationExt {
    /// Convert an SDK operation into an `http::Request`.
    fn into_http(self) -> http::Request<SdkBody>;
}

impl<H, R> OperationExt for aws_smithy_http::operation::Operation<H, R> {
    fn into_http(self) -> http::Request<SdkBody> {
        self.into_request_response().0.into_parts().0
    }
}

#[tokio::test]
async fn plugin_layers_are_executed_in_registration_order() {
    // Each plugin layer will push its name into this vector when it gets invoked.
    // We can then check the vector content to verify the invocation order
    let output = Arc::new(Mutex::new(Vec::new()));

    let pipeline = PluginPipeline::new()
        .push(SentinelPlugin::new("first", output.clone()))
        .push(SentinelPlugin::new("second", output.clone()));
    let mut app = pokemon_service_server_sdk::PokemonService::builder_with_plugins(pipeline)
        .do_nothing(do_nothing)
        .build_unchecked();
    let request = DoNothingInput::builder()
        .build()
        .unwrap()
        .make_operation(&Config::builder().build())
        .await
        .unwrap()
        .into_http();
    app.call(request).await.unwrap();

    let output_guard = output.lock().unwrap();
    assert_eq!(output_guard.deref(), &vec!["first", "second"]);
}

struct SentinelPlugin {
    name: &'static str,
    output: Arc<Mutex<Vec<&'static str>>>,
}

impl SentinelPlugin {
    pub fn new(name: &'static str, output: Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self { name, output }
    }
}

impl<Protocol, Op, S, L> Plugin<Protocol, Op, S, L> for SentinelPlugin {
    type Service = S;
    type Layer = Stack<L, SentinelLayer>;

    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
        input.layer(SentinelLayer {
            name: self.name,
            output: self.output.clone(),
        })
    }
}

/// A [`Service`] that adds a print log.
#[derive(Clone, Debug)]
pub struct SentinelService<S> {
    inner: S,
    output: Arc<Mutex<Vec<&'static str>>>,
    name: &'static str,
}

impl<R, S> Service<R> for SentinelService<S>
where
    S: Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        self.output.lock().unwrap().push(self.name);
        self.inner.call(req)
    }
}

/// A [`Layer`] which constructs the [`PrintService`].
#[derive(Debug)]
pub struct SentinelLayer {
    name: &'static str,
    output: Arc<Mutex<Vec<&'static str>>>,
}

impl<S> Layer<S> for SentinelLayer {
    type Service = SentinelService<S>;

    fn layer(&self, service: S) -> Self::Service {
        SentinelService {
            inner: service,
            output: self.output.clone(),
            name: self.name,
        }
    }
}
