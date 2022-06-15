/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_http::user_agent::AwsUserAgent;
use aws_sdk_s3::{model::ChecksumAlgorithm, operation::PutObject};
use aws_smithy_client::test_connection::capture_request;
// use aws_smithy_types::base64;
use bytes::Buf;
use http::HeaderValue;
// use std::fmt::Write;
use std::io::Read;
use std::time::{Duration, UNIX_EPOCH};

// // Use this test to run a real request against S3 to prove that things work
// #[tokio::test]
// async fn test_checksum_on_streaming_request_against_s3() {
//     let sdk_config = aws_config::from_env().load().await;
//     let s3_client = aws_sdk_s3::Client::new(&sdk_config);
//
//     let input_text = b"";
//     let _res = s3_client
//         .put_object()
//         .bucket("your-test-bucket")
//         .key("test.txt")
//         .body(aws_sdk_s3::types::ByteStream::from_static(input_text))
//         .checksum_algorithm(ChecksumAlgorithm::Sha256)
//         .send()
//         .await
//         .unwrap();
// }

// #[tokio::test]
// async fn test_checksum_on_streaming_response() {
//     let sdk_config = aws_config::from_env().load().await;
//     let s3_client = aws_sdk_s3::Client::new(&sdk_config);
//
//     let res = s3_client
//         .get_object()
//         .bucket("telephone-game")
//         .key("test.txt")
//         .checksum_mode(ChecksumMode::Enabled)
//         .send()
//         .await
//         .unwrap();
//
//     fn decode_checksum_header(value: String) -> String {
//         let decoded_value = base64::decode(value).unwrap();
//         let mut bytes = String::with_capacity(decoded_value.len());
//         for byte in decoded_value.into_iter() {
//             write!(bytes, "{:x}", byte).unwrap();
//         }
//
//         bytes
//     }
//
//     let crc32 = res.checksum_crc32.map(decode_checksum_header);
//     let crc32c = res.checksum_crc32_c.map(decode_checksum_header);
//     let sha1 = res.checksum_sha1.map(decode_checksum_header);
//     let sha256 = res.checksum_sha256.map(decode_checksum_header);
//
//     println!(
//         r#"checksum headers:
//         crc32: {crc32:?}
//         crc32c: {crc32c:?}
//         sha1: {sha1:?}
//         sha256: {sha256:?}
//     "#
//     );
//
//     let body = res.body.collect().await.unwrap();
//     let mut output_text = String::new();
//     body.reader()
//         .read_to_string(&mut output_text)
//         .expect("Doesn't cause IO errors");
//     assert_eq!(output_text, "Hello world");
// }

#[tokio::test]
async fn test_crc32_checksum_on_streaming_request() {
    let creds = aws_sdk_s3::Credentials::new(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
        None,
        "test",
    );
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(creds)
        .region(aws_sdk_s3::Region::new("us-east-1"))
        .build();
    let (conn, rcvr) = capture_request(None);

    let client: aws_smithy_client::Client<_, aws_sdk_s3::middleware::DefaultMiddleware> =
        aws_smithy_client::Client::new(conn.clone());

    let input_text = b"Hello world";
    let mut op = PutObject::builder()
        .bucket("test-bucket")
        .key("test.txt")
        .body(aws_sdk_s3::types::ByteStream::from_static(input_text))
        .checksum_algorithm(ChecksumAlgorithm::Crc32)
        .build()
        .unwrap()
        .make_operation(&conf)
        .await
        .expect("failed to construct operation");
    op.properties_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1624036048));
    op.properties_mut().insert(AwsUserAgent::for_tests());

    // The response from the fake connection won't return the expected XML but we don't care about
    // that error in this test
    let _ = client.call(op).await;
    let req = rcvr.expect_request();

    let headers = req.headers();
    let x_amz_content_sha256 = headers
        .get("x-amz-content-sha256")
        .expect("x-amz-content-sha256 header exists");
    let x_amz_trailer = headers
        .get("x-amz-trailer")
        .expect("x-amz-trailer header exists");
    let x_amz_decoded_content_length = headers
        .get("x-amz-decoded-content-length")
        .expect("x-amz-decoded-content-length header exists");
    let content_length = headers
        .get("Content-Length")
        .expect("Content-Length header exists");
    let content_encoding = headers
        .get("Content-Encoding")
        .expect("Content-Encoding header exists");

    assert_eq!(
        HeaderValue::from_static("STREAMING-UNSIGNED-PAYLOAD-TRAILER"),
        x_amz_content_sha256,
        "signing header is incorrect"
    );
    assert_eq!(
        HeaderValue::from_static("x-amz-checksum-crc32"),
        x_amz_trailer,
        "x-amz-trailer is incorrect"
    );
    assert_eq!(
        HeaderValue::from_static(aws_http::content_encoding::header_value::AWS_CHUNKED),
        content_encoding,
        "content-encoding wasn't set to aws-chunked"
    );

    // The length of the string "Hello world"
    assert_eq!(
        HeaderValue::from_static("11"),
        x_amz_decoded_content_length,
        "decoded content length was wrong"
    );
    // The sum of the length of the original body, chunk markers, and trailers
    assert_eq!(
        HeaderValue::from_static("52"),
        content_length,
        "content-length was wrong"
    );

    let body = collect_body_into_string(req.into_body()).await;
    // When sending a streaming body with a checksum, the trailers are included as part of the body content
    assert_eq!(
        body.as_str(),
        "B\r\nHello world\r\n0\r\nx-amz-checksum-crc32:i9aeUg==\r\n\r\n"
    );
}

