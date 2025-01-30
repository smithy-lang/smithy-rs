/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
enum EscapeErrorKind {
    ExpectedSurrogatePair(String),
    InvalidEscapeCharacter(char),
    #[cfg(not(feature = "replace-invalid-utf8"))]
    InvalidSurrogatePair(u16, u16),
    InvalidUnicodeEscape(String),
    InvalidUtf8,
    UnexpectedEndOfString,
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct EscapeError {
    kind: EscapeErrorKind,
}

impl std::error::Error for EscapeError {}

impl fmt::Display for EscapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use EscapeErrorKind::*;
        match &self.kind {
            ExpectedSurrogatePair(low) => {
                write!(
                    f,
                    "expected a UTF-16 surrogate pair, but got {} as the low word",
                    low
                )
            }
            InvalidEscapeCharacter(chr) => write!(f, "invalid JSON escape: \\{}", chr),
            #[cfg(not(feature = "replace-invalid-utf8"))]
            InvalidSurrogatePair(high, low) => {
                write!(f, "invalid surrogate pair: \\u{:04X}\\u{:04X}", high, low)
            }
            InvalidUnicodeEscape(escape) => write!(f, "invalid JSON Unicode escape: \\u{}", escape),
            InvalidUtf8 => write!(f, "invalid UTF-8 codepoint in JSON string"),
            UnexpectedEndOfString => write!(f, "unexpected end of string"),
        }
    }
}

impl From<EscapeErrorKind> for EscapeError {
    fn from(kind: EscapeErrorKind) -> Self {
        Self { kind }
    }
}

/// Escapes a string for embedding in a JSON string value.
pub(crate) fn escape_string(value: &str) -> Cow<'_, str> {
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
    let mut escaped = Vec::with_capacity(start.len() + rest.len() + 1);
    escaped.extend(start);

    for byte in rest {
        match byte {
            b'"' => escaped.extend(b"\\\""),
            b'\\' => escaped.extend(b"\\\\"),
            0x08 => escaped.extend(b"\\b"),
            0x0C => escaped.extend(b"\\f"),
            b'\n' => escaped.extend(b"\\n"),
            b'\r' => escaped.extend(b"\\r"),
            b'\t' => escaped.extend(b"\\t"),
            0..=0x1F => escaped.extend(format!("\\u{:04x}", byte).bytes()),
            _ => escaped.push(*byte),
        }
    }

    // This is safe because:
    // - The original input was valid UTF-8 since it came in as a `&str`
    // - Only single-byte code points were escaped
    // - The escape sequences are valid UTF-8
    debug_assert!(std::str::from_utf8(&escaped).is_ok());
    unsafe { String::from_utf8_unchecked(escaped) }
}

/// Unescapes a JSON-escaped string.
/// If there are no escape sequences, it directly returns the reference.
pub(crate) fn unescape_string(value: &str) -> Result<Cow<'_, str>, EscapeError> {
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\\' {
            return unescape_string_inner(&bytes[0..index], &bytes[index..]).map(Cow::Owned);
        }
    }
    Ok(Cow::Borrowed(value))
}

fn unescape_string_inner(start: &[u8], rest: &[u8]) -> Result<String, EscapeError> {
    let mut unescaped = Vec::with_capacity(start.len() + rest.len());
    unescaped.extend(start);

    let mut index = 0;
    while index < rest.len() {
        match rest[index] {
            b'\\' => {
                index += 1;
                if index == rest.len() {
                    return Err(EscapeErrorKind::UnexpectedEndOfString.into());
                }
                match rest[index] {
                    b'u' => {
                        index -= 1;
                        index += read_unicode_escapes(&rest[index..], &mut unescaped)?;
                    }
                    byte => {
                        match byte {
                            b'\\' => unescaped.push(b'\\'),
                            b'/' => unescaped.push(b'/'),
                            b'"' => unescaped.push(b'"'),
                            b'b' => unescaped.push(0x08),
                            b'f' => unescaped.push(0x0C),
                            b'n' => unescaped.push(b'\n'),
                            b'r' => unescaped.push(b'\r'),
                            b't' => unescaped.push(b'\t'),
                            _ => {
                                return Err(
                                    EscapeErrorKind::InvalidEscapeCharacter(byte.into()).into()
                                )
                            }
                        }
                        index += 1;
                    }
                }
            }
            byte => {
                unescaped.push(byte);
                index += 1
            }
        }
    }

    String::from_utf8(unescaped).map_err(|_| EscapeErrorKind::InvalidUtf8.into())
}

