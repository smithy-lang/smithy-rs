/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use canary::{get_canaries_to_run, CanaryEnv};
use lambda_runtime::{Context as LambdaContext, Error};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use tokio::task::JoinHandle;
use tracing::info;

mod canary;
mod s3_canary;
mod transcribe_canary;

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let main_handler = LambdaMain::new().await;
    lambda_runtime::run(main_handler).await?;
    Ok(())
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
        .map(|(name, future)| (name, tokio::spawn(future)));

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
    match handle.await {
        Ok(result) => match result {
            Ok(_) => Ok(()),
            Err(err) => Err(err.to_string()),
        },
        Err(err) => Err(err.to_string()),
    }
}
