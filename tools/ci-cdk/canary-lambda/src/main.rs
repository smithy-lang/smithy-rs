/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use canary::{get_canaries_to_run, CanaryEnv};
use lambda_runtime::{Context as LambdaContext, Error};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::{info, info_span, Instrument};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;
use tracing_texray::TeXRayLayer;

/// Conditionally include the module based on the $version feature gate
///
/// When the module is not included, an `mk_canary` function will be generated that returns `None`.
macro_rules! canary_module {
    ($name: ident, since: $version: expr) => {
        #[cfg(feature = $version)]
        mod $name;

        #[cfg(not(feature = $version))]
        mod $name {
            pub(crate) fn mk_canary(
                _clients: &crate::canary::Clients,
                _env: &crate::canary::CanaryEnv,
            ) -> Option<(&'static str, crate::canary::CanaryFuture)> {
                tracing::warn!(concat!(
                    stringify!($name),
                    " is disabled because it is not supported by this version of the SDK."
                ));
                None
            }
        }
    };
}

mod canary;

mod s3_canary;
canary_module!(paginator_canary, since: "v0.4.1");
mod transcribe_canary;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(
            TeXRayLayer::new()
                // by default, all metadata fields will be printed. If this is too noisy,
                // filter only the fields you care about
                //.only_show_fields(&["name", "operation", "service"]),
        );
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let local = env::args().any(|arg| arg == "--local");
    let main_handler = LambdaMain::new().await;
    if local {
        let result = lambda_main(main_handler.clients)
            .instrument(tracing_texray::examine(info_span!("run_canaries")))
            .await?;
        if result
            .as_object()
            .expect("is object")
            .get_key_value("result")
            .expect("exists")
            .1
            .as_str()
            .expect("is str")
            == "success"
        {
            Ok(())
        } else {
            Err(format!("canary failed: {:?}", result).into())
        }
    } else {
        lambda_runtime::run(main_handler).await?;
        Ok(())
    }
}

// Enables us to keep the clients alive between successive Lambda executions.
// Not because we need to for this use-case, but to demonstrate how to.
struct LambdaMain {
    clients: canary::Clients,
}

impl LambdaMain {
    async fn new() -> Self {
        Self {
            clients: canary::Clients::initialize().await,
        }
    }
}

impl lambda_runtime::Handler<Value, Value> for LambdaMain {
    type Error = Error;
    type Fut = Pin<Box<dyn Future<Output = Result<Value, Error>>>>;

    fn call(&self, _: Value, _: LambdaContext) -> Self::Fut {
        Box::pin(lambda_main(self.clients.clone()))
    }
}

async fn lambda_main(clients: canary::Clients) -> Result<Value, Error> {
    // Load necessary parameters from environment variables
    let env = CanaryEnv::from_env();
    info!("Env: {:#?}", env);

    // Get list of canaries to run and spawn them
    let canaries = get_canaries_to_run(clients, env);
    let join_handles = canaries
        .into_iter()
        .map(|(name, future)| (name, tokio::spawn(future)))
        .collect::<Vec<_>>();

    // Wait for and aggregate results
    let mut failures = BTreeMap::new();
    for (name, handle) in join_handles {
        if let Err(err) = canary_result(handle).await {
            failures.insert(name, err);
        }
    }

    let result = if failures.is_empty() {
        json!({ "result": "success" })
    } else {
        json!({ "result": "failure", "failures": failures })
    };
    info!("Result: {}", result);
    Ok(result)
}

async fn canary_result(handle: JoinHandle<anyhow::Result<()>>) -> Result<(), String> {
    match timeout(Duration::from_secs(20), handle).await {
        Err(_timeout) => Err("canary timed out".into()),
        Ok(Ok(result)) => match result {
            Ok(_) => Ok(()),
            Err(err) => Err(format!("{:?}", err)),
        },
        Ok(Err(err)) => Err(err.to_string()),
    }
}
