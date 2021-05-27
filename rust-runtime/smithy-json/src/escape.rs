/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::borrow::Cow;

/// Escapes a string for embedding in a JSON string value.
pub fn escape_string(value: &str) -> Cow<str> {
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            0..=0x1F | b'"' | b'\\' => {
                return Cow::Owned(escape_string_inner(&bytes[0..index], &bytes[index..]))
            }
            _ => {}
        }
    }
    Cow::Borrowed(value)
}

fn escape_string_inner(start: &[u8], rest: &[u8]) -> String {
    let mut escaped = start.to_vec();
    for byte in rest {
        match byte {
            b'"' => escaped.extend("\\\"".bytes()),
            b'\\' => escaped.extend("\\\\".bytes()),
            0x08 => escaped.extend("\\b".bytes()),
            0x0C => escaped.extend("\\f".bytes()),
            b'\n' => escaped.extend("\\n".bytes()),
            b'\r' => escaped.extend("\\r".bytes()),
            b'\t' => escaped.extend("\\t".bytes()),
            0..=0x1F => escaped.extend(format!("\\u{:04x}", byte).bytes()),
            _ => escaped.push(*byte),
        }
    }
    // Our input was originally valid UTF-8, and we didn't do anything to invalidate it
    debug_assert!(String::from_utf8(escaped.clone()).is_ok());
    unsafe { String::from_utf8_unchecked(escaped) }
}

#[cfg(test)]
mod test {
    use super::escape_string;

    #[test]
    fn escape() {
        assert_eq!("", escape_string("").as_ref());
        assert_eq!("foo", escape_string("foo").as_ref());
        assert_eq!("foo\\r\\n", escape_string("foo\r\n").as_ref());
        assert_eq!("foo\\r\\nbar", escape_string("foo\r\nbar").as_ref());
        assert_eq!(r#"foo\\bar"#, escape_string(r#"foo\bar"#).as_ref());
        assert_eq!(r#"\\foobar"#, escape_string(r#"\foobar"#).as_ref());
        assert_eq!(
            r#"\bf\fo\to\r\n"#,
            escape_string("\u{08}f\u{0C}o\to\r\n").as_ref()
        );
        assert_eq!("\\\"test\\\"", escape_string("\"test\"").as_ref());
        assert_eq!("\\u0000", escape_string("\u{0}").as_ref());
        assert_eq!("\\u001f", escape_string("\u{1f}").as_ref());
    }

    use proptest::proptest;
    proptest! {
        #[test]
        fn matches_serde_json(s in ".*") {
            assert_eq!(
                serde_json::to_string(&s).unwrap(),
                format!(r#""{}""#, escape_string(&s))
            )
        }
    }
}
