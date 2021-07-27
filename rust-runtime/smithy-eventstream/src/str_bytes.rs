/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! UTF-8 string byte buffer representation with validation amortization.

use bytes::Bytes;
use std::str::Utf8Error;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Eq, PartialEq, Copy, Clone)]
enum State {
    Valid,
    Invalid,
    Unknown,
}

impl State {
    fn as_u8(&self) -> u8 {
        match self {
            State::Valid => 1,
            State::Invalid => 2,
            State::Unknown => 3,
        }
    }
}

impl From<&AtomicU8> for State {
    fn from(value: &AtomicU8) -> Self {
        match value.load(Ordering::Relaxed) {
            1 => State::Valid,
            2 => State::Invalid,
            _ => State::Unknown,
        }
    }
}

/// UTF-8 string byte buffer representation with validation amortization.
#[non_exhaustive]
#[derive(Debug)]
pub struct StrBytes {
    bytes: Bytes,
    valid_utf8: AtomicU8,
}

impl StrBytes {
    pub fn as_bytes(&self) -> &Bytes {
        &self.bytes
    }

    pub fn as_str(&self) -> Result<&str, Utf8Error> {
        if State::Valid == State::from(&self.valid_utf8) {
            // Safety: We know it's valid UTF-8 already
            return Ok(unsafe { std::str::from_utf8_unchecked(self.bytes.as_ref()) });
        }
        match std::str::from_utf8(self.bytes.as_ref()) {
            Ok(value) => {
                self.valid_utf8
                    .store(State::Valid.as_u8(), Ordering::Relaxed);
                Ok(value)
            }
            Err(err) => {
                self.valid_utf8
                    .store(State::Invalid.as_u8(), Ordering::Relaxed);
                Err(err)
            }
        }
    }
}

#[cfg(feature = "derive-arbitrary")]
impl<'a> arbitrary::Arbitrary<'a> for StrBytes {
    fn arbitrary(unstruct: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(String::arbitrary(unstruct)?.into())
    }
}

impl Eq for StrBytes {}

impl PartialEq for StrBytes {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Clone for StrBytes {
    fn clone(&self) -> Self {
        StrBytes {
            bytes: self.bytes.clone(),
            valid_utf8: AtomicU8::new(self.valid_utf8.load(Ordering::Relaxed)),
        }
    }
}

impl From<String> for StrBytes {
    fn from(value: String) -> Self {
        StrBytes {
            bytes: Bytes::from(value),
            valid_utf8: AtomicU8::new(State::Valid.as_u8()),
        }
    }
}

impl From<&'static str> for StrBytes {
    fn from(value: &'static str) -> Self {
        StrBytes {
            bytes: Bytes::from(value),
            valid_utf8: AtomicU8::new(State::Valid.as_u8()),
        }
    }
}

impl From<&'static [u8]> for StrBytes {
    fn from(value: &'static [u8]) -> Self {
        StrBytes {
            bytes: Bytes::from(value),
            valid_utf8: AtomicU8::new(State::Unknown.as_u8()),
        }
    }
}

impl From<Vec<u8>> for StrBytes {
    fn from(value: Vec<u8>) -> Self {
        StrBytes {
            bytes: Bytes::from(value),
            valid_utf8: AtomicU8::new(State::Unknown.as_u8()),
        }
    }
}

impl From<Bytes> for StrBytes {
    fn from(bytes: Bytes) -> Self {
        StrBytes {
            bytes,
            valid_utf8: AtomicU8::new(State::Unknown.as_u8()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::str_bytes::StrBytes;
    use bytes::Bytes;

    #[test]
    fn invalid_utf8_correctly_errors() {
        let invalid_utf8 = &[0xC3, 0x28][..];
        assert!(std::str::from_utf8(invalid_utf8).is_err());

        let str_bytes: StrBytes = invalid_utf8.into();
        assert_eq!(invalid_utf8, str_bytes.as_bytes());
        for _ in 0..3 {
            assert!(str_bytes.as_str().is_err());
            assert!(str_bytes.clone().as_str().is_err());
        }

        let str_bytes: StrBytes = invalid_utf8.to_vec().into();
        assert_eq!(invalid_utf8, str_bytes.as_bytes());
        assert!(str_bytes.as_str().is_err());

        let str_bytes: StrBytes = Bytes::from_static(invalid_utf8).into();
        assert_eq!(invalid_utf8, str_bytes.as_bytes());
        assert!(str_bytes.as_str().is_err());
    }

    #[test]
    fn valid_utf8() {
        let valid_utf8 = "hello";
        let str_bytes: StrBytes = valid_utf8.into();
        assert_eq!(valid_utf8.as_bytes(), str_bytes.as_bytes());
        assert_eq!(valid_utf8, str_bytes.as_str().unwrap());
        assert_eq!(valid_utf8, str_bytes.clone().as_str().unwrap());
    }

    #[test]
    fn equals() {
        let str_bytes: StrBytes = "test".into();
        assert_eq!(str_bytes, str_bytes);
        let other_bytes: StrBytes = "foo".into();
        assert_ne!(str_bytes, other_bytes);
    }
}
