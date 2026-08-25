/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

const OPERATION: &[u8] = b"/operation";
const SERVICE: &[u8] = b"/service";
const MIN_BYTES: usize = "/service/operation/".len();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RouteIdentity<'a> {
    pub(super) service: &'a str,
    pub(super) operation: &'a str,
    pub(super) route_key: &'a str,
}

/// Returns true for bytes matching the Smithy `Word` production: `[A-Za-z0-9_]`.
#[inline(always)]
pub(super) fn is_word(character: u8) -> bool {
    character.is_ascii_alphanumeric() || character == b'_'
}

/// Checks the leading-character portion of the Smithy `Identifier` production.
///
/// Callers must first establish that every byte is a `Word` byte.
#[inline]
pub(super) fn has_valid_identifier_start(identifier: &[u8]) -> bool {
    let underscores = identifier.iter().take_while(|&&character| character == b'_').count();
    // An identifier cannot consist only of underscores.
    if underscores == identifier.len() {
        return false;
    }
    underscores != 0 || identifier[0].is_ascii_alphabetic()
}

/// Parses a path ending in `/service/{service}/operation/{operation}`.
///
/// Parsing backwards avoids scanning an arbitrary prefix and allows the route lookup key to borrow
/// the contiguous `{service}/operation/{operation}` tail directly from the request URI.
pub(super) fn parse_route_identity(path: &str) -> Option<RouteIdentity<'_>> {
    let bytes = path.as_bytes();
    if bytes.len() < MIN_BYTES {
        return None;
    }

    let mut position = bytes.len() - 1;
    while position > 0 {
        if bytes[position] == b'/' {
            break;
        }
        if !is_word(bytes[position]) {
            return None;
        }
        position -= 1;
    }

    let operation_slash = position;
    let operation_start = operation_slash + 1;
    if operation_start >= bytes.len() || operation_slash < OPERATION.len() {
        return None;
    }
    if &bytes[operation_slash - OPERATION.len()..operation_slash] != OPERATION {
        return None;
    }
    if !has_valid_identifier_start(&bytes[operation_start..]) {
        return None;
    }

    let service_end = operation_slash - OPERATION.len();
    position = service_end;
    let mut segment_end = service_end;
    let mut service_start = None;
    while position > 0 {
        position -= 1;
        match bytes[position] {
            b'/' => {
                if !has_valid_identifier_start(&bytes[position + 1..segment_end]) {
                    return None;
                }
                break;
            }
            b'.' => {
                if !has_valid_identifier_start(&bytes[position + 1..segment_end]) {
                    return None;
                }
                service_start.get_or_insert(position + 1);
                segment_end = position;
            }
            character if is_word(character) => {}
            _ => return None,
        }
    }
    if bytes[position] != b'/' {
        return None;
    }
    let service_slash = position;

    // Invalid if fewer than `/service` bytes precede the slash, or if it is not
    // immediately preceded by `/service`.
    if service_slash < SERVICE.len() || &bytes[service_slash - SERVICE.len()..service_slash] != SERVICE {
        return None;
    }

    // If the service name is dotted, use the shape name after the last dot;
    // otherwise, use the complete service name.
    let service_start = service_start.unwrap_or(service_slash + 1);
    Some(RouteIdentity {
        service: &path[service_start..service_end],
        operation: &path[operation_start..],
        route_key: &path[service_start..],
    })
}
