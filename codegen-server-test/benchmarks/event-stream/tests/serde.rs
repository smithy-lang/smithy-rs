// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use event_stream_benchmark::{serde, Case, Direction, Kind, Protocol};

#[test]
fn direct_legacy_schema_and_multi_serde_match_independent_event_fixtures() {
    for protocol in Protocol::ALL {
        for kind in Kind::ALL {
            for (size, count) in [(0, 1), (64, 3), (1024, 8), (65536, 2)] {
                let case = Case {
                    protocol,
                    direction: Direction::Upload,
                    kind,
                    size,
                    count,
                };
                match protocol {
                    Protocol::RestJson => {
                        serde::rest_json_legacy::fixture(case).validate();
                        serde::rest_json_schema::fixture(case).validate();
                        serde::multi_rest_json_schema::fixture(case).validate();
                    }
                    Protocol::RpcV2Cbor => {
                        serde::cbor_legacy::fixture(case).validate();
                        serde::cbor_schema::fixture(case).validate();
                        serde::multi_schema::fixture(case).validate();
                    }
                }
            }
        }
    }
}
