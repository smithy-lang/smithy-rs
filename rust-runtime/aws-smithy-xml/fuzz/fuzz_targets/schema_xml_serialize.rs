/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Fuzz target: serializer output validity for the schema-based XML codec.
//!
//! # Strategy
//!
//! libfuzzer generates raw bytes; `Arbitrary` interprets them as structured
//! [`FuzzValue`]s. Each variant is wrapped in its single-member struct
//! schema (`wrapper_schema_for`) so the value can serialize to a complete
//! XML document. We don't deserialize — we only verify the serializer's
//! output is well-formed XML.
//!
//! Validation:
//! 1. The output is valid UTF-8 (Rust's `String` makes this trivially
//!    true on the inner buffer, but verifying the published byte stream
//!    catches any latent unsafe path that bypasses `String`).
//! 2. The output tokenizes cleanly with `xmlparser` — every escape
//!    sequence, attribute, comment-like construct must be syntactically
//!    valid XML.
//! 3. Start and end tags balance — a small stack of element names
//!    over the `xmlparser` token stream catches any future bug where the
//!    serializer emits `</A>` for an open `<B>` or leaves a tag unclosed.
//!
//! `xmlparser` is intentionally permissive about a few things (e.g.,
//! duplicated attribute names) but is strict about escape syntax,
//! attribute quoting, and UTF-8. This catches the failure modes most
//! likely to actually exist:
//! - Bad escape sequences (escape outputs invalid `&xyz;`)
//! - Missing attribute quoting / `=`
//! - Unescaped `&`/`<`/`>`/`"` in text or attribute content
//! - Unescaped EOL chars (per the EOL-encoding SEP, `\r`/`\n`/U+0085/U+2028
//!   in text or attribute values must be emitted as `&#xD;`/`&#xA;`/etc.)
//!
//! # Skips
//!
//! - **XML 1.0 invalid chars** in string content: the serializer's
//!   `escape()` emits these as `&#x0;` (NUL) etc., but `xmlparser`
//!   correctly rejects numeric character references that resolve to
//!   characters disallowed by XML 1.0 §2.2. That's not a serializer
//!   bug — XML 1.0 simply can't represent NUL. The roundtrip target
//!   exercises the same path; this target focuses on parser well-
//!   formedness, so we skip variants containing such bytes.

#![no_main]
use libfuzzer_sys::fuzz_target;

mod schema_common;

use schema_common::*;

fuzz_target!(|value: FuzzValue| {
    if contains_xml10_invalid_char(&value) {
        return;
    }

    let codec = default_codec();
    let bytes = match serialize_value(&value, &codec) {
        Ok(b) => b,
        // Non-finite floats currently round-trip via "Infinity" / "NaN"
        // text — that's well-formed XML. Other Err paths (e.g. document
        // serialization) are intentional rejections from the codec; nothing
        // to validate. Skip silently.
        Err(_) => return,
    };

    // 1) Output must be valid UTF-8.
    let s = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(_) => panic!(
            "XmlSerializer produced non-UTF-8 output!\nInput: {:?}\nOutput: {:?}",
            value, bytes
        ),
    };

    // 2 + 3) Tokenize through xmlparser, tracking a small stack of element
    //        names to catch unbalanced or unmatched tags.
    let mut stack: Vec<String> = Vec::new();
    for token in xmlparser::Tokenizer::from(s) {
        let token = match token {
            Ok(t) => t,
            Err(e) => panic!(
                "XmlSerializer produced output that xmlparser rejected!\n\
                 Input: {:?}\n\
                 Output: {:?}\n\
                 Error: {}",
                value, s, e,
            ),
        };
        match token {
            xmlparser::Token::ElementStart { local, .. } => {
                stack.push(local.as_str().to_owned());
            }
            xmlparser::Token::ElementEnd { end, .. } => match end {
                xmlparser::ElementEnd::Open => { /* `>` — start stays on stack */ }
                xmlparser::ElementEnd::Close(_, local) => {
                    let popped = stack.pop().unwrap_or_default();
                    if popped != local.as_str() {
                        panic!(
                            "Mismatched close tag!\nInput: {:?}\nOutput: {:?}\n\
                             Expected closing of <{}>, got </{}>",
                            value, s, popped, local,
                        );
                    }
                }
                xmlparser::ElementEnd::Empty => {
                    // `<X/>` — open and close in one token; pop the start we just pushed.
                    stack.pop();
                }
            },
            _ => {}
        }
    }

    if !stack.is_empty() {
        panic!(
            "Unclosed elements at end of output!\nInput: {:?}\nOutput: {:?}\n\
             Still open: {:?}",
            value, s, stack,
        );
    }
});
