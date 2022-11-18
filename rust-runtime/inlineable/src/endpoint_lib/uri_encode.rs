/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::endpoint_lib::diagnostic::DiagnosticCollector;

use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

/// base set of characters that must be URL encoded
pub(crate) const BASE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'/')
    // RFC-3986 §3.3 allows sub-delims (defined in section2.2) to be in the path component.
    // This includes both colon ':' and comma ',' characters.
    // Smithy protocol tests & AWS services percent encode these expected values. Signing
    // will fail if these values are not percent encoded
    .add(b':')
    .add(b',')
    .add(b'?')
    .add(b'#')
    .add(b'[')
    .add(b']')
    .add(b'{')
    .add(b'}')
    .add(b'|')
    .add(b'@')
    .add(b'!')
    .add(b'$')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b';')
    .add(b'=')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'"')
    .add(b'^')
    .add(b'`')
    .add(b'\\');

// Returns `Option` for forwards compatibility
pub(crate) fn uri_encode<'a, 'b>(
    s: &'a str,
    _e: &'b mut DiagnosticCollector,
) -> std::borrow::Cow<'a, str> {
    utf8_percent_encode(s, &BASE_SET).into()
}
