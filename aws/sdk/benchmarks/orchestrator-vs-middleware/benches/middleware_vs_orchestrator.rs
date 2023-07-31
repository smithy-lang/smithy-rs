/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[macro_use]
extern crate criterion;
use aws_sdk_s3 as s3;
use criterion::{BenchmarkId, Criterion};

macro_rules! test_connection {
    ($package:ident) => {
        $package::test_connection::infallible_connection_fn(|req| {
            assert_eq!(
                "https://test-bucket.s3.us-east-1.amazonaws.com/?list-type=2&prefix=prefix~",
                req.uri().to_string()
            );
            assert!(req.headers().contains_key("authorization"));
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

async fn orchestrator(client: &s3::Client) {
    let _output = client
        .list_objects_v2()
        .bucket("test-bucket")
        .prefix("prefix~")
        .send()
        .await
        .expect("successful execution");
}

async fn middleware(client: &middleware_s3::Client) {
    client
        .list_objects_v2()
        .bucket("test-bucket")
        .prefix("prefix~")
        .send()
        .await
        .expect("successful execution");
}

fn bench(c: &mut Criterion) {
    let orchestrator_client = {
        let conn = test_connection!(aws_smithy_client);
        let config = aws_sdk_s3::Config::builder()
            .credentials_provider(aws_sdk_s3::config::Credentials::for_tests())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .http_connector(conn.clone())
            .build();
        aws_sdk_s3::Client::from_conf(config)
    };
    let middleware_client = {
        let conn = test_connection!(middleware_smithy_client);
        let config = middleware_s3::Config::builder()
            .credentials_provider(middleware_s3::config::Credentials::for_tests())
            .region(middleware_s3::config::Region::new("us-east-1"))
            .http_connector(conn.clone())
            .build();
        middleware_s3::Client::from_conf(config)
    };

    let mut group = c.benchmark_group("compare");
    let param = "S3 ListObjectsV2";
    group.bench_with_input(
        BenchmarkId::new("middleware (last_release)", param),
        param,
        |b, _| {
            b.to_async(tokio::runtime::Runtime::new().unwrap())
                .iter(|| async { middleware(&middleware_client).await })
        },
    );
    group.bench_with_input(BenchmarkId::new("orchestrator", param), param, |b, _| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { orchestrator(&orchestrator_client).await })
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
