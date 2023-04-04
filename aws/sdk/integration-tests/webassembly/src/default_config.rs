/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{Context, Result};
use aws_credential_types::Credentials;
use aws_smithy_types::{retry::RetryConfig, timeout::TimeoutConfig};
use aws_types::region::Region;
use std::{ffi::CStr, future::Future};

// This can be replaced with `std::env::vars` as soon as the component model lands in wasmtime
// https://github.com/bytecodealliance/wasmtime/issues/4185
fn read_environment() -> Result<(Option<String>, Option<String>, Option<String>)> {
    let (count, size) = unsafe { wasi::environ_sizes_get()? };
    let mut entries: Vec<*mut u8> = Vec::with_capacity(count);

    let mut buf: Vec<u8> = Vec::with_capacity(size);
    unsafe { wasi::environ_get(entries.as_mut_ptr(), buf.as_mut_ptr())? };
    unsafe { entries.set_len(count) };

    let mut access_key: Option<String> = None;
    let mut secret_key: Option<String> = None;
    let mut session_token: Option<String> = None;

    for entry in entries {
        let cstr = unsafe { CStr::from_ptr(entry as *const i8) };
        let (name, value) = cstr
            .to_str()?
            .split_once('=')
            .context("missing = in environment variable")?;
        if name == "AWS_ACCESS_KEY_ID" {
            access_key = Some(value.to_string());
        } else if name == "AWS_SECRET_ACCESS_KEY" {
            secret_key = Some(value.to_string());
        } else if name == "AWS_SESSION_TOKEN" {
            session_token = Some(value.to_string());
        }
    }

    Ok((access_key, secret_key, session_token))
}

pub(crate) fn get_default_config() -> impl Future<Output = aws_config::SdkConfig> {
    let (access_key, secret_key, session_token) =
        read_environment().expect("read aws credentials from environment variables");

    aws_config::from_env()
        .region(Region::from_static("us-west-2"))
        .credentials_provider(Credentials::from_keys(
            access_key.expect("AWS_ACCESS_KEY_ID"),
            secret_key.expect("AWS_SECRET_ACCESS_KEY"),
            session_token,
        ))
        .timeout_config(TimeoutConfig::disabled())
        .retry_config(RetryConfig::disabled())
        .load()
}

#[tokio::test]
pub async fn test_default_config() {
    let shared_config = get_default_config().await;
    let client = aws_sdk_s3::Client::new(&shared_config);
    assert_eq!(client.conf().region().unwrap().to_string(), "us-west-2")
}
