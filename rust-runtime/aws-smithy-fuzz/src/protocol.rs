/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use crate::HttpRequest;

/// Pins protocol identity for a differential campaign. Body bytes and operation
/// selectors remain fuzzable. Protocol-selection mutations belong in routing tests.
pub fn pin_protocol(request: &mut HttpRequest, protocol: &str) {
    let content_type = match protocol {
        "aws-json-10" => "application/x-amz-json-1.0",
        "aws-json-11" => "application/x-amz-json-1.1",
        "rest-json1" => "application/json",
        "rest-xml" | "rest-xml-nojson" => "application/xml",
        "rpcv2-cbor" => "application/cbor",
        _ => panic!("unknown isolated fuzz protocol: {protocol}"),
    };
    // CBOR has an unambiguous protocol header even for event-stream requests, and the REST
    // protocols identify event-stream inputs by this content type, so those requests keep it.
    // AWS JSON uses its versioned content type to disambiguate event streams.
    let event_stream = matches!(
        protocol,
        "rpcv2-cbor" | "rest-json1" | "rest-xml" | "rest-xml-nojson"
    ) && request.headers.iter().any(|(name, values)| {
        name.eq_ignore_ascii_case("content-type")
            && values.iter().any(|value| {
                value.split(';').next().unwrap_or("").trim() == "application/vnd.amazon.eventstream"
            })
    });
    for headers in [&mut request.headers, &mut request.trailers] {
        headers.retain(|name, _| {
            !name.eq_ignore_ascii_case("content-type")
                && !name.eq_ignore_ascii_case("smithy-protocol")
                && !name.eq_ignore_ascii_case("content-encoding")
                && (protocol.starts_with("aws-json") || !name.eq_ignore_ascii_case("x-amz-target"))
        });
    }
    request.headers.insert(
        "content-type".into(),
        vec![if event_stream {
            "application/vnd.amazon.eventstream"
        } else {
            content_type
        }
        .into()],
    );
    if protocol == "rpcv2-cbor" {
        request
            .headers
            .insert("smithy-protocol".into(), vec!["rpc-v2-cbor".into()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_cannot_leak_between_campaigns() {
        for (protocol, expected) in [
            ("aws-json-10", "application/x-amz-json-1.0"),
            ("aws-json-11", "application/x-amz-json-1.1"),
            ("rest-json1", "application/json"),
            ("rest-xml", "application/xml"),
            ("rest-xml-nojson", "application/xml"),
            ("rpcv2-cbor", "application/cbor"),
        ] {
            let mut request = HttpRequest {
                uri: "/test".into(),
                method: "POST".into(),
                body: b"unaltered payload".to_vec(),
                ..Default::default()
            };
            for key in [
                "Content-Type",
                "content-type",
                "SMITHY-PROTOCOL",
                "Content-Encoding",
                "X-Amz-Target",
            ] {
                request
                    .headers
                    .insert(key.into(), vec!["other protocol".into()]);
                request
                    .trailers
                    .insert(key.into(), vec!["other protocol".into()]);
            }
            request
                .headers
                .insert("accept".into(), vec!["fuzzed".into()]);
            pin_protocol(&mut request, protocol);
            assert_eq!(request.headers["content-type"], [expected]);
            assert!(!request.headers.contains_key("Content-Type"));
            assert!(!request.headers.contains_key("SMITHY-PROTOCOL"));
            assert!(!request.headers.contains_key("Content-Encoding"));
            assert_eq!(
                request.headers.contains_key("X-Amz-Target"),
                protocol.starts_with("aws-json")
            );
            assert_eq!(
                request.headers.contains_key("smithy-protocol"),
                protocol == "rpcv2-cbor"
            );
            assert_eq!(request.headers["accept"], ["fuzzed"]);
            assert_eq!(request.body, b"unaltered payload");
            assert_eq!(request.uri, "/test");
            let once = request.clone();
            pin_protocol(&mut request, protocol);
            assert!(request == once);
        }
    }

    #[test]
    fn event_stream_content_type_survives_everywhere_but_aws_json() {
        for protocol in [
            "aws-json-10",
            "aws-json-11",
            "rest-json1",
            "rest-xml",
            "rest-xml-nojson",
            "rpcv2-cbor",
        ] {
            let mut request = HttpRequest::default();
            request.headers.insert(
                "content-type".into(),
                vec!["application/vnd.amazon.eventstream".into()],
            );
            pin_protocol(&mut request, protocol);
            assert_eq!(
                request.headers["content-type"][0],
                match protocol {
                    "aws-json-10" => "application/x-amz-json-1.0",
                    "aws-json-11" => "application/x-amz-json-1.1",
                    _ => "application/vnd.amazon.eventstream",
                }
            );
        }
    }
}
