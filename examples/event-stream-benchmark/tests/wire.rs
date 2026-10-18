/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use event_stream_benchmark::{
    drain, server, Case, Direction, Fixture, Implementation, Kind, Protocol,
};

#[test]
fn generated_sdk_paths_match_the_benchmark_labels() {
    macro_rules! check_sdk {
        ($projection:literal, $schema:literal) => {
            let serde = include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../codegen-server-test/build/smithyprojections/codegen-server-test/",
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
                "/../../codegen-server-test/build/smithyprojections/codegen-server-test/",
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
