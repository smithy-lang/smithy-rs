/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The size-limited collection API that released generated servers call:
//! `body::collect_body_limited` returning `body::CollectBodyError::TooLarge`.

use crate::body::{collect_body_limited, empty, from_bytes, BodyLimitExceeded, CollectBodyError};
use bytes::Bytes;

#[tokio::test]
async fn under_limit_collects_the_body() {
    let result = collect_body_limited(from_bytes(Bytes::from("hello")), 1024).await;
    assert_eq!(result.unwrap(), Bytes::from("hello"));
}

#[tokio::test]
async fn over_limit_is_too_large() {
    let result = collect_body_limited(from_bytes(Bytes::from("this is way too long")), 5).await;
    assert!(matches!(
        result,
        Err(CollectBodyError::TooLarge(BodyLimitExceeded { limit: 5 }))
    ));
}

#[tokio::test]
async fn chunked_body_exceeding_mid_stream_is_too_large() {
    use futures_util::stream;
    let chunks = vec![
        Ok::<_, std::io::Error>(http_body::Frame::data(Bytes::from("aaaa"))),
        Ok(http_body::Frame::data(Bytes::from("bbbb"))),
    ];
    let body = http_body_util::StreamBody::new(stream::iter(chunks));
    let result = collect_body_limited(body, 6).await;
    assert!(matches!(result, Err(CollectBodyError::TooLarge(_))));
}

#[tokio::test]
async fn zero_limit_disables_the_check() {
    let result = collect_body_limited(from_bytes(Bytes::from("unbounded body")), 0).await;
    assert_eq!(result.unwrap(), Bytes::from("unbounded body"));
    assert_eq!(collect_body_limited(empty(), 100).await.unwrap(), Bytes::new());
}

#[test]
fn too_large_displays_the_limit_and_exposes_its_source() {
    let error: CollectBodyError<std::io::Error> = CollectBodyError::TooLarge(BodyLimitExceeded { limit: 7 });
    assert_eq!(
        error.to_string(),
        "request body exceeded the configured maximum of 7 bytes"
    );
    assert!(std::error::Error::source(&error).is_some());
}
