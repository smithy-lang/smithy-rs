/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// True for bytes matching the `Word` production: `[A-Za-z0-9_]`.
#[inline(always)]
fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Matches the `Identifier` ABNF rule: `((_+ (Alpha / Digit)) | Alpha) Word*`.
///
/// Callers must already have established that every byte of `s` is a `Word`
/// byte; this function only checks the leading-character rule.
#[inline]
fn is_valid_identifier_prefix(s: &[u8]) -> bool {
    let underscores = s.iter().take_while(|&&c| c == b'_').count();
    if underscores == s.len() {
        return false; // empty, or nothing but underscores
    }
    if underscores == 0 {
        s[0].is_ascii_alphabetic()
    } else {
        true // `_+` followed by a word byte, which is Alpha or Digit
    }
}

/// A successfully parsed RPC v2 request URI path.
#[derive(Debug, PartialEq, Eq)]
pub struct RpcV2Path<'a> {
    /// The service shape name (final identifier, namespace stripped).
    pub service: &'a str,
    /// The operation shape name.
    pub operation: &'a str,
    /// The verbatim path tail `"{service}/operation/{operation}"`, suitable as
    /// a zero-copy router lookup key. Borrowed directly from the request path:
    /// no composition, no copies, no allocation.
    pub route_key: &'a str,
}

