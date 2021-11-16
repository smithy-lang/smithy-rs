/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_sdk_s3::model::{
    CompressionType, CsvInput, CsvOutput, ExpressionType, FileHeaderInfo, InputSerialization,
    OutputSerialization,
};
use aws_sdk_s3::{Client, Config, Credentials, Region, TimeoutConfig};
use aws_smithy_client::never::NeverService;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::result::ConnectorError;
use std::time::Duration;

// Copied from aws-smithy-client/src/hyper_impls.rs
macro_rules! assert_elapsed {
    ($start:expr, $dur:expr) => {{
        let elapsed = $start.elapsed();
        // type ascription improves compiler error when wrong type is passed
        let lower: std::time::Duration = $dur;

        // Handles ms rounding
        assert!(
            elapsed >= lower && elapsed <= lower + std::time::Duration::from_millis(5),
            "actual = {:?}, expected = {:?}",
            elapsed,
            lower
        );
    }};
}

#[tokio::test]
async fn test_timeout_service_ends_request_that_never_completes() {
    let conn: NeverService<http::Request<SdkBody>, http::Response<SdkBody>, ConnectorError> =
        NeverService::new();
    let region = Region::from_static("us-east-2");
    let credentials = Credentials::from_keys("test", "test", None);
    let timeout_config = TimeoutConfig::new().with_api_call_timeout(Duration::from_secs_f32(0.5));
    let config = Config::builder()
        .region(region)
        .credentials_provider(credentials)
        .timeout_config(timeout_config)
        .build();
    let client = Client::from_conf_conn(config, conn.clone());

    let now = tokio::time::Instant::now();
    tokio::time::pause();

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

    assert_eq!(format!("{:?}", err), "ConstructionFailure(TimedOutError)");
    assert_elapsed!(now, std::time::Duration::from_secs_f32(0.5));
}
