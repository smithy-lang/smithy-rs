/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{Context, Error};
use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::{
    Environment, FunctionConfiguration, InvocationType, LastUpdateStatus, LogType,
};
use aws_sdk_lambda::Client;
use clap::Parser;
use smithy_rs_tool_common::here;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_stream::StreamExt;
use tracing::info;

#[derive(Debug, Parser, Eq, PartialEq)]
pub struct InvokeArgs {
    #[clap(long)]
    cold_start: bool,
    #[clap(short, long)]
    func_name_filter: Option<String>,
    #[clap(short, long)]
    debug: bool,

    #[clap(short, long, default_value_t = 1)]
    count: usize,
}

fn force_cold_start(vars: &mut HashMap<String, String>) {
    let new_value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .to_string();
    vars.insert("NONCE".to_string(), new_value.clone());
}

async fn wait_for_update(lambda_client: &Client, name: &str) -> Result<(), Error> {
    loop {
        let func = lambda_client
            .get_function_configuration()
            .function_name(name)
            .send()
            .await?;
        if func.last_update_status().unwrap() == &LastUpdateStatus::Successful {
            break;
        } else {
            tokio::time::sleep(Duration::from_secs(1)).await;
            info!("waiting for function to be ready...");
        }
    }
    Ok(())
}

async fn set_lambda_env(
    lambda_client: &aws_sdk_lambda::Client,
    name: &str,
    update: impl Fn(HashMap<String, String>) -> HashMap<String, String>,
) -> Result<(), anyhow::Error> {
    let mut current_config = lambda_client
        .get_function_configuration()
        .function_name(name)
        .send()
        .await?
        .environment
        .unwrap()
        .variables
        .unwrap_or_default();
    let initial = current_config.clone();
    let new = update(current_config);
    if new == initial {
        info!("no updates!");
        return Ok(());
    }

    lambda_client
        .update_function_configuration()
        .function_name(name)
        .environment(Environment::builder().set_variables(Some(new)).build())
        .send()
        .await?;
    wait_for_update(lambda_client, name).await?;
    Ok(())
}

pub(crate) async fn invoke(args: InvokeArgs) -> Result<(), anyhow::Error> {
    let conf = aws_config::load_from_env().await;
    let client = aws_sdk_lambda::Client::new(&conf);
    for _ in 0..args.count {
        invoke_once(&args, &client).await?;
    }
    Ok(())
}

async fn invoke_once(
    args: &InvokeArgs,
    lambda: &aws_sdk_lambda::Client,
) -> Result<(), anyhow::Error> {
    let mut name: Vec<FunctionConfiguration> = lambda
        .list_functions()
        .into_paginator()
        .items()
        .send()
        .collect::<Result<Vec<FunctionConfiguration>, _>>()
        .await?;
    name.sort_by(|a, b| a.last_modified().cmp(&b.last_modified()));
    name.reverse();
    let name_filter = args.func_name_filter.as_deref().unwrap_or("canary");
    let latest_canary = name
        .iter()
        .find(|name| name.function_name().unwrap().contains(name_filter))
        .and_then(|name| name.function_name())
        .ok_or_else(|| {
            anyhow::Error::msg(format!(
                "no canary functions found ({:?})",
                name.iter()
                    .map(|f| f.function_name().unwrap())
                    .collect::<Vec<_>>()
            ))
        })?;
    info!("Invoking {latest_canary}");

    set_lambda_env(&lambda, &latest_canary, |mut map| {
        if args.debug {
            map.insert("RUST_LOG".to_string(), "debug".to_string());
        }
        if args.cold_start {
            force_cold_start(&mut map);
        }
        map
    })
    .await?;
    let response = lambda
        .invoke()
        .function_name(latest_canary)
        .invocation_type(InvocationType::RequestResponse)
        .log_type(LogType::Tail)
        .payload(Blob::new(&b"{\"action\":\"BenchDynamo\"}"[..]))
        .send()
        .await
        .context(here!("failed to invoke the canary Lambda"))?;

    if let Some(log_result) = response.log_result() {
        tracing::debug!(
            "Last 4 KB of canary logs:\n----\n{}\n----\n",
            std::str::from_utf8(&base64::decode(log_result)?)?
        );
    }
    if let Some(payload) = response.payload() {
        tracing::info!(payload = %std::str::from_utf8(payload.as_ref()).unwrap());
    }
    Ok(())
}