fn is_utf16_low_surrogate(codepoint: u16) -> bool {
    codepoint & 0xFC00 == 0xDC00
}

fn is_utf16_high_surrogate(codepoint: u16) -> bool {
    codepoint & 0xFC00 == 0xD800
}

fn read_codepoint(rest: &[u8]) -> Result<u16, EscapeError> {
    if rest.len() < 6 {
        return Err(EscapeErrorKind::UnexpectedEndOfString.into());
    }
    if &rest[0..2] != b"\\u" {
        // The first codepoint is always prefixed with "\u" since unescape_string_inner does
        // that check, so this error will always be for the low word of a surrogate pair.
        return Err(EscapeErrorKind::ExpectedSurrogatePair(
            String::from_utf8_lossy(&rest[0..6]).into(),
        )
        .into());
    }

    let codepoint_str =
        std::str::from_utf8(&rest[2..6]).map_err(|_| EscapeErrorKind::InvalidUtf8)?;

    // Error on characters `u16::from_str_radix` would otherwise accept, such as `+`
    if codepoint_str.bytes().any(|byte| !byte.is_ascii_hexdigit()) {
        return Err(EscapeErrorKind::InvalidUnicodeEscape(codepoint_str.into()).into());
    }
    Ok(u16::from_str_radix(codepoint_str, 16).expect("hex string is valid 16-bit value"))
}

#[cfg(not(feature = "replace-invalid-utf8"))]
/// Reads JSON Unicode escape sequences (i.e., "\u1234"). Will also read
/// an additional codepoint if the first codepoint is the start of a surrogate pair.
fn read_unicode_escapes(bytes: &[u8], into: &mut Vec<u8>) -> Result<usize, EscapeError> {
    let high = read_codepoint(bytes)?;
    let (bytes_read, chr) = if is_utf16_high_surrogate(high) {
        let low = read_codepoint(&bytes[6..])?;
        if !is_utf16_low_surrogate(low) {
            return Err(EscapeErrorKind::InvalidSurrogatePair(high, low).into());
        }

        let codepoint =
            std::char::from_u32(0x10000 + (high - 0xD800) as u32 * 0x400 + (low - 0xDC00) as u32)
                .ok_or(EscapeErrorKind::InvalidSurrogatePair(high, low))?;
        (12, codepoint)
    } else {
        let codepoint = std::char::from_u32(high as u32).ok_or_else(|| {
            EscapeErrorKind::InvalidUnicodeEscape(String::from_utf8_lossy(&bytes[0..6]).into())
        })?;
        (6, codepoint)
    };

    match chr.len_utf8() {
        1 => into.push(chr as u8),
        _ => into.extend_from_slice(chr.encode_utf8(&mut [0; 4]).as_bytes()),
    }
    Ok(bytes_read)
}

#[cfg(feature = "replace-invalid-utf8")]
fn read_unicode_escapes(bytes: &[u8], into: &mut Vec<u8>) -> Result<usize, EscapeError> {
    let high = read_codepoint(bytes)?;
    let (bytes_read, chr) = if is_utf16_high_surrogate(high) {
        match read_codepoint(&bytes[6..]) {
            Ok(low) if is_utf16_low_surrogate(low) => {
                let codepoint = 0x10000 + (high - 0xD800) as u32 * 0x400 + (low - 0xDC00) as u32;
                (12, std::char::from_u32(codepoint))
            }
            _ => (6, None),
        }
    } else {
        (6, std::char::from_u32(high as u32))
    };

    match chr {
        Some(chr) => match chr.len_utf8() {
            1 => into.push(chr as u8),
            _ => into.extend_from_slice(chr.encode_utf8(&mut [0; 4]).as_bytes()),
        },
        None => into.extend_from_slice(&[0xEF, 0xBF, 0xBD]), // &[0xEF, 0xBF, 0xBD] is the byte representation of the '�' (\uFFFD) replacement character.
    }

    Ok(bytes_read)
}