// This test isn't a duplicate. It tests CRC32C (note the C) checksum request validation
#[tokio::test]
async fn test_crc32c_checksum_on_streaming_request() {
    let creds = aws_sdk_s3::Credentials::new(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
        None,
        "test",
    );
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(creds)
        .region(aws_sdk_s3::Region::new("us-east-1"))
        .build();
    let (conn, rcvr) = capture_request(None);

    let client: aws_smithy_client::Client<_, aws_sdk_s3::middleware::DefaultMiddleware> =
        aws_smithy_client::Client::new(conn.clone());

    let input_text = b"Hello world";
    let mut op = PutObject::builder()
        .bucket("test-bucket")
        .key("test.txt")
        .body(aws_sdk_s3::types::ByteStream::from_static(input_text))
        .checksum_algorithm(ChecksumAlgorithm::Crc32C)
        .build()
        .unwrap()
        .make_operation(&conf)
        .await
        .expect("failed to construct operation");
    op.properties_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1624036048));
    op.properties_mut().insert(AwsUserAgent::for_tests());

    // The response from the fake connection won't return the expected XML but we don't care about
    // that error in this test
    let _ = client.call(op).await;
    let req = rcvr.expect_request();

    let headers = req.headers();
    let x_amz_content_sha256 = headers
        .get("x-amz-content-sha256")
        .expect("x-amz-content-sha256 header exists");
    let x_amz_trailer = headers
        .get("x-amz-trailer")
        .expect("x-amz-trailer header exists");
    let x_amz_decoded_content_length = headers
        .get("x-amz-decoded-content-length")
        .expect("x-amz-decoded-content-length header exists");
    let content_length = headers
        .get("Content-Length")
        .expect("Content-Length header exists");
    let content_encoding = headers
        .get("Content-Encoding")
        .expect("Content-Encoding header exists");

    assert_eq!(
        HeaderValue::from_static("STREAMING-UNSIGNED-PAYLOAD-TRAILER"),
        x_amz_content_sha256,
        "signing header is incorrect"
    );
    assert_eq!(
        HeaderValue::from_static("x-amz-checksum-crc32c"),
        x_amz_trailer,
        "x-amz-trailer is incorrect"
    );
    assert_eq!(
        HeaderValue::from_static(aws_http::content_encoding::header_value::AWS_CHUNKED),
        content_encoding,
        "content-encoding wasn't set to aws-chunked"
    );

    // The length of the string "Hello world"
    assert_eq!(
        HeaderValue::from_static("11"),
        x_amz_decoded_content_length,
        "decoded content length was wrong"
    );
    // The sum of the length of the original body, chunk markers, and trailers
    assert_eq!(
        HeaderValue::from_static("53"),
        content_length,
        "content-length was wrong"
    );

    let body = collect_body_into_string(req.into_body()).await;
    // When sending a streaming body with a checksum, the trailers are included as part of the body content
    assert_eq!(
        body.as_str(),
        "B\r\nHello world\r\n0\r\nx-amz-checksum-crc32c:crUfeA==\r\n\r\n"
    );
}

