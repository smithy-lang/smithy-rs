/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[macro_use]
extern crate criterion;
use aws_sdk_s3 as s3;
use aws_smithy_runtime_api::client::interceptors::InterceptorRegistrar;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::config_bag::ConfigBag;
use criterion::{BenchmarkId, Criterion};

macro_rules! test_connection {
    (head) => {
        test_connection!(aws_smithy_client)
    };
    (last_release) => {
        test_connection!(last_release_smithy_client)
    };
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

macro_rules! create_client {
    (head) => {
        create_client!(head, aws_sdk_s3)
    };
    (last_release) => {
        create_client!(last_release, last_release_s3)
    };
    ($original:ident, $package:ident) => {{
        let conn = test_connection!($original);
        let config = $package::Config::builder()
            .credentials_provider($package::config::Credentials::for_tests())
            .region($package::config::Region::new("us-east-1"))
            .http_connector(conn.clone())
            .build();
        $package::Client::from_conf(config)
    }};
}

macro_rules! middleware_bench_fn {
    ($fn_name:ident, head) => {
        middleware_bench_fn!($fn_name, aws_sdk_s3)
    };
    ($fn_name:ident, last_release) => {
        middleware_bench_fn!($fn_name, last_release_s3)
    };
    ($fn_name:ident, $package:ident) => {
        async fn $fn_name(client: &$package::Client) {
            client
                .list_objects_v2()
                .bucket("test-bucket")
                .prefix("prefix~")
                .send()
                .await
                .expect("successful execution");
        }
    };
}

async fn orchestrator(client: &s3::Client) {
    let _output = client
        .list_objects_v2()
        .bucket("test-bucket")
        .prefix("prefix~")
        .send_orchestrator()
        .await
        .expect("successful execution");
}

fn bench(c: &mut Criterion) {
    let head_client = create_client!(head);
    middleware_bench_fn!(middleware_head, head);

    let last_release_client = create_client!(last_release);
    middleware_bench_fn!(middleware_last_release, last_release);

    let mut group = c.benchmark_group("compare");
    let param = "S3 ListObjectsV2";
    group.bench_with_input(
        BenchmarkId::new("middleware (HEAD)", param),
        param,
        |b, _| {
            b.to_async(tokio::runtime::Runtime::new().unwrap())
                .iter(|| async { middleware_head(&head_client).await })
        },
    );
    group.bench_with_input(
        BenchmarkId::new("middleware (last_release)", param),
        param,
        |b, _| {
            b.to_async(tokio::runtime::Runtime::new().unwrap())
                .iter(|| async { middleware_last_release(&last_release_client).await })
        },
    );
    group.bench_with_input(BenchmarkId::new("orchestrator", param), param, |b, _| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { orchestrator(&head_client).await })
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
