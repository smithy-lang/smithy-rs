/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{Case, Kind, Server, METADATA};
use aws_smithy_types::Blob;
use std::sync::Arc;
use tower::util::BoxCloneService;

// All generated SDKs have different Rust types. Compile identical handler logic for each.
macro_rules! generated_server {
    ($module:ident, $sdk:ident) => {
        pub(crate) mod $module {
            use super::*;
            use ::$sdk::{error, input, model, output, BenchmarkService, BenchmarkServiceConfig};

            fn event(case: Case, sequence: usize) -> model::Events {
                match case.kind {
                    Kind::Binary => model::Events::Binary(
                        model::BinaryEvent::builder()
                            .sequence(Some(sequence as i64))
                            .payload(Some(Blob::new(vec![(sequence % 251) as u8; case.size])))
                            .build(),
                    ),
                    Kind::Structured => model::Events::Structured(
                        model::StructuredEvent::builder()
                            .sequence(Some(sequence as i64))
                            .payload(Some(
                                model::StructuredData::builder()
                                    .text(Some("x".repeat(case.size)))
                                    .nested(Some(
                                        model::Nested::builder()
                                            .label(Some(METADATA.into()))
                                            .enabled(Some(true))
                                            .build(),
                                    ))
                                    .values(Some((0..8).collect()))
                                    .build(),
                            ))
                            .build(),
                    ),
                }
            }

            fn event_bytes(event: &model::Events, index: usize) -> usize {
                match event {
                    model::Events::Binary(value) => {
                        assert_eq!(value.sequence, Some(index as i64));
                        value
                            .payload
                            .as_ref()
                            .expect("binary payload")
                            .as_ref()
                            .len()
                    }
                    model::Events::Structured(value) => {
                        assert_eq!(value.sequence, Some(index as i64));
                        value
                            .payload
                            .as_ref()
                            .expect("structured payload")
                            .text
                            .as_ref()
                            .expect("text")
                            .len()
                    }
                }
            }

            pub(crate) fn server(case: Case) -> Server {
                // Fixtures and service construction stay outside the measurement.
                // Download clones owned modeled events lazily, identically on both paths.
                let output_events: Arc<[model::Events]> =
                    (0..case.count).map(|index| event(case, index)).collect();
                let app = BenchmarkService::builder(BenchmarkServiceConfig::builder().build())
                    .upload(|mut input: input::UploadInput| async move {
                        let mut count = 0;
                        let mut bytes = 0;
                        while let Some(value) = input.events.recv().await.expect("valid upload") {
                            let size = event_bytes(&value, count);
                            assert_eq!(size, input.size as usize);
                            bytes += size;
                            count += 1;
                        }
                        assert_eq!(count, input.count as usize);
                        Ok::<_, error::UploadError>(
                            output::UploadOutput::builder()
                                .count(count as i32)
                                .bytes(bytes as i64)
                                .build()
                                .expect("upload output"),
                        )
                    })
                    .download(move |input: input::DownloadInput| {
                        let events = output_events.clone();
                        async move {
                            assert_eq!(input.count as usize, events.len());
                            let count = events.len() + usize::from(input.fail);
                            let stream = futures_util::stream::iter(0..count).map(move |index| {
                                if index < events.len() {
                                    Ok(events[index].clone())
                                } else {
                                    Err(error::EventsError::StreamFailure(
                                        error::StreamFailure::builder()
                                            .message(Some("expected stream failure".into()))
                                            .build(),
                                    ))
                                }
                            });
                            Ok::<_, error::DownloadError>(
                                output::DownloadOutput::builder()
                                    .metadata(METADATA.into())
                                    .events(stream.into())
                                    .build()
                                    .expect("download output"),
                            )
                        }
                    })
                    .echo(|input: input::EchoInput| async move {
                        let stream = futures_util::stream::unfold(
                            (input, 0),
                            |(mut input, count)| async move {
                                match input.events.recv().await.expect("valid echo") {
                                    Some(value) => {
                                        assert_eq!(event_bytes(&value, count), input.size as usize);
                                        Some((Ok(value), (input, count + 1)))
                                    }
                                    None => {
                                        assert_eq!(count, input.count as usize);
                                        None
                                    }
                                }
                            },
                        );
                        Ok::<_, error::EchoError>(
                            output::EchoOutput::builder()
                                .metadata(METADATA.into())
                                .events(stream.into())
                                .build()
                                .expect("echo output"),
                        )
                    })
                    .build()
                    .expect("bound handlers");
                BoxCloneService::new(app)
            }

            use futures_util::StreamExt;
        }
    };
}

generated_server!(rest_json_legacy, rest_json_legacy);
generated_server!(rest_json_schema, rest_json_schema);
generated_server!(cbor_legacy, cbor_legacy);
generated_server!(cbor_schema, cbor_schema);
generated_server!(multi_schema, multi_schema);
generated_server!(multi_rest_json_schema, multi_rest_json_schema);
