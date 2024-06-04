/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use core::fmt;
use std::str::FromStr;

use crate::error;

/// Representation of `Range` header.
/// NOTE: S3 only supports a single bytes range this is a simplified representation
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Range(pub(crate) ByteRange);

impl Range {
    /// Create a range from the given byte range
    pub(crate) fn bytes(rng: ByteRange) -> Self {
        Self(rng)
    }

    /// Create a range from the inclusive start and end offsets
    pub(crate) fn bytes_inclusive(start: u64, end: u64) -> Self {
        Range::bytes(ByteRange::Inclusive(start, end))
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bytes={}", self.0)
    }
}

impl From<Range> for String {
    fn from(value: Range) -> Self {
        format!("{}", value)
    }
}

impl FromStr for Range {
    type Err = error::TransferError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut iter = s.splitn(2, '=');
        match (iter.next(), iter.next()) {
            (Some("bytes"), Some(range)) => {
                if range.contains(',') {
                    // TODO(aws-sdk-rust#1159) - error S3 doesn't support multiple byte ranges
                    Err(error::invalid_meta_request(format!(
                        "multiple byte ranges not supported for range header {}",
                        s
                    )))
                } else {
                    let spec = ByteRange::from_str(range).map_err(|_| {
                        error::invalid_meta_request(format!("invalid range header {}", s))
                    })?;
                    Ok(Range(spec))
                }
            }
            _ => Err(error::invalid_meta_request(format!(
                "unsupported byte range header format `{s}`; see https://www.rfc-editor.org/rfc/rfc9110.html#name-range for valid formats"
            ))),
        }
    }
}

/// Representation of a single [RFC-99110 byte range](https://www.rfc-editor.org/rfc/rfc9110.html#name-byte-ranges)
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ByteRange {
    /// Get all bytes between x and y inclusive ("bytes=x-y")
    Inclusive(u64, u64),

    /// Get all bytes starting from x ("bytes=x-")
    AllFrom(u64),

    /// Get the last n bytes ("bltes=-n")
    Last(u64),
}

impl fmt::Display for ByteRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ByteRange::Inclusive(start, end) => write!(f, "{}-{}", start, end),
            ByteRange::AllFrom(from) => write!(f, "{}-", from),
            ByteRange::Last(n) => write!(f, "-{}", n),
        }
    }
}

impl FromStr for ByteRange {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut iter = s.splitn(2, '-');
        match (iter.next(), iter.next()) {
            (Some(""), Some(end)) => end.parse().map(ByteRange::Last).or(Err(())),
            (Some(start), Some("")) => start.parse().map(ByteRange::AllFrom).or(Err(())),
            (Some(start), Some(end)) => match (start.parse(), end.parse()) {
                (Ok(start), Ok(end)) if start <= end => Ok(ByteRange::Inclusive(start, end)),
                _ => Err(()),
            },
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteRange, Range};
    use crate::error::TransferError;
    use std::str::FromStr;

    #[test]
    fn test_byte_range_from_str() {
        assert_eq!(
            ByteRange::Last(500),
            Range::from_str("bytes=-500").unwrap().0
        );
        assert_eq!(
            ByteRange::AllFrom(200),
            Range::from_str("bytes=200-").unwrap().0
        );
        assert_eq!(
            ByteRange::Inclusive(200, 500),
            Range::from_str("bytes=200-500").unwrap().0
        );
    }

    fn assert_err_contains(r: Result<Range, TransferError>, msg: &str) {
        let err = r.unwrap_err();
        match err {
            TransferError::InvalidMetaRequest(m) => {
                assert!(m.contains(msg), "'{}' does not contain '{}'", m, msg);
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn test_invalid_byte_range_from_str() {
        assert_err_contains(Range::from_str("bytes=-"), "invalid range header");
        assert_err_contains(Range::from_str("bytes=500-200"), "invalid range header");
        assert_err_contains(
            Range::from_str("bytes=0-200,400-500"),
            "multiple byte ranges not supported for range header",
        );
    }
}
