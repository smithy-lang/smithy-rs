/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! UTF-8 string byte buffer representation with validation amortization.

use bytes::Bytes;
use std::convert::TryFrom;
use std::str::Utf8Error;

#[non_exhaustive]
#[derive(Debug)]
pub struct StrBytes {
    bytes: Bytes,
}

impl StrBytes {
    pub fn as_bytes(&self) -> &Bytes {
        &self.bytes
    }

    pub fn as_str(&self) -> &str {
        // Safety: StrBytes can only be constructed from a valid UTF-8 string
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..]) }
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
        }
    }
}

impl From<String> for StrBytes {
    fn from(value: String) -> Self {
        StrBytes {
            bytes: Bytes::from(value),
        }
    }
}

impl From<&'static str> for StrBytes {
    fn from(value: &'static str) -> Self {
        StrBytes {
            bytes: Bytes::from(value),
        }
    }
}

impl TryFrom<&'static [u8]> for StrBytes {
    type Error = Utf8Error;

    fn try_from(value: &'static [u8]) -> Result<Self, Self::Error> {
        match std::str::from_utf8(value) {
            Ok(_) => Ok(StrBytes {
                bytes: Bytes::from(value),
            }),
            Err(err) => Err(err),
        }
    }
}

impl TryFrom<Vec<u8>> for StrBytes {
    type Error = Utf8Error;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        match std::str::from_utf8(&value[..]) {
            Ok(_) => Ok(StrBytes {
                bytes: Bytes::from(value),
            }),
            Err(err) => Err(err),
        }
    }
}

impl TryFrom<Bytes> for StrBytes {
    type Error = Utf8Error;

    fn try_from(bytes: Bytes) -> Result<Self, Self::Error> {
        match std::str::from_utf8(&bytes[..]) {
            Ok(_) => Ok(StrBytes { bytes }),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::str_bytes::StrBytes;
    use bytes::Bytes;
    use std::convert::TryInto;
    use std::str::Utf8Error;

    #[test]
    fn invalid_utf8_correctly_errors() {
        let invalid_utf8 = &[0xC3, 0x28][..];
        assert!(std::str::from_utf8(invalid_utf8).is_err());

        let result: Result<StrBytes, Utf8Error> = invalid_utf8.try_into();
        assert!(result.is_err());

        let result: Result<StrBytes, Utf8Error> = invalid_utf8.to_vec().try_into();
        assert!(result.is_err());

        let result: Result<StrBytes, Utf8Error> = Bytes::from_static(invalid_utf8).try_into();
        assert!(result.is_err());
    }

    #[test]
    fn valid_utf8() {
        let valid_utf8 = "hello";
        let str_bytes: StrBytes = valid_utf8.into();
        assert_eq!(valid_utf8.as_bytes(), str_bytes.as_bytes());
        assert_eq!(valid_utf8, str_bytes.as_str());
        assert_eq!(valid_utf8, str_bytes.clone().as_str());
    }

    #[test]
    fn equals() {
        let str_bytes: StrBytes = "test".into();
        assert_eq!(str_bytes, str_bytes);
        let other_bytes: StrBytes = "foo".into();
        assert_ne!(str_bytes, other_bytes);
    }
}
