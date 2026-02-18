/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_credential_types::Credentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use criterion::{criterion_group, criterion_main, Criterion};
use std::time::SystemTime;

fn create_dynamodb_request() -> SignableRequest<'static> {
    // Small DynamoDB payload (few KB as mentioned by customer)
    let body = br#"{"TableName":"TestTable","Key":{"id":{"S":"test-id-123"}}}"#;

    let headers = vec![
        ("content-type", "application/x-amz-json-1.0"),
        ("x-amz-target", "DynamoDB_20120810.GetItem"),
        ("content-length", "75"),
        ("user-agent", "aws-sdk-rust/1.3.11 os/linux lang/rust/1.93.0"),
        ("x-amz-user-agent", "aws-sdk-rust/1.3.11 ua/2.1 api/dynamodb/1.104.0 os/linux lang/rust/1.93.0 exec-env/AWS_ECS_EC2 m/E,P,z md/http#hyper-1.x"),
        ("host", "dynamodb.eu-central-1.amazonaws.com"),
    ];

    SignableRequest::new(
        "POST",
        "https://dynamodb.eu-central-1.amazonaws.com/",
        headers.into_iter(),
        SignableBody::Bytes(body),
    )
    .unwrap()
}

fn bench_sign_dynamodb_request(c: &mut Criterion) {
    let credentials = Credentials::new(
        "AKIAIOSFODNN7EXAMPLE",
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        None,
        None,
        "test",
    );
    let settings = SigningSettings::default();
    let identity = &credentials.into();

    let params = v4::SigningParams::builder()
        .identity(identity)
        .region("eu-central-1")
        .name("dynamodb")
        .time(SystemTime::UNIX_EPOCH)
        .settings(settings)
        .build()
        .unwrap()
        .into();

    c.bench_function("sign_dynamodb_request", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                let req = create_dynamodb_request();
                sign(req, &params).unwrap();
            }
        })
    });
}

criterion_group!(benches, bench_sign_dynamodb_request);
criterion_main!(benches);
