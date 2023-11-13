/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[macro_use]
extern crate criterion;
use criterion::{BenchmarkId, Criterion};

macro_rules! test_client {
    (previous) => {
        test_client!(@internal previous_runtime)
    };
    (main) => {
        test_client!(@internal aws_smithy_runtime)
    };
    (@internal $runtime_crate:ident) => {
        $runtime_crate::client::http::test_util::infallible_client_fn(|req| {
            assert_eq!(
                "https://test-bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=prefix~",
                req.uri().to_string()
            );
            http::Response::builder()
                .status(200)
                .body(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
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
                    </ListBucketResult>
                    "#,
                )
                .unwrap()
        })
    };
}

macro_rules! test {
    (previous, $client:ident) => {
        test!(@internal, $client)
    };
    (main, $client:ident) => {
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
    let main_client = {
        let http_client = test_client!(main);
        let config = aws_sdk_s3::Config::builder()
            .credentials_provider(aws_sdk_s3::config::Credentials::for_tests())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .http_client(http_client)
            .build();
        aws_sdk_s3::Client::from_conf(config)
    };
    let previous_client = {
        let http_client = test_client!(previous);
        let config = previous_s3::Config::builder()
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
            .iter(|| async { test!(previous, previous_client) })
    });
    group.bench_with_input(BenchmarkId::new("main", param), param, |b, _| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { test!(main, main_client) })
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
