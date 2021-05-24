/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::borrow::Cow;

const ESCAPES: &[char] = &['"', '\\', '\u{08}', '\u{0C}', '\n', '\r', '\t'];

/// Escapes a string for embedding in a JSON string value.
pub fn escape_string(value: &str) -> Cow<str> {
    if !value.contains(ESCAPES) {
        return Cow::Borrowed(value);
    }

    let mut escaped = String::new();
    let (mut last, end) = (0, value.len());
    for (index, chr) in value
        .char_indices()
        .filter(|(_index, chr)| ESCAPES.contains(chr))
    {
        escaped.push_str(&value[last..index]);
        escaped.push_str(match chr {
            '"' => "\\\"",
            '\\' => "\\\\",
            '\u{08}' => "\\b",
            '\u{0C}' => "\\f",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            _ => unreachable!(),
        });
        last = index + 1;
    }
    escaped.push_str(&value[last..end]);
    Cow::Owned(escaped)
}

#[cfg(test)]
mod test {
    use super::escape_string;
    use serde::Serialize;

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
    }

    #[derive(Serialize)]
    struct TestStruct {
        value: String,
    }

    use proptest::proptest;
    proptest! {
        #[test]
        fn matches_serde_json(s: String) {
            let mut formatted = String::new();
            formatted.push_str(r#"{"value":""#);
            formatted.push_str(&escape_string(&s));
            formatted.push_str("\"}");

            assert_eq!(
                serde_json::to_string(&TestStruct { value: s }).unwrap(),
                formatted
            )
        }
    }
}
