/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_credential_types::Credentials;
use aws_sigv4::http_request::{SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4a;
use aws_smithy_runtime_api::client::identity::Identity;
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use std::time::{Duration, SystemTime};

const ACCESS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
const SECRET_KEY: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

pub fn generate_signing_key(c: &mut Criterion) {
    c.bench_function("generate_signing_key", |b| {
        b.iter(|| {
            let _ = v4a::generate_signing_key(black_box(ACCESS_KEY), black_box(SECRET_KEY));
        })
    });
}

/// Signing given an already-derived key, which is the half that does the ECDSA work.
pub fn calculate_signature(c: &mut Criterion) {
    let key = v4a::generate_signing_key(ACCESS_KEY, SECRET_KEY);
    let string_to_sign =
        b"AWS4-ECDSA-P256-SHA256\n20260101T000000Z\n20260101/lambda/aws4_request\n\
                           a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
    c.bench_function("calculate_signature", |b| {
        b.iter(|| {
            let _ = v4a::calculate_signature(black_box(&key), black_box(string_to_sign));
        })
    });
}

/// The whole public entry point, which is what a caller actually pays per request.
pub fn sign_http_request(c: &mut Criterion) {
    let identity: Identity = Credentials::new(ACCESS_KEY, SECRET_KEY, None, None, "bench").into();
    c.bench_function("sign_http_request", |b| {
        b.iter(|| {
            let params: v4a::SigningParams<'_, SigningSettings> = v4a::SigningParams::builder()
                .identity(&identity)
                .region_set("*")
                .name("lambda")
                .time(SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_331_200))
                .settings(SigningSettings::default())
                .build()
                .unwrap();
            let request = SignableRequest::new(
                "POST",
                "https://abc.lambda-url.us-east-1.on.aws/path?x=1",
                [
                    ("host", "abc.lambda-url.us-east-1.on.aws"),
                    ("content-type", "application/json"),
                ]
                .into_iter(),
                SignableBody::Bytes(b"{\"hello\":\"world\"}"),
            )
            .unwrap();
            let _ = aws_sigv4::http_request::sign(black_box(request), &params.into()).unwrap();
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = generate_signing_key, calculate_signature, sign_http_request
}
criterion_main!(benches);