#[cfg(test)]
mod test {
    use super::escape_string;
    use crate::escape::{unescape_string, EscapeErrorKind};
    use std::borrow::Cow;

    #[test]
    fn escape() {
        assert_eq!("", escape_string("").as_ref());
        assert_eq!("foo", escape_string("foo").as_ref());
        assert_eq!("foo\\r\\n", escape_string("foo\r\n").as_ref());
        assert_eq!("foo\\r\\nbar", escape_string("foo\r\nbar").as_ref());
        assert_eq!(r"foo\\bar", escape_string(r"foo\bar").as_ref());
        assert_eq!(r"\\foobar", escape_string(r"\foobar").as_ref());
        assert_eq!(
            r"\bf\fo\to\r\n",
            escape_string("\u{08}f\u{0C}o\to\r\n").as_ref()
        );
        assert_eq!("\\\"test\\\"", escape_string("\"test\"").as_ref());
        assert_eq!("\\u0000", escape_string("\u{0}").as_ref());
        assert_eq!("\\u001f", escape_string("\u{1f}").as_ref());
    }

    #[test]
    fn unescape_no_escapes() {
        let unescaped = unescape_string("test test").unwrap();
        assert_eq!("test test", unescaped);
        assert!(matches!(unescaped, Cow::Borrowed(_)));
    }

