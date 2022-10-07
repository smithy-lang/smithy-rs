/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_http::user_agent::AwsUserAgent;
use aws_sdk_s3::middleware::DefaultMiddleware;
use aws_sdk_s3::model::{
    CompressionType, CsvInput, CsvOutput, ExpressionType, FileHeaderInfo, InputSerialization,
    OutputSerialization, SelectObjectContentEventStream,
};
use aws_sdk_s3::operation::SelectObjectContent;
use aws_sdk_s3::{Client, Config, Credentials, Region};
use aws_smithy_client::dvr::{Event, ReplayingConnection};
use aws_smithy_client::test_connection::TestConnection;
use aws_smithy_http::body::SdkBody;
use aws_smithy_protocol_test::{assert_ok, validate_body, MediaType};
use std::error::Error as StdError;
use std::time::{Duration, UNIX_EPOCH};

#[tokio::test]
async fn test_success() {
    let events: Vec<Event> =
        serde_json::from_str(include_str!("select-object-content.json")).unwrap();
    let replayer = ReplayingConnection::new(events);

    let region = Region::from_static("us-east-2");
    let credentials = Credentials::new("test", "test", None, None, "test");
    let config = Config::builder()
        .region(region)
        .credentials_provider(credentials)
        .build();
    let client = Client::from_conf_conn(config, replayer.clone());

    let mut output = client
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
        .unwrap();

    let mut received = Vec::new();
    while let Some(event) = output.payload.recv().await.unwrap() {
        match event {
            SelectObjectContentEventStream::Records(records) => {
                received.push(
                    std::str::from_utf8(records.payload.as_ref().unwrap().as_ref())
                        .unwrap()
                        .trim()
                        .to_string(),
                );
            }
            SelectObjectContentEventStream::Stats(stats) => {
                let stats = stats.details.unwrap();
                received.push(format!(
                    "scanned:{},processed:{},returned:{}",
                    stats.bytes_scanned, stats.bytes_processed, stats.bytes_returned
                ))
            }
            SelectObjectContentEventStream::End(_) => {}
            otherwise => panic!("unexpected message: {:?}", otherwise),
        }
    }
    assert_eq!(
        vec![
            "Jane,(949) 555-6704,Chicago,Developer".to_string(),
            "scanned:333,processed:333,returned:39".to_string()
        ],
        received
    );

    // Validate the requests
    replayer
        .validate(&["content-type", "content-length"], body_validator)
        .await
        .unwrap();
}

fn body_validator(expected_body: &[u8], actual_body: &[u8]) -> Result<(), Box<dyn StdError>> {
    let expected = std::str::from_utf8(expected_body).unwrap();
    let actual = std::str::from_utf8(actual_body).unwrap();
    assert_ok(validate_body(actual, expected, MediaType::Xml));
    Ok(())
}

#[tokio::test]
async fn test_scan_range_starting_at_zero() {
    pub type Client<C> = aws_smithy_client::Client<C, DefaultMiddleware>;
    let request_body_xml: &[u8] = br#"<SelectObjectContentRequest xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Expression>SELECT * FROM s3object s WHERE s.id=&apos;1&apos;</Expression><ExpressionType>SQL</ExpressionType><InputSerialization><CSV><FileHeaderInfo>USE</FileHeaderInfo></CSV><CompressionType>NONE</CompressionType></InputSerialization><OutputSerialization><CSV></CSV></OutputSerialization><ScanRange><Start>0</Start><End>1000</End></ScanRange></SelectObjectContentRequest>"#;

    let creds = Credentials::new(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
        None,
        "test",
    );
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(creds)
        .region(Region::new("us-east-1"))
        .build();
    let conn = TestConnection::new(vec![(
        http::Request::builder()
            .header("authorization", "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210618/us-east-1/s3/aws4_request, SignedHeaders=content-length;content-type;host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=e817c816a21d8e74671d30cc1f85ff353e6e904050690e064486ac974902ab3b")
            .uri("https://s3.us-east-1.amazonaws.com/test-bucket/test-file.csv?select&select-type=2&x-id=SelectObjectContent")
            .body(request_body_xml.into())
            .unwrap(),
        http::Response::builder().status(200).body("").unwrap(),
    )]);
    let client = Client::new(conn.clone());
    let mut op = SelectObjectContent::builder()
        .bucket("test-bucket")
        .key("test-file.csv")
        .expression_type(ExpressionType::Sql)
        .expression(r#"SELECT * FROM s3object s WHERE s.id='1'"#)
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
        .scan_range(
            aws_sdk_s3::model::ScanRange::builder()
                .start(0)
                .end(1000)
                .build(),
        )
        .build()
        .unwrap()
        .make_operation(&conf)
        .await
        .unwrap();
    op.properties_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1624036048));
    op.properties_mut().insert(AwsUserAgent::for_tests());

    // Event streams have an output even when the response body is empty
    let mut output = client.call(op).await.unwrap();
    let mut received = Vec::new();
    while let Some(event) = output.payload.recv().await.unwrap() {
        match event {
            SelectObjectContentEventStream::Records(records) => {
                received.push(
                    std::str::from_utf8(records.payload.as_ref().unwrap().as_ref())
                        .unwrap()
                        .trim()
                        .to_string(),
                );
            }
            SelectObjectContentEventStream::Stats(stats) => {
                let stats = stats.details.unwrap();
                received.push(format!(
                    "scanned:{},processed:{},returned:{}",
                    stats.bytes_scanned, stats.bytes_processed, stats.bytes_returned
                ))
            }
            SelectObjectContentEventStream::End(_) => {}
            otherwise => panic!("unexpected message: {:?}", otherwise),
        }
    }

    conn.assert_requests_match(&[]);
}
