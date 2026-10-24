/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A permanent HTTP fixture for request-body size limits and read deadlines.

use std::{future::Future, time::Duration};

use hyper_util::rt::TokioTimer;
use request_limits_server_sdk::{
    error, input, output, RequestLimitsService, RequestLimitsServiceConfig,
};
use tokio::net::TcpListener;

async fn echo(input: input::EchoInput) -> Result<output::EchoOutput, error::EchoError> {
    Ok(output::EchoOutput { note: input.note })
}

async fn echo_with_override(
    input: input::EchoWithOverrideInput,
) -> Result<output::EchoWithOverrideOutput, error::EchoWithOverrideError> {
    Ok(output::EchoWithOverrideOutput { note: input.note })
}

async fn upload(input: input::UploadInput) -> Result<output::UploadOutput, error::UploadError> {
    Ok(output::UploadOutput {
        size: input.data.as_ref().len() as i64,
    })
}

/// Serves the generated service until shutdown, with the same settings for tests and manual use.
/// The caller must supply a loopback listener.
pub async fn serve(
    listener: TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    if !listener.local_addr()?.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the request-limits fixture must listen on loopback",
        ));
    }
    let config = RequestLimitsServiceConfig::builder().build();
    let app = RequestLimitsService::builder(config)
        .echo(echo)
        .echo_with_override(echo_with_override)
        .upload(upload)
        .build()
        .expect("all fixture handlers are registered");

    request_limits_server_sdk::serve(listener, app.into_make_service())
        .header_read_timeout(Some(Duration::from_secs(10)))
        .configure_hyper(|mut builder| {
            builder
                .http2()
                .timer(TokioTimer::new())
                .keep_alive_interval(Some(Duration::from_secs(10)))
                .keep_alive_timeout(Duration::from_secs(5));
            builder
        })
        .with_graceful_shutdown(shutdown)
        .await
}
