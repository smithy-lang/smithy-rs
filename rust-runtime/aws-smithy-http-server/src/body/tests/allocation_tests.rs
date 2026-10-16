/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{collect_body, new_collection_buffer, CollectBodyError, CollectionConfig, DEFAULT_MAX_INITIAL_CAPACITY};
use bytes::Bytes;
use http_body::Frame;
use http_body_util::{Full, StreamBody};
use std::convert::Infallible;
use std::num::NonZeroUsize;

#[test]
fn initial_reservation_respects_the_caller_cap_and_untrusted_hints() {
    for (lower, maximum, expected_capacity) in [
        (u64::MAX, 0, 0),
        (u64::MAX, 1, 1),
        (u64::MAX, 4096, 4096),
        (u64::MAX, DEFAULT_MAX_INITIAL_CAPACITY.get(), 32 * 1024),
        (u64::MAX, 64 * 1024, 64 * 1024),
        (128, 4096, 128),
        (0, 4096, 0),
    ] {
        let buffer = new_collection_buffer(lower, maximum);
        assert_eq!(buffer.capacity(), expected_capacity);
        assert!(buffer.is_empty());
    }
}

#[tokio::test]
async fn custom_reservation_caps_allow_collection_to_grow_as_data_arrives() {
    let payload = Bytes::from(vec![b'x'; 48 * 1024]);
    for maximum in [0, 1, 64 * 1024] {
        let collected = collect_body(
            Full::new(payload.clone()),
            Some(
                CollectionConfig::builder()
                    .frame_budget(NonZeroUsize::new(3).unwrap())
                    .initial_capacity(maximum)
                    .build(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(collected, payload);
    }
}

#[tokio::test]
async fn custom_reservation_caps_preserve_body_limits_and_trailer_handling() {
    for maximum in [0, 1, 64 * 1024] {
        for limit in [5, 6] {
            let body = StreamBody::new(futures_util::stream::iter([
                Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"abc"))),
                Ok(Frame::data(Bytes::from_static(b"def"))),
                Ok(Frame::trailers(http::HeaderMap::new())),
            ]));
            let result = collect_body(
                body,
                Some(
                    CollectionConfig::builder()
                        .maybe_limit(NonZeroUsize::new(limit))
                        .frame_budget(NonZeroUsize::new(1).unwrap())
                        .initial_capacity(maximum)
                        .build(),
                ),
            )
            .await;
            if limit == 5 {
                assert!(matches!(result, Err(CollectBodyError::BodyLimitExceeded(limit)) if limit.get() == 5));
            } else {
                assert_eq!(result.unwrap(), "abcdef");
            }
        }
    }
}