/// Extracts the service name, operation name, and route key from an RPC v2
/// request URI path.
///
/// Returns `None` if the path does not end in
/// `/service/{serviceName}/operation/{operationName}` with valid identifiers.
/// When `serviceName` is namespace-qualified (`com.example.TheService`), only
/// the final identifier (`TheService`) is used, so `route_key` for
/// `/service/com.example.TheService/operation/Op` is `"TheService/operation/Op"`,
/// matching the generated route keys.
pub fn parse_rpc_v2_path(path: &str) -> Option<RpcV2Path<'_>> {
    const OPERATION: &[u8] = b"/operation"; // 10 bytes
    const SERVICE: &[u8] = b"/service"; // 8 bytes; its '/' is the segment separator

    let b = path.as_bytes();

    // 1. Operation name: scan backwards from the end to the last '/'. The
    //    grammar is anchored at the end of the path, so any non-Word byte here
    //    means no window of the path can match: reject immediately.
    let mut pos = b.len();
    let op_slash = loop {
        if pos == 0 {
            return None; // no '/' at all, or path is empty
        }
        pos -= 1;
        match b[pos] {
            b'/' => break pos,
            c if is_word(c) => {}
            _ => return None,
        }
    };
    let operation = &b[op_slash + 1..];
    if !is_valid_identifier_prefix(operation) {
        return None; // empty ("…/operation/") or bad leading character
    }

    // 2. The 10 bytes before that slash must be exactly "/operation".
    //    (Compiles to two word-sized compares; no per-byte loop.)
    if op_slash < OPERATION.len() || &b[op_slash - OPERATION.len()..op_slash] != OPERATION {
        return None;
    }
    let name_end = op_slash - OPERATION.len();

    // 3. Service name segment: scan backwards to the previous '/'. Only Word
    //    bytes and '.' (namespace separators) may appear.
    pos = name_end;
    let name_slash = loop {
        if pos == 0 {
            return None;
        }
        pos -= 1;
        match b[pos] {
            b'/' => break pos,
            c if is_word(c) || c == b'.' => {}
            _ => return None,
        }
    };
    let name = &b[name_slash + 1..name_end];
    if name.is_empty() {
        return None; // "/service//operation/Foo"
    }

    // 4. The 8 bytes ending at (and including the byte before) `name_slash`
    //    must be "/service"; together with `name_slash` itself this forms the
    //    literal "/service/". Anything may precede it (the ignored prefix).
    if name_slash < SERVICE.len() || &b[name_slash - SERVICE.len()..name_slash] != SERVICE {
        return None;
    }

    // 5. Validate each dot-separated namespace segment as an Identifier. The
    //    service name is the final segment, mirroring the regex's greedy
    //    `({ID}\.)*({ID})` capture.
    let mut seg_start = 0usize;
    for (i, &c) in name.iter().enumerate() {
        if c == b'.' {
            if !is_valid_identifier_prefix(&name[seg_start..i]) {
                return None; // e.g. ".Service" or "a..b"
            }
            seg_start = i + 1;
        }
    }
    if !is_valid_identifier_prefix(&name[seg_start..]) {
        return None; // trailing dot, or invalid final identifier
    }

    let service_start = name_slash + 1 + seg_start;
    Some(RpcV2Path {
        service: &path[service_start..name_end],
        operation: &path[op_slash + 1..],
        // The tail of the path from the service name onward is exactly
        // "{service}/operation/{operation}" and already contiguous in the
        // request buffer, so the lookup key is a borrow, not a construction.
        route_key: &path[service_start..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors ported from the existing `rpc_v2_cbor::router` regex tests.

    #[test]
    fn uri_parse_accepts() {
        for uri in [
            "/service/Service/operation/Operation",
            "prefix/69/service/Service/operation/Operation",
            // Prefix runs up to the last occurrence of `/service`.
            "prefix/69/service/Service/operation/Operation/service/Service/operation/Operation",
            // Absolute shape IDs with `#` replaced by `.` are accepted, and only
            // the shape name is captured.
            "/service/aws.protocoltests.rpcv2Cbor.Service/operation/Operation",
            "/service/namespace.Service/operation/Operation",
        ] {
            let p =
                parse_rpc_v2_path(uri).unwrap_or_else(|| panic!("uri incorrectly rejected: {uri}"));
            assert_eq!("Service", p.service, "uri: {uri}");
            assert_eq!("Operation", p.operation, "uri: {uri}");
            assert_eq!("Service/operation/Operation", p.route_key, "uri: {uri}");
        }
    }

    #[test]
    fn uri_parse_rejects() {
        for uri in [
            "",
            "foo",
            "/servicee/Service/operation/Operation",
            "/service/Service",
            "/service/Service/operation/",
            "/service/Service/operation/Operation/",
            "/service/Service/operation/Operation/invalid-suffix",
            "/service/namespace.foo#Service/operation/Operation",
            "/service/namespace-Service/operation/Operation",
            "/service/.Service/operation/Operation",
        ] {
            assert!(
                parse_rpc_v2_path(uri).is_none(),
                "uri incorrectly accepted: {uri}"
            );
        }
    }

    #[test]
    fn valid_identifiers() {
        for id in [
            "a",
            "_a",
            "_0",
            "__0",
            "variable123",
            "_underscored_variable",
        ] {
            assert!(
                parse_rpc_v2_path(&format!("/service/{id}/operation/{id}")).is_some(),
                "'{id}' is incorrectly rejected"
            );
        }
    }

    #[test]
    fn invalid_identifiers() {
        for id in [
            "0",
            "123starts_with_digit",
            "@invalid_start_character",
            " space_in_identifier",
            "invalid-character",
            "invalid@character",
            "no#hashes",
            "_",
        ] {
            assert!(
                parse_rpc_v2_path(&format!("/service/{id}/operation/Op")).is_none(),
                "service '{id}' is incorrectly accepted"
            );
            assert!(
                parse_rpc_v2_path(&format!("/service/Svc/operation/{id}")).is_none(),
                "operation '{id}' is incorrectly accepted"
            );
        }
    }

    #[test]
    fn namespace_edge_cases() {
        // service name is the final identifier
        let p = parse_rpc_v2_path("/service/com.example.TheService/operation/Op").unwrap();
        assert_eq!(p.service, "TheService");
        assert_eq!(p.operation, "Op");
        // namespace is excluded from the zero-copy key
        assert_eq!(p.route_key, "TheService/operation/Op");
        // every namespace segment is validated
        assert!(parse_rpc_v2_path("/service/com..TheService/operation/Op").is_none());
        assert!(parse_rpc_v2_path("/service/com.example./operation/Op").is_none());
        assert!(parse_rpc_v2_path("/service/0com.TheService/operation/Op").is_none());
    }

    #[test]
    fn route_key_is_a_borrow_of_the_path() {
        let path = "/api/v1/service/SprocketService/operation/GetSprocket".to_string();
        let p = parse_rpc_v2_path(&path).unwrap();
        assert_eq!(p.route_key, "SprocketService/operation/GetSprocket");
        // same memory, not a copy: the key points into the request path buffer
        let path_range = path.as_ptr() as usize..path.as_ptr() as usize + path.len();
        assert!(path_range.contains(&(p.route_key.as_ptr() as usize)));
    }
}