#[tokio::test]
async fn test_sha1_checksum_on_streaming_request() {
    let creds = aws_sdk_s3::Credentials::new(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
        None,
        "test",
    );
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(creds)
        .region(aws_sdk_s3::Region::new("us-east-1"))
        .build();
    let (conn, rcvr) = capture_request(None);

    let client: aws_smithy_client::Client<_, aws_sdk_s3::middleware::DefaultMiddleware> =
        aws_smithy_client::Client::new(conn.clone());

    let input_text = b"Hello world";
    let mut op = PutObject::builder()
        .bucket("test-bucket")
        .key("test.txt")
        .body(aws_sdk_s3::types::ByteStream::from_static(input_text))
        .checksum_algorithm(ChecksumAlgorithm::Sha1)
        .build()
        .unwrap()
        .make_operation(&conf)
        .await
        .expect("failed to construct operation");
    op.properties_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1624036048));
    op.properties_mut().insert(AwsUserAgent::for_tests());

    // The response from the fake connection won't return the expected XML but we don't care about
    // that error in this test
    let _ = client.call(op).await;
    let req = rcvr.expect_request();

    let headers = req.headers();
    let x_amz_content_sha256 = headers
        .get("x-amz-content-sha256")
        .expect("x-amz-content-sha256 header exists");
    let x_amz_trailer = headers
        .get("x-amz-trailer")
        .expect("x-amz-trailer header exists");
    let x_amz_decoded_content_length = headers
        .get("x-amz-decoded-content-length")
        .expect("x-amz-decoded-content-length header exists");
    let content_length = headers
        .get("Content-Length")
        .expect("Content-Length header exists");
    let content_encoding = headers
        .get("Content-Encoding")
        .expect("Content-Encoding header exists");

    assert_eq!(
        HeaderValue::from_static("STREAMING-UNSIGNED-PAYLOAD-TRAILER"),
        x_amz_content_sha256,
        "signing header is incorrect"
    );
    assert_eq!(
        HeaderValue::from_static("x-amz-checksum-sha1"),
        x_amz_trailer,
        "x-amz-trailer is incorrect"
    );
    assert_eq!(
        HeaderValue::from_static(aws_http::content_encoding::header_value::AWS_CHUNKED),
        content_encoding,
        "content-encoding wasn't set to aws-chunked"
    );

    // The length of the string "Hello world"
    assert_eq!(
        HeaderValue::from_static("11"),
        x_amz_decoded_content_length,
        "decoded content length was wrong"
    );
    // The sum of the length of the original body, chunk markers, and trailers
    assert_eq!(
        HeaderValue::from_static("71"),
        content_length,
        "content-length was wrong"
    );

    let body = collect_body_into_string(req.into_body()).await;
    // When sending a streaming body with a checksum, the trailers are included as part of the body content
    assert_eq!(
        body.as_str(),
        "B\r\nHello world\r\n0\r\nx-amz-checksum-sha1:e1AsOh9IyGCa4hLN+2Od7jlnP14=\r\n\r\n"
    );
}

#[tokio::test]
async fn test_sha256_checksum_on_streaming_request() {
    let creds = aws_sdk_s3::Credentials::new(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
        None,
        "test",
    );
    let conf = aws_sdk_s3::Config::builder()
        .credentials_provider(creds)
        .region(aws_sdk_s3::Region::new("us-east-1"))
        .build();
    let (conn, rcvr) = capture_request(None);

    let client: aws_smithy_client::Client<_, aws_sdk_s3::middleware::DefaultMiddleware> =
        aws_smithy_client::Client::new(conn.clone());

    let input_text = b"Hello world";
    let mut op = PutObject::builder()
        .bucket("test-bucket")
        .key("test.txt")
        .body(aws_sdk_s3::types::ByteStream::from_static(input_text))
        .checksum_algorithm(ChecksumAlgorithm::Sha256)
        .build()
        .unwrap()
        .make_operation(&conf)
        .await
        .expect("failed to construct operation");
    op.properties_mut()
        .insert(UNIX_EPOCH + Duration::from_secs(1624036048));
    op.properties_mut().insert(AwsUserAgent::for_tests());

    // The response from the fake connection won't return the expected XML but we don't care about
    // that error in this test
    let _ = client.call(op).await;
    let req = rcvr.expect_request();

    let headers = req.headers();
    let x_amz_content_sha256 = headers
        .get("x-amz-content-sha256")
        .expect("x-amz-content-sha256 header exists");
    let x_amz_trailer = headers
        .get("x-amz-trailer")
        .expect("x-amz-trailer header exists");
    let x_amz_decoded_content_length = headers
        .get("x-amz-decoded-content-length")
        .expect("x-amz-decoded-content-length header exists");
    let content_length = headers
        .get("Content-Length")
        .expect("Content-Length header exists");
    let content_encoding = headers
        .get("Content-Encoding")
        .expect("Content-Encoding header exists");

    assert_eq!(
        HeaderValue::from_static("STREAMING-UNSIGNED-PAYLOAD-TRAILER"),
        x_amz_content_sha256,
        "signing header is incorrect"
    );
    assert_eq!(
        HeaderValue::from_static("x-amz-checksum-sha256"),
        x_amz_trailer,
        "x-amz-trailer is incorrect"
    );
    assert_eq!(
        HeaderValue::from_static(aws_http::content_encoding::header_value::AWS_CHUNKED),
        content_encoding,
        "content-encoding wasn't set to aws-chunked"
    );

    // The length of the string "Hello world"
    assert_eq!(
        HeaderValue::from_static("11"),
        x_amz_decoded_content_length,
        "decoded content length was wrong"
    );
    // The sum of the length of the original body, chunk markers, and trailers
    assert_eq!(
        HeaderValue::from_static("89"),
        content_length,
        "content-length was wrong"
    );

    let body = collect_body_into_string(req.into_body()).await;
    // When sending a streaming body with a checksum, the trailers are included as part of the body content
    assert_eq!(body.as_str(), "B\r\nHello world\r\n0\r\nx-amz-checksum-sha256:ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=\r\n\r\n");
}

async fn collect_body_into_string(mut body: aws_smithy_http::body::SdkBody) -> String {
    use bytes_utils::SegmentedBuf;
    use http_body::Body;

    let mut output = SegmentedBuf::new();
    while let Some(buf) = body.data().await {
        output.push(buf.unwrap());
    }

    let mut output_text = String::new();
    output
        .reader()
        .read_to_string(&mut output_text)
        .expect("Doesn't cause IO errors");

    output_text
}
