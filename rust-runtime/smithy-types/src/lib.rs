/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pub mod instant;
use std::collections::HashMap;

pub use crate::instant::Instant;

#[derive(Debug, PartialEq, Clone)]
pub struct Blob {
    inner: Vec<u8>,
}

impl Blob {
    pub fn new<T: Into<Vec<u8>>>(inp: T) -> Self {
        Blob { inner: inp.into() }
    }
}

impl AsRef<[u8]> for Blob {
    fn as_ref(&self) -> &[u8] {
        &self.inner
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Document {
    Object(HashMap<String, Document>),
    Array(Vec<Document>),
    Number(Number),
    String(String),
    Bool(bool),
    Null,
}

/// A number type that implements Javascript / JSON semantics, modeled on serde_json:
/// https://docs.serde.rs/src/serde_json/number.rs.html#20-22
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    PosInt(u64),
    NegInt(i64),
    Float(f64),
}

#[cfg(test)]
mod test {
    use crate::instant::Format;
    use crate::Instant;

    #[test]
    fn test_instant_fmt() {
        let instant = Instant::from_epoch_seconds(1576540098);
        assert_eq!(instant.fmt(Format::DateTime), "2019-12-16T23:48:18Z");
        assert_eq!(instant.fmt(Format::EpochSeconds), "1576540098");
        assert_eq!(
            instant.fmt(Format::HttpDate),
            "Mon, 16 Dec 2019 23:48:18 GMT"
        );

        let instant = Instant::from_fractional_seconds(1576540098, 0.52);
        assert_eq!(instant.fmt(Format::DateTime), "2019-12-16T23:48:18.52Z");
        assert_eq!(instant.fmt(Format::EpochSeconds), "1576540098.52");
        assert_eq!(
            instant.fmt(Format::HttpDate),
            "Mon, 16 Dec 2019 23:48:18.520 GMT"
        );
    }
}
