/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

mod conn;
mod interceptors;
mod retry;

use aws_sdk_s3::operation::get_object::{GetObjectError, GetObjectInput, GetObjectOutput};
use aws_sdk_s3::types::ChecksumMode;
use aws_smithy_runtime::client::orchestrator::invoke;
use aws_smithy_runtime_api::client::interceptors::Interceptors;
use aws_smithy_runtime_api::client::orchestrator::{BoxError, HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::runtime_plugin::{RuntimePlugin, RuntimePlugins};
use aws_smithy_runtime_api::config_bag::ConfigBag;
use aws_smithy_runtime_api::type_erasure::TypedBox;

#[derive(Debug)]
struct GetObjectOperationPlugin;

impl RuntimePlugin for GetObjectOperationPlugin {
    fn configure(&self, _cfg: &mut ConfigBag) -> Result<(), BoxError> {
        // TODO(orchestrator): Add operation-specific things to the config bag:
        // - serializer
        // - deserializer
        // - retry strategy
        todo!()
    }
}

#[tokio::test]
async fn sra_test() -> Result<(), BoxError> {
    tracing_subscriber::fmt::init();

    let input = TypedBox::new(
        GetObjectInput::builder()
            .bucket("zhessler-test-bucket")
            .key("1000-lines.txt")
            .checksum_mode(ChecksumMode::Enabled)
            .build()?,
    )
    .erase();

    let mut runtime_plugins = RuntimePlugins::new();

    // TODO(smithy-orchestrator-codegen) Make it so these are added by default for S3
    runtime_plugins
        .with_client_plugin(conn::HyperConnection::new())
        // TODO(smithy-orchestrator-codegen) Make it so these are added by default for this S3 operation
        .with_operation_plugin(GetObjectOperationPlugin);

    let mut cfg = ConfigBag::base();
    let mut interceptors: Interceptors<HttpRequest, HttpResponse> = Interceptors::new();
    let output = TypedBox::<GetObjectOutput>::assume_from(
        invoke(input, &mut interceptors, &runtime_plugins, &mut cfg)
            .await
            .map_err(|err| {
                err.map_service_error(|err| {
                    TypedBox::<GetObjectError>::assume_from(err)
                        .expect("error is GetObjectError")
                        .unwrap()
                })
            })?,
    )
    .expect("output is GetObjectOutput")
    .unwrap();

    dbg!(output);
    Ok(())
}
