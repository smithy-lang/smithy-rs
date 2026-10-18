// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use aws_smithy_types::Blob;
#[cfg(feature = "server")]
use bench_sdk::model::{BinaryEvent, Events, Nested, StructuredData, StructuredEvent};
#[cfg(feature = "client")]
use bench_sdk::types::{BinaryEvent, Events, Nested, StructuredData, StructuredEvent};

pub const METADATA: &str = "event-stream-benchmark-v1";

#[cfg(feature = "client")]
pub fn event(kind: &str, size: usize, sequence: usize) -> Events {
    if kind == "binary" {
        Events::Binary(
            BinaryEvent::builder()
                .sequence(sequence as i64)
                .payload(Blob::new(vec![(sequence % 251) as u8; size]))
                .build(),
        )
    } else {
        assert_eq!(kind, "structured");
        Events::Structured(
            StructuredEvent::builder()
                .sequence(sequence as i64)
                .payload(
                    StructuredData::builder()
                        .text("x".repeat(size))
                        .nested(Nested::builder().label(METADATA).enabled(true).build())
                        .set_values(Some((0..8).collect()))
                        .build(),
                )
                .build(),
        )
    }
}

#[cfg(feature = "server")]
pub fn event(kind: &str, size: usize, sequence: usize) -> Events {
    if kind == "binary" {
        Events::Binary(
            BinaryEvent::builder()
                .sequence(Some(sequence as i64))
                .payload(Some(Blob::new(vec![(sequence % 251) as u8; size])))
                .build(),
        )
    } else {
        assert_eq!(kind, "structured");
        Events::Structured(
            StructuredEvent::builder()
                .sequence(Some(sequence as i64))
                .payload(Some(
                    StructuredData::builder()
                        .text(Some("x".repeat(size)))
                        .nested(Some(
                            Nested::builder()
                                .label(Some(METADATA.into()))
                                .enabled(Some(true))
                                .build(),
                        ))
                        .values(Some((0..8).collect()))
                        .build(),
                ))
                .build(),
        )
    }
}

#[allow(unreachable_patterns)]
pub fn verify(value: &Events, kind: &str, size: usize, sequence: usize) {
    match value {
        Events::Binary(v) => {
            assert_eq!(kind, "binary");
            assert_eq!(v.sequence, Some(sequence as i64));
            let payload = v.payload.as_ref().unwrap();
            assert_eq!(payload.as_ref().len(), size);
            assert!(payload
                .as_ref()
                .iter()
                .all(|b| *b == (sequence % 251) as u8));
        }
        Events::Structured(v) => {
            assert_eq!(kind, "structured");
            assert_eq!(v.sequence, Some(sequence as i64));
            let payload = v.payload.as_ref().unwrap();
            let text = payload.text.as_ref().unwrap();
            assert_eq!(text.len(), size);
            assert!(text.bytes().all(|b| b == b'x'));
            let nested = payload.nested.as_ref().unwrap();
            assert_eq!(nested.label.as_deref(), Some(METADATA));
            assert_eq!(nested.enabled, Some(true));
            assert_eq!(
                payload.values.as_ref().unwrap(),
                &(0..8).collect::<Vec<i32>>()
            );
        }
        _ => panic!("unexpected event variant: {value:?}"),
    }
}
