/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

use crate::endpoint_lib::diagnostic::DiagnosticCollector;
use urlencoding::encode;

// Returns `Option` for forwards compatibility
pub(crate) fn uri_encode<'a, 'b>(s: &'a str, _e: &'b mut DiagnosticCollector) -> Option<std::borrow::Cow<'a, str>> {
    Some(encode(s))
}
