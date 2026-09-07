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

use pokemon_service_server_sdk::{
    server::plugin::{HttpMarker, HttpPlugins, Plugin},
    PokemonService, PokemonServiceConfig,
};
use tower::{Layer, Service};

use aws_smithy_runtime::client::http::test_util::capture_request;
use pokemon_service_client::{Client, Config};
use pokemon_service_common::do_nothing;

#[tokio::test]
async fn plugin_layers_are_executed_in_registration_order() {
    // Each plugin layer will push its name into this vector when it gets invoked.
    // We can then check the vector content to verify the invocation order
    let output = Arc::new(Mutex::new(Vec::new()));

    let http_plugins = HttpPlugins::new()
        .push(SentinelPlugin::new("first", output.clone()))
        .push(SentinelPlugin::new("second", output.clone()));
    let config = PokemonServiceConfig::builder()
        .http_plugin(http_plugins)
        .build();
    let mut app = PokemonService::builder(config)
        .do_nothing(do_nothing)
        .build_unchecked();

    let request = {
        let (http_client, rcvr) = capture_request(None);
        let config = Config::builder()
            .http_client(http_client)
            .endpoint_url("http://localhost:1234")
            .build();
        Client::from_conf(config).do_nothing().send().await.unwrap();
        rcvr.expect_request()
    };

    app.call(request.try_into().unwrap()).await.unwrap();

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

impl<Ser, Op, T> Plugin<Ser, Op, T> for SentinelPlugin {
    type Output = SentinelService<T>;

    fn apply(&self, inner: T) -> Self::Output {
        SentinelService {
            inner,
            name: self.name,
            output: self.output.clone(),
        }
    }
}

impl HttpMarker for SentinelPlugin {}

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
