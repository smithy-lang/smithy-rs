/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// The consumer-shaped app. `cargo build` compiles exactly this and its [dependencies],
// which is what owns COMPILE_FAIL -- no test-only crate is reachable from here.
//
// The hermetic assertions live in tests/hermetic.rs and run under `cargo test`, which
// is where test-util is allowed.

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_smithy_types::{Document, Number};
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    check_document_roundtrip();
    check_client_constructs();
    println!("ALL CHECKS PASSED");
}

/// September's exact surface: `Document`'s object variant changed shape in
/// aws-smithy-types 1.7.0. Touch it so a graph that resolves an incoherent pair
/// fails here rather than in a customer's build.
fn check_document_roundtrip() {
    let mut map = HashMap::new();
    map.insert("n".to_string(), Document::Number(Number::PosInt(42)));
    map.insert("s".to_string(), Document::String("hello".into()));
    // NOTE: pass the `HashMap` *directly*. This is the construct that failed with
    // `error[E0308]` on the yanked aws-smithy-types 1.7.0, which wrapped the object
    // in a `DocumentObject` newtype. Writing `map.into()` here would be an identity
    // conversion today AND would have compiled against 1.7.0 -- masking the break.
    let doc = Document::Object(map);
    let Document::Object(obj) = &doc else {
        panic!("expected object variant");
    };
    assert_eq!(obj.len(), 2, "object lost entries");
    match obj.get("s") {
        Some(Document::String(s)) => assert_eq!(s, "hello"),
        other => panic!("unexpected value for `s`: {other:?}"),
    }
    println!("ok: Document object round-trip");
}

/// Construct a real client. Needs no network and no test-util, but it links the
/// config, credential, endpoint and orchestrator types together -- which is what
/// makes a duplicate-major graph observable at build time.
fn check_client_constructs() {
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "AKIDCANARY",
            "secret",
            None,
            None,
            "canary",
        ))
        .region(Region::new("us-east-1"))
        .build();
    let _client = aws_sdk_s3::Client::from_conf(config);
    println!("ok: S3 client constructed from config");
}
