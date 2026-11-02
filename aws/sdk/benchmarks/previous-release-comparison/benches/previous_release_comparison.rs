/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[macro_use]
extern crate criterion;
use criterion::{BenchmarkId, Criterion};

const LIST_OBJECTS_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
    <ListBucketResult>
        <Name>test-bucket</Name>
        <Prefix>prefix~</Prefix>
        <KeyCount>1</KeyCount>
        <MaxKeys>1000</MaxKeys>
        <IsTruncated>false</IsTruncated>
        <Contents>
            <Key>some-file.file</Key>
            <LastModified>2009-10-12T17:50:30.000Z</LastModified>
            <Size>434234</Size>
            <StorageClass>STANDARD</StorageClass>
        </Contents>
    </ListBucketResult>"#;

macro_rules! test_client {
    (previous) => {
        test_client!(@internal previous_http_client)
    };
    (current) => {
        test_client!(@internal aws_smithy_http_client)
    };
    (@internal $crate_path:ident) => {
        $crate_path::test_util::infallible_client_fn(|req| {
            assert_eq!(
                "https://test-bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=prefix~",
                req.uri().to_string()
            );
            http::Response::builder()
                .status(200)
                .body(LIST_OBJECTS_RESPONSE)
                .unwrap()
        })
    };
}

macro_rules! test {
    (previous, $client:ident) => {
        test!(@internal, $client)
    };
    (current, $client:ident) => {
        test!(@internal, $client)
    };
    (@internal, $client:ident) => {
        $client
            .list_objects_v2()
            .bucket("test-bucket")
            .prefix("prefix~")
            .send()
            .await
            .expect("successful execution")
    };
}

fn bench(c: &mut Criterion) {
    let current_client = {
        let http_client = test_client!(current);
        let config = aws_sdk_s3::Config::builder()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .credentials_provider(aws_sdk_s3::config::Credentials::for_tests())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .http_client(http_client)
            .build();
        aws_sdk_s3::Client::from_conf(config)
    };
    let previous_client = {
        let http_client = test_client!(previous);
        let config = previous_s3::Config::builder()
            .behavior_version(previous_s3::config::BehaviorVersion::latest())
            .credentials_provider(previous_s3::config::Credentials::for_tests())
            .region(previous_s3::config::Region::new("us-east-1"))
            .http_client(http_client)
            .build();
        previous_s3::Client::from_conf(config)
    };

    let mut group = c.benchmark_group("compare");
    let param = "S3 ListObjectsV2";
    group.bench_with_input(BenchmarkId::new("previous", param), param, |b, _| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { std::hint::black_box(test!(previous, previous_client)) })
    });
    group.bench_with_input(BenchmarkId::new("current", param), param, |b, _| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { std::hint::black_box(test!(current, current_client)) })
    });
    group.finish();

    get_object(c);
}

const GET_OBJECT_BODY: &str = "Hello world";
/// The CRC32 of `GET_OBJECT_BODY`, in base64.
const GET_OBJECT_CRC32: &str = "i9aeUg==";

macro_rules! get_object_client {
    ($sdk:ident, $http_client:ident) => {{
        let http_client = $http_client::test_util::infallible_client_fn(|_req| {
            http::Response::builder()
                .status(200)
                .header("content-length", "11")
                .header("x-amz-checksum-crc32", GET_OBJECT_CRC32)
                .body(GET_OBJECT_BODY)
                .unwrap()
        });
        let config = $sdk::Config::builder()
            .behavior_version($sdk::config::BehaviorVersion::latest())
            .credentials_provider($sdk::config::Credentials::for_tests())
            .region($sdk::config::Region::new("us-east-1"))
            .http_client(http_client)
            .build();
        $sdk::Client::from_conf(config)
    }};
}

macro_rules! get_object {
    ($sdk:ident, $client:ident) => {
        $client
            .get_object()
            .bucket("test-bucket")
            .key("test.txt")
            .checksum_mode($sdk::types::ChecksumMode::Enabled)
    };
}

/// A validated GetObject with nothing attached, in the previous release and in this build, and
/// in this build with a checksum capture attached.
fn get_object(c: &mut Criterion) {
    use aws_smithy_observability::unit_of_work::{Capture, ChecksumCapture};

    let previous_client = get_object_client!(previous_s3, previous_http_client);
    let current_client = get_object_client!(aws_sdk_s3, aws_smithy_http_client);

    let mut group = c.benchmark_group("get_object");
    group.bench_function("previous", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let output = get_object!(previous_s3, previous_client)
                    .send()
                    .await
                    .unwrap();
                std::hint::black_box(output.body.collect().await.unwrap())
            })
    });
    group.bench_function("current", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let output = get_object!(aws_sdk_s3, current_client)
                    .send()
                    .await
                    .unwrap();
                std::hint::black_box(output.body.collect().await.unwrap())
            })
    });
    group.bench_function("current with a checksum capture", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let checksum = ChecksumCapture::new();
                let output = get_object!(aws_sdk_s3, current_client)
                    .customize()
                    .interceptor(Capture::new().with(&checksum))
                    .send()
                    .await
                    .unwrap();
                let body = output.body.collect().await.unwrap();
                std::hint::black_box((body, checksum.try_outcome()))
            })
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
