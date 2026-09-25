/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Opt-in comparison of fuzz results that ignores differences a client cannot observe.
//!
//! Set `SMITHY_FUZZ_SEMANTIC_COMPARE` to compare the deserialized operation input as well as the response,
//! and to treat responses as equal when they differ only in `content-length`, JSON member order, or CBOR
//! encoding choices (definite versus indefinite lengths, integer widths, map entry order).

use aws_smithy_fuzz::FuzzResult;
use cbor_diag::DataItem;
use std::collections::HashMap;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SMITHY_FUZZ_SEMANTIC_COMPARE").is_some())
}

/// Whether two targets' results for the same request agree.
pub(crate) fn results_agree(a: &FuzzResult, b: &FuzzResult) -> bool {
    if !enabled() {
        return a.response == b.response;
    }
    a.input == b.input
        && a.response.status == b.response.status
        && headers(&a.response.headers) == headers(&b.response.headers)
        && bodies_agree(&a.response.body, &b.response.body)
}

fn headers(headers: &HashMap<String, String>) -> HashMap<&str, &str> {
    headers
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("content-length"))
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

fn bodies_agree(a: &[u8], b: &[u8]) -> bool {
    if a == b {
        return true;
    }
    if let (Ok(a), Ok(b)) = (
        serde_json::from_slice::<serde_json::Value>(a),
        serde_json::from_slice::<serde_json::Value>(b),
    ) {
        return a == b;
    }
    if let (Ok(a), Ok(b)) = (cbor_diag::parse_bytes(a), cbor_diag::parse_bytes(b)) {
        return canonical_cbor(&a) == canonical_cbor(&b);
    }
    false
}

/// Renders a CBOR item so that encodings of the same value render identically.
fn canonical_cbor(item: &DataItem) -> String {
    match item {
        DataItem::Integer { value, .. } => value.to_string(),
        DataItem::Negative { value, .. } => format!("-{}", *value as u128 + 1),
        DataItem::ByteString(bytes) => format!("h{:?}", bytes.data),
        DataItem::IndefiniteByteString(chunks) => {
            format!("h{:?}", chunks.iter().flat_map(|c| c.data.iter().copied()).collect::<Vec<_>>())
        }
        DataItem::TextString(text) => format!("{:?}", text.data),
        DataItem::IndefiniteTextString(chunks) => {
            format!("{:?}", chunks.iter().map(|c| c.data.as_str()).collect::<String>())
        }
        DataItem::Array { data, .. } => {
            format!("[{}]", data.iter().map(canonical_cbor).collect::<Vec<_>>().join(","))
        }
        DataItem::Map { data, .. } => {
            let mut entries: Vec<_> = data
                .iter()
                .map(|(k, v)| format!("{}:{}", canonical_cbor(k), canonical_cbor(v)))
                .collect();
            entries.sort();
            format!("{{{}}}", entries.join(","))
        }
        DataItem::Tag { tag, value, .. } => format!("{}({})", tag.0, canonical_cbor(value)),
        DataItem::Float { value, .. } => format!("{value:?}"),
        DataItem::Simple(simple) => format!("simple({})", simple.0),
    }
}
