/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Validates the event-stream benchmark before comparing timings. It checks that
//! implementation labels select the intended generated SDK, that in-memory HTTP/2
//! preserves payloads, and that legacy and schema services emit equivalent decoded
//! events with a modeled failure following the successful events.
//!
use event_stream_benchmark::{
    drain, hyper_in_memory, server, Case, Direction, Fixture, Implementation, Kind, Protocol,
};

#[test]
fn generated_sdk_paths_match_the_benchmark_labels() {
    macro_rules! check_sdk {
        ($projection:literal, $schema:literal) => {
            let serde = include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../build/smithyprojections/codegen-server-test/",
                $projection,
                "/rust-server-codegen/src/event_stream_serde.rs"
            ));
            assert_eq!(
                serde.contains("SchemaEventMarshaller"),
                $schema,
                $projection
            );
            assert_eq!(
                serde.contains("SchemaEventUnmarshaller"),
                $schema,
                $projection
            );
            let manifest = include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../build/smithyprojections/codegen-server-test/",
                $projection,
                "/rust-server-codegen/Cargo.toml"
            ));
            assert!(
                manifest.contains("[dependencies.http-1x]\nversion = \"1\""),
                $projection
            );
        };
    }
    check_sdk!("event-stream-bench-rest-json-legacy", false);
    check_sdk!("event-stream-bench-rest-json-schema", true);
    check_sdk!("event-stream-bench-rpc-v2-cbor-legacy", false);
    check_sdk!("event-stream-bench-rpc-v2-cbor-schema", true);
    check_sdk!("event-stream-bench-multi-schema", true);
    check_sdk!("event-stream-bench-multi-rest-json-schema", true);
}

#[test]
fn in_memory_http2_preserves_stream_payloads_and_errors() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for protocol in Protocol::ALL {
            for direction in Direction::ALL {
                for kind in Kind::ALL {
                    // Exceeds the duplex buffer and HTTP/2's initial stream window.
                    let case = Case {
                        protocol,
                        direction,
                        kind,
                        size: 65536,
                        count: 3,
                    };
                    for implementation in Implementation::ALL {
                        let service = hyper_in_memory(server(implementation, case)).await;
                        Fixture::new(case)
                            .fragmented(8191)
                            .validate(service.clone())
                            .await;
                        // Multiplex response bodies on the same connection without sockets.
                        let first = Fixture::new(case);
                        let second = Fixture::new(case);
                        futures_util::future::join(
                            first.validate(service.clone()),
                            second.validate(service.clone()),
                        )
                        .await;
                        // Reuse the connection and exercise complete body draining.
                        assert!(
                            drain(service.clone(), Fixture::new(case).request())
                                .await
                                .bytes
                                > 0
                        );
                        if direction == Direction::Download {
                            Fixture::with_error(case, true).validate(service).await;
                        }
                    }
                }
            }
        }
    });
}

#[test]
fn generated_legacy_and_schema_agree_on_real_wire_events() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for protocol in Protocol::ALL {
        for direction in Direction::ALL {
            for kind in Kind::ALL {
                for (size, count) in [(0, 1), (64, 3), (1024, 8)] {
                    let case = Case {
                        protocol,
                        direction,
                        kind,
                        size,
                        count,
                    };
                    let fixture = Fixture::new(case);
                    for implementation in Implementation::ALL {
                        let service = server(implementation, case);
                        runtime.block_on(fixture.validate(service.clone()));
                        // Input HTTP chunk boundaries need not align with event-frame boundaries.
                        runtime.block_on(fixture.fragmented(7).validate(service.clone()));
                        let consumed = runtime.block_on(drain(service, fixture.request()));
                        assert!(consumed.frames > 0);
                        assert!(consumed.bytes > 0);
                    }
                }
            }
        }
    }
}

#[test]
fn modeled_stream_error_follows_all_successful_events() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for protocol in Protocol::ALL {
        for kind in Kind::ALL {
            let case = Case {
                protocol,
                direction: Direction::Download,
                kind,
                size: 64,
                count: 2,
            };
            for implementation in Implementation::ALL {
                runtime.block_on(
                    Fixture::with_error(case, true).validate(server(implementation, case)),
                );
            }
        }
    }
}
