/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::SdkConfig;
use canary::{get_canaries_to_run, CanaryEnv};
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::{info, info_span, Instrument};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;
use tracing_texray::TeXRayLayer;

mod canary;

#[cfg(feature = "latest")]
mod latest;
#[cfg(feature = "latest")]
pub(crate) use latest as current_canary;

// NOTE: This module can be deleted 3 releases after release-2023-10-26
#[cfg(feature = "release-2023-10-26")]
mod release_2023_10_26;
#[cfg(feature = "release-2023-10-26")]
pub(crate) use release_2023_10_26 as current_canary;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(
            TeXRayLayer::new(), // by default, all metadata fields will be printed. If this is too noisy,
                                // filter only the fields you care about
                                //.only_show_fields(&["name", "operation", "service"]),
        );
    tracing::subscriber::set_global_default(subscriber).unwrap();
    let local = env::args().any(|arg| arg == "--local");
    let sdk_config = aws_config::load_from_env().await;
    if local {
        let result = lambda_main(sdk_config)
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
            Err(format!("canary failed: {result:?}").into())
        }
    } else {
        lambda_runtime::run(service_fn(|_event: LambdaEvent<Value>| {
            let sdk_config = sdk_config.clone();
            async move { lambda_main(sdk_config).await }
        }))
        .await?;
        Ok(())
    }
}

async fn lambda_main(sdk_config: SdkConfig) -> Result<Value, Error> {
    // Load necessary parameters from environment variables
    let env = CanaryEnv::from_env();
    info!("Env: {:#?}", env);

    // Get list of canaries to run and spawn them
    let canaries = get_canaries_to_run(sdk_config, env);
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
    match timeout(Duration::from_secs(180), handle).await {
        Err(_timeout) => Err("canary timed out".into()),
        Ok(Ok(result)) => match result {
            Ok(_) => Ok(()),
            Err(err) => Err(format!("{err:?}")),
        },
        Ok(Err(err)) => Err(err.to_string()),
    }
}
