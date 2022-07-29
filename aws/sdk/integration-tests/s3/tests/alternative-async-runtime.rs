/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::model::{
    CompressionType, CsvInput, CsvOutput, ExpressionType, FileHeaderInfo, InputSerialization,
    OutputSerialization,
};
use aws_sdk_s3::{Client, Config, Credentials, Region};
use aws_smithy_async::assert_elapsed;
use aws_smithy_async::rt::sleep::{AsyncSleep, Sleep};
use aws_smithy_client::never::NeverConnector;
use aws_smithy_http::result::SdkError;
use aws_smithy_types::timeout;
use aws_smithy_types::tristate::TriState;

use aws_config::meta::credentials::LazyCachingCredentialsProvider;
use aws_smithy_client::http_connector::HttpConnector;
use aws_smithy_http::body::SdkBody;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

#[derive(Debug)]
struct SmolSleep;

impl AsyncSleep for SmolSleep {
    fn sleep(&self, duration: Duration) -> Sleep {
        Sleep::new(async move {
            smol::Timer::after(duration).await;
        })
    }
}

#[test]
fn test_smol_runtime_timeouts() {
    if let Err(err) = smol::block_on(async { timeout_test(Arc::new(SmolSleep)).await }) {
        println!("{err}");
        panic!();
    }
}

#[test]
fn test_smol_runtime_retry() {
    if let Err(err) = smol::block_on(async { retry_test(Arc::new(SmolSleep)).await }) {
        println!("{err}");
        panic!();
    }
}

// This test is ignored because it actually sends a request to AWS
#[ignore]
#[test]
fn test_smol_runtime_list_buckets() {
    if let Err(err) = smol::block_on(async { list_buckets_test(Arc::new(SmolSleep)).await }) {
        println!("{err}");
        panic!();
    }
}

#[derive(Debug)]
struct AsyncStdSleep;

impl AsyncSleep for AsyncStdSleep {
    fn sleep(&self, duration: Duration) -> Sleep {
        Sleep::new(async move { async_std::task::sleep(duration).await })
    }
}

#[test]
fn test_async_std_runtime_timeouts() {
    if let Err(err) =
        async_std::task::block_on(async { timeout_test(Arc::new(AsyncStdSleep)).await })
    {
        println!("{err}");
        panic!();
    }
}

#[test]
fn test_async_std_runtime_retry() {
    if let Err(err) = async_std::task::block_on(async { retry_test(Arc::new(AsyncStdSleep)).await })
    {
        println!("{err}");
        panic!();
    }
}

// This test is ignored because it actually sends a request to AWS
// #[ignore]
#[test]
fn test_async_std_runtime_list_buckets() {
    if let Err(err) =
        async_std::task::block_on(async { list_buckets_test(Arc::new(AsyncStdSleep)).await })
    {
        println!("{err}");
        panic!();
    }
}

async fn timeout_test(sleep_impl: Arc<dyn AsyncSleep>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = NeverConnector::new();
    let region = Region::from_static("us-east-2");
    let credentials = Credentials::new("test", "test", None, None, "test");
    let api_timeouts =
        timeout::Api::new().with_call_timeout(TriState::Set(Duration::from_secs_f32(0.5)));
    let timeout_config = timeout::Config::new().with_api_timeouts(api_timeouts);
    let config = Config::builder()
        .region(region)
        .credentials_provider(credentials)
        .timeout_config(timeout_config)
        .sleep_impl(sleep_impl)
        .build();
    let client = Client::from_conf_conn(config, conn.clone());

    let now = std::time::Instant::now();

    let err = client
        .select_object_content()
        .bucket("aws-rust-sdk")
        .key("sample_data.csv")
        .expression_type(ExpressionType::Sql)
        .expression("SELECT * FROM s3object s WHERE s.\"Name\" = 'Jane'")
        .input_serialization(
            InputSerialization::builder()
                .csv(
                    CsvInput::builder()
                        .file_header_info(FileHeaderInfo::Use)
                        .build(),
                )
                .compression_type(CompressionType::None)
                .build(),
        )
        .output_serialization(
            OutputSerialization::builder()
                .csv(CsvOutput::builder().build())
                .build(),
        )
        .send()
        .await
        .unwrap_err();

    assert_eq!(format!("{:?}", err), "TimeoutError(RequestTimeoutError { kind: \"API call (all attempts including retries)\", duration: 500ms })");
    assert_elapsed!(now, std::time::Duration::from_secs_f32(0.5));

    Ok(())
}

async fn retry_test(sleep_impl: Arc<dyn AsyncSleep>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = NeverConnector::new();
    let credentials = Credentials::new("test", "test", None, None, "test");
    let conf = aws_types::SdkConfig::builder()
        .region(Region::new("us-east-2"))
        .credentials_provider(aws_types::credentials::SharedCredentialsProvider::new(
            credentials,
        ))
        .timeout_config(
            timeout::Config::new().with_api_timeouts(
                timeout::Api::new()
                    .with_call_attempt_timeout(TriState::Set(Duration::from_secs_f64(0.1))),
            ),
        )
        .sleep_impl(sleep_impl)
        .build();
    let client = Client::from_conf_conn(Config::new(&conf), conn.clone());
    let resp = client
        .list_buckets()
        .send()
        .await
        .expect_err("call should fail");
    assert_eq!(
        conn.num_calls(),
        3,
        "client level timeouts should be retried"
    );
    assert!(
        matches!(resp, SdkError::TimeoutError { .. }),
        "expected a timeout error, got: {}",
        resp
    );

    Ok(())
}

#[derive(Clone)]
struct SurfConnector;

impl tower::Service<http::Request<SdkBody>> for SurfConnector {
    type Response = http::Response<SdkBody>;
    type Error = aws_smithy_http::result::ConnectorError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<SdkBody>) -> Self::Future {
        Box::pin(async move {
            let client = surf::Client::new();
            client
                .send(convert_request(req))
                .await
                .map(convert_response)
                .map_err(|surf_err| {
                    let box_err = Box::new(SurfError(surf_err.to_string()));
                    Self::Error::other(box_err, None)
                })
        })
    }
}

#[derive(Debug)]
struct SurfError(String);

impl std::fmt::Display for SurfError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SurfError {}

async fn list_buckets_test(
    sleep_impl: Arc<dyn AsyncSleep>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let conn = aws_smithy_client::erase::DynConnector::new(SurfConnector);
    let sdk_config = aws_config::from_env()
        .sleep_impl(sleep_impl.clone())
        .http_connector(HttpConnector::Prebuilt(Some(conn.clone())))
        // The credentials provider also requires us to pass a sleep impl
        .credentials_provider(
            LazyCachingCredentialsProvider::builder()
                .sleep(sleep_impl)
                .build(),
        )
        .load()
        .await;
    let service_config = aws_sdk_s3::Config::from(&sdk_config);
    let client = Client::from_conf_conn(service_config, conn);
    let _ = client.list_buckets().send().await?;

    Ok(())
}

fn convert_request(_req: http::Request<SdkBody>) -> surf::Request {
    todo!()
}

fn convert_response(_res: surf::Response) -> http::Response<SdkBody> {
    todo!()
}