    #[cfg(not(feature = "replace-invalid-utf8"))]
    #[test]
    fn unescape() {
        assert_eq!(
            "\x08f\x0Co\to\r\n",
            unescape_string(r"\bf\fo\to\r\n").unwrap()
        );
        assert_eq!("\"test\"", unescape_string(r#"\"test\""#).unwrap());
        assert_eq!("\x00", unescape_string("\\u0000").unwrap());
        assert_eq!("\x1f", unescape_string("\\u001f").unwrap());
        assert_eq!("foo\r\nbar", unescape_string("foo\\r\\nbar").unwrap());
        assert_eq!("foo\r\n", unescape_string("foo\\r\\n").unwrap());
        assert_eq!("\r\nbar", unescape_string("\\r\\nbar").unwrap());
        assert_eq!("\u{10437}", unescape_string("\\uD801\\uDC37").unwrap());

        assert_eq!(
            Err(EscapeErrorKind::UnexpectedEndOfString.into()),
            unescape_string("\\")
        );
        assert_eq!(
            Err(EscapeErrorKind::UnexpectedEndOfString.into()),
            unescape_string("\\u")
        );
        assert_eq!(
            Err(EscapeErrorKind::UnexpectedEndOfString.into()),
            unescape_string("\\u00")
        );
        assert_eq!(
            Err(EscapeErrorKind::InvalidEscapeCharacter('z').into()),
            unescape_string("\\z")
        );

        assert_eq!(
            Err(EscapeErrorKind::ExpectedSurrogatePair("\\nasdf".into()).into()),
            unescape_string("\\uD801\\nasdf")
        );
        assert_eq!(
            Err(EscapeErrorKind::UnexpectedEndOfString.into()),
            unescape_string("\\uD801\\u00")
        );
        assert_eq!(
            Err(EscapeErrorKind::InvalidSurrogatePair(0xD801, 0xC501).into()),
            unescape_string("\\uD801\\uC501")
        );

        assert_eq!(
            Err(EscapeErrorKind::InvalidUnicodeEscape("+04D".into()).into()),
            unescape_string("\\u+04D")
        );
    }

    #[cfg(feature = "replace-invalid-utf8")]
    #[test]
    fn unescape() {
        assert_eq!(
            "\x08f\x0Co\to\r\n",
            unescape_string(r"\bf\fo\to\r\n").unwrap()
        );
        assert_eq!("\"test\"", unescape_string(r#"\"test\""#).unwrap());
        assert_eq!("\x00", unescape_string("\\u0000").unwrap());
        assert_eq!("\x1f", unescape_string("\\u001f").unwrap());
        assert_eq!("foo\r\nbar", unescape_string("foo\\r\\nbar").unwrap());
        assert_eq!("foo\r\n", unescape_string("foo\\r\\n").unwrap());
        assert_eq!("\r\nbar", unescape_string("\\r\\nbar").unwrap());
        assert_eq!("\u{10437}", unescape_string("\\uD801\\uDC37").unwrap());

        // New tests for invalid Unicode replacement
        assert_eq!("�", unescape_string("\\uD800").unwrap()); // High surrogate without low surrogate
        assert_eq!("�", unescape_string("\\uDC00").unwrap()); // Low surrogate without high surrogate
        assert_eq!("��", unescape_string("\\uD800\\uD800").unwrap()); // Two high surrogates
        assert_eq!("��", unescape_string("\\uDC00\\uDC00").unwrap()); // Two low surrogates
        assert_eq!("test�test", unescape_string("test\\uD800test").unwrap()); // Orphaned surrogate in middle of string
        assert_eq!(
            "�\u{10437}",
            unescape_string("\\uD800\\uD801\\uDC37").unwrap()
        ); // Invalid then valid surrogate pair

        // These error cases should still work as before
        assert_eq!(
            Err(EscapeErrorKind::UnexpectedEndOfString.into()),
            unescape_string("\\")
        );
        assert_eq!(
            Err(EscapeErrorKind::UnexpectedEndOfString.into()),
            unescape_string("\\u")
        );
        assert_eq!(
            Err(EscapeErrorKind::UnexpectedEndOfString.into()),
            unescape_string("\\u00")
        );
        assert_eq!(
            Err(EscapeErrorKind::InvalidEscapeCharacter('z').into()),
            unescape_string("\\z")
        );
        assert_eq!(
            Err(EscapeErrorKind::InvalidUnicodeEscape("+04D".into()).into()),
            unescape_string("\\u+04D")
        );

        // Regular character.
        assert_eq!("A", unescape_string("\\u0041").unwrap());

        // Single surrogates (should each become �).
        assert_eq!("�", unescape_string("\\uD800").unwrap()); // High surrogate
        assert_eq!("�", unescape_string("\\uDC00").unwrap()); // Low surrogate

        // Valid surrogate pair.
        assert_eq!("🦀", unescape_string("\\uD83E\\uDD80").unwrap());

        // Invalid pairs (should each become ��).
        assert_eq!("��", unescape_string("\\uD800\\uD801").unwrap()); // High + High
        assert_eq!("��", unescape_string("\\uDC00\\uDC01").unwrap()); // Low + Low
        assert_eq!("��", unescape_string("\\uDC00\\uD800").unwrap()); // Low + High

        // Surrogate + non-surrogate.
        assert_eq!("�A", unescape_string("\\uD800\\u0041").unwrap()); // High + ASCII
        assert_eq!("�A", unescape_string("\\uDC00\\u0041").unwrap()); // Low + ASCII
    }

    use proptest::proptest;
    proptest! {
        #[test]
        fn matches_serde_json(s in ".*") {
            let serde_escaped = serde_json::to_string(&s).unwrap();
            let serde_escaped = &serde_escaped[1..(serde_escaped.len() - 1)];
            assert_eq!(serde_escaped,escape_string(&s))
        }

        #[test]
        fn round_trip(chr in proptest::char::any()) {
            let mut original = String::new();
            original.push(chr);

            let escaped = escape_string(&original);
            let unescaped = unescape_string(&escaped).unwrap();
            assert_eq!(original, unescaped);
        }

        #[test]
        fn unicode_surrogates(chr in proptest::char::range(
            std::char::from_u32(0x10000).unwrap(),
            std::char::from_u32(0x10FFFF).unwrap(),
        )) {
            let mut codepoints = [0; 2];
            chr.encode_utf16(&mut codepoints);

            let escaped = format!("\\u{:04X}\\u{:04X}", codepoints[0], codepoints[1]);
            let unescaped = unescape_string(&escaped).unwrap();

            let expected = format!("{}", chr);
            assert_eq!(expected, unescaped);
        }
    }

    #[test]
    #[ignore] // This tests escaping of all codepoints, but can take a long time in debug builds
    fn all_codepoints() {
        for value in 0..u32::MAX {
            if let Some(chr) = char::from_u32(value) {
                let string = String::from(chr);
                let escaped = escape_string(&string);
                let serde_escaped = serde_json::to_string(&string).unwrap();
                let serde_escaped = &serde_escaped[1..(serde_escaped.len() - 1)];
                assert_eq!(&escaped, serde_escaped);
            }
        }
    }
}
