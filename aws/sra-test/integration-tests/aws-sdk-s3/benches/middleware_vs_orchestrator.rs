/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#[macro_use]
extern crate criterion;
use aws_sdk_s3 as s3;
use aws_smithy_client::erase::DynConnector;
use aws_smithy_client::test_connection::infallible_connection_fn;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_runtime_api::config_bag::ConfigBag;
use criterion::Criterion;
use s3::endpoint::Params;

async fn middleware(client: &s3::Client) {
    client
        .list_objects_v2()
        .bucket("test-bucket")
        .prefix("prefix~")
        .send()
        .await
        .expect("successful execution");
}

async fn orchestrator(client: &s3::Client) {
    struct FixupPlugin {
        region: String,
    }
    impl RuntimePlugin for FixupPlugin {
        fn configure(
            &self,
            cfg: &mut ConfigBag,
        ) -> Result<(), aws_smithy_runtime_api::client::runtime_plugin::BoxError> {
            let params_builder = Params::builder()
                .set_region(Some(self.region.clone()))
                .bucket("test-bucket");

            cfg.put(params_builder);
            Ok(())
        }
    }
    let _output = client
        .list_objects_v2()
        .bucket("test-bucket")
        .prefix("prefix~")
        .send_v2_with_plugin(Some(FixupPlugin {
            region: client
                .conf()
                .region()
                .map(|c| c.as_ref().to_string())
                .unwrap(),
        }))
        .await
        .expect("successful execution");
}

fn test_connection() -> DynConnector {
    infallible_connection_fn(|req| {
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
}

fn client() -> s3::Client {
    let conn = test_connection();
    let config = s3::Config::builder()
        .credentials_provider(s3::config::Credentials::for_tests())
        .region(s3::config::Region::new("us-east-1"))
        .http_connector(conn.clone())
        .build();
    s3::Client::from_conf(config)
}

fn middleware_bench(c: &mut Criterion) {
    let client = client();
    c.bench_function("middleware", move |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { middleware(&client).await })
    });
}

fn orchestrator_bench(c: &mut Criterion) {
    let client = client();
    c.bench_function("orchestrator", move |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { orchestrator(&client).await })
    });
}

criterion_group!(benches, middleware_bench, orchestrator_bench);
criterion_main!(benches);
