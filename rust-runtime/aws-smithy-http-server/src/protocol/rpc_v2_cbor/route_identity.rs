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
    // A route must have at least `/service/operation` length bytes in it.
    if bytes.len() < MIN_BYTES {
        return None;
    }

    let (operation_start, service_end) = {
        // Find the first `/` from the right to figure out the operation name. When found,
        // there must at least be `/service/operation/` still left in the string, otherwise
        // it is invalid.
        let mut position = bytes.len() - 1;
        while position > MIN_BYTES && bytes[position] != b'/' {
            // It must be a valid operation.
            if !is_word(bytes[position]) {
                return None;
            }
            position -= 1;
        }

        let operation_slash = position;
        let operation_start = operation_slash + 1;
        // The request is invalid if:
        // 1. There is no operation name following the first `/` found from right.
        // 2. Bytes to the left of the first `/` from right must at least have the keywords `/service/operation` in it.
        if operation_start >= bytes.len() || operation_slash < MIN_BYTES {
            return None;
        }
        // String must have `/operation` to the left of the first `/` from right.
        if &bytes[operation_slash - OPERATION.len()..operation_slash] != OPERATION {
            return None;
        }
        // Can't be all underscores, must be a valid operation name.
        if !has_valid_identifier_start(&bytes[operation_start..]) {
            return None;
        }

        // Extract the service name preceding `/operation/`.
        let service_end = operation_slash - OPERATION.len();
        (operation_start, service_end)
    };

    let service_start = {
        let mut position = service_end - 1;

        let mut segment_end = service_end;
        let mut service_start = None;
        // Look for the `/` from right starting from service_end. There must at least be as many characters
        // as `/service` still left in the router otherwise it is invalid.
        while position > SERVICE.len() && bytes[position] != b'/' {
            if bytes[position] == b'.' {
                //  Each segment in a namespaced service (e.g `com.alpha.service`) name must be a valid identifier.
                if !has_valid_identifier_start(&bytes[position + 1..segment_end]) {
                    return None;
                }
                service_start.get_or_insert(position + 1);
                segment_end = position;
            } else if !is_word(bytes[position]) {
                return None;
            }
            position -= 1;
        }

        let service_slash = position;
        // Invalid if fewer than `/service` bytes precede the slash, or if it is not
        // immediately preceded by `/service`.
        if service_slash < SERVICE.len() || &bytes[service_slash - SERVICE.len()..service_slash] != SERVICE {
            return None;
        }

        // The remaining text is either the entire service name or its first namespace
        // segment. Subsequent segments were validated at their preceding `.` delimiters.
        if !has_valid_identifier_start(&bytes[service_slash + 1..segment_end]) {
            return None;
        }

        // If the service name is dotted, use the shape name after the last dot;
        // otherwise, use the complete service name.
        service_start.unwrap_or(service_slash + 1)
    };

    Some(RouteIdentity {
        service: &path[service_start..service_end],
        operation: &path[operation_start..],
        route_key: &path[service_start..],
    })
}
