/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod auth;
mod conn;
mod de;
mod interceptors;
mod retry;
mod ser;

use aws_sdk_s3::input::GetObjectInput;
use aws_sdk_s3::model::ChecksumMode;
use aws_sdk_s3::output::GetObjectOutput;
use aws_smithy_http::body::SdkBody;
use aws_smithy_interceptor::Interceptors;
use aws_smithy_orchestrator::{invoke, BoxErr, ConfigBag};
use std::str::from_utf8;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), BoxErr> {
    tracing_subscriber::fmt::init();

    // Create the config we'll need to send the request + the request itself
    let sdk_config = aws_config::load_from_env().await;
    let service_config = aws_sdk_s3::Config::from(&sdk_config);
    // TODO Make it so these are added by default for S3
    // .with_runtime_plugin(auth::GetObjectAuthOrc::new())
    // .with_runtime_plugin(conn::HyperConnection::new());

    let input = GetObjectInput::builder()
        .bucket("zhessler-test-bucket")
        .key("1000-lines.txt")
        .checksum_mode(ChecksumMode::Enabled)
        // TODO Make it so these are added by default for this S3 operation
        // .with_runtime_plugin(retry::GetObjectRetryStrategy::new())
        // .with_runtime_plugin(de::GetObjectResponseDeserializer::new())
        // .with_runtime_plugin(ser::GetObjectInputSerializer::new())
        .build()?;

    let mut cfg = ConfigBag::new();
    let mut interceptors: Interceptors<
        GetObjectInput,
        http::Request<SdkBody>,
        http::Response<SdkBody>,
        Result<GetObjectOutput, BoxErr>,
    > = Interceptors::new();
    let res = invoke(input, &mut interceptors, &mut cfg).await?;

    let body = res.body.collect().await?.to_vec();
    let body_string = from_utf8(&body)?;

    info!("{body_string}");

    Ok(())
}
