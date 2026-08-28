/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

const OPERATION_KEYWORD: &[u8] = b"/operation";
const SERVICE_KEYWORD: &[u8] = b"/service";
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

    // /service/Service/operation/OperationShape
    //                 ^ slash at start of /operation, service_segment_end_pos
    //                           ^ first byte of OperationShape, operation_start_pos
    let (operation_start_pos, service_segment_end_pos) = {
        // Scan backwards for the `/` before the operation name. When found,
        // there must at least be `/service/operation/` still left in the string, otherwise
        // it is invalid.
        let mut scan_pos = bytes.len() - 1;
        while scan_pos > MIN_BYTES && bytes[scan_pos] != b'/' {
            // It must be a valid operation.
            if !is_word(bytes[scan_pos]) {
                return None;
            }
            scan_pos -= 1;
        }

        let operation_slash_pos = scan_pos;
        let operation_start_pos = operation_slash_pos + 1;
        // The request is invalid if:
        // 1. There is no operation name following the first `/` found while scanning backwards.
        // 2. Bytes to the left of that `/` must at least have the keywords `/service/operation` in it.
        if operation_start_pos >= bytes.len() || operation_slash_pos < MIN_BYTES {
            return None;
        }
        // String must have `/operation` to the left of the `/` found while scanning backwards.
        if &bytes[operation_slash_pos - OPERATION_KEYWORD.len()..operation_slash_pos] != OPERATION_KEYWORD {
            return None;
        }
        // Can't be all underscores, must be a valid operation name.
        if !has_valid_identifier_start(&bytes[operation_start_pos..]) {
            return None;
        }

        // The service segment ends where `/operation` begins.
        let service_segment_end_pos = operation_slash_pos - OPERATION_KEYWORD.len();
        (operation_start_pos, service_segment_end_pos)
    };

    let service_shape_start_pos = {
        // /service/Service/operation/OperationShape
        //                 ^service_segment_end_pos
        let mut segment_end_pos = service_segment_end_pos;
        let mut service_shape_start_pos = None;

        // Scan backwards for the `/` before the service segment. There must at least be as many characters
        // as `/service` still left in the router otherwise it is invalid.
        let mut scan_pos = service_segment_end_pos - 1;
        while scan_pos > SERVICE_KEYWORD.len() && bytes[scan_pos] != b'/' {
            if bytes[scan_pos] == b'.' {
                //  Each segment in a namespaced service (e.g `com.alpha.service`) name must be a valid identifier.
                if !has_valid_identifier_start(&bytes[scan_pos + 1..segment_end_pos]) {
                    return None;
                }
                service_shape_start_pos.get_or_insert(scan_pos + 1);
                segment_end_pos = scan_pos;
            } else if !is_word(bytes[scan_pos]) {
                return None;
            }
            scan_pos -= 1;
        }

        let service_start_slash_pos = scan_pos;

        // Invalid if fewer than `/service` bytes precede the slash and the loop came out because of
        // that, or if it is not immediately preceded by `/service`.
        if bytes[service_start_slash_pos] != b'/' {
            return None;
        }
        if service_start_slash_pos < SERVICE_KEYWORD.len()
            || &bytes[service_start_slash_pos - SERVICE_KEYWORD.len()..service_start_slash_pos] != SERVICE_KEYWORD
        {
            return None;
        }

        // The remaining text is either the entire service name or its first namespace
        // segment. Subsequent segments were validated at their preceding `.` delimiters.
        if !has_valid_identifier_start(&bytes[service_start_slash_pos + 1..segment_end_pos]) {
            return None;
        }

        // If the service name is dotted, use the shape name after the last dot;
        // otherwise, use the complete service name.
        service_shape_start_pos.unwrap_or(service_start_slash_pos + 1)
    };

    Some(RouteIdentity {
        service: &path[service_shape_start_pos..service_segment_end_pos],
        operation: &path[operation_start_pos..],
        route_key: &path[service_shape_start_pos..],
    })
}
