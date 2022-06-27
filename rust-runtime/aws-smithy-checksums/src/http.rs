/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{BoxError, Checksum, Crc32, Crc32c, Md5, Sha1, Sha256};
use aws_smithy_types::base64;
use http::header::{HeaderMap, HeaderName, HeaderValue};

// Valid checksum algorithm names
pub const CRC_32_NAME: &str = "crc32";
pub const CRC_32_C_NAME: &str = "crc32c";
pub const SHA_1_NAME: &str = "sha1";
pub const SHA_256_NAME: &str = "sha256";

pub static CRC_32_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-crc32");
pub static CRC_32_C_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-crc32c");
pub static SHA_1_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-sha1");
pub static SHA_256_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-sha256");

// Preserved for compatibility purposes. This should never be used by users, only within smithy-rs
pub(crate) const MD5_NAME: &str = "md5";
pub(crate) static MD5_HEADER_NAME: HeaderName = HeaderName::from_static("content-md5");

/// Given a `&str` representing a checksum algorithm, return the corresponding `HeaderName`
/// for that checksum algorithm.
pub fn algorithm_to_header_name(checksum_algorithm: &str) -> HeaderName {
    if checksum_algorithm.eq_ignore_ascii_case(CRC_32_NAME) {
        CRC_32_HEADER_NAME.clone()
    } else if checksum_algorithm.eq_ignore_ascii_case(CRC_32_C_NAME) {
        CRC_32_C_HEADER_NAME.clone()
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_1_NAME) {
        SHA_1_HEADER_NAME.clone()
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_256_NAME) {
        SHA_256_HEADER_NAME.clone()
    } else if checksum_algorithm.eq_ignore_ascii_case(MD5_NAME) {
        MD5_HEADER_NAME.clone()
    } else {
        HeaderName::from_static("x-amz-checksum-unknown")
    }
}

/// Given a `HeaderName` representing a checksum algorithm, return the name of that algorithm
/// as a `&'static str`.
pub fn header_name_to_algorithm(checksum_header_name: &HeaderName) -> &'static str {
    if checksum_header_name == CRC_32_HEADER_NAME {
        CRC_32_NAME
    } else if checksum_header_name == CRC_32_C_HEADER_NAME {
        CRC_32_C_NAME
    } else if checksum_header_name == SHA_1_HEADER_NAME {
        SHA_1_NAME
    } else if checksum_header_name == SHA_256_HEADER_NAME {
        SHA_256_NAME
    } else if checksum_header_name == MD5_HEADER_NAME {
        MD5_NAME
    } else {
        "unknown-checksum-algorithm"
    }
}

/// Create a new `Box<dyn HttpChecksum>` from an algorithm name. Valid algorithm names are defined
/// as `static`s in [this module](crate::http).
pub fn new_from_algorithm(checksum_algorithm: &str) -> Box<dyn HttpChecksum> {
    if checksum_algorithm.eq_ignore_ascii_case(CRC_32_NAME) {
        Box::new(Crc32::default())
    } else if checksum_algorithm.eq_ignore_ascii_case(CRC_32_C_NAME) {
        Box::new(Crc32c::default())
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_1_NAME) {
        Box::new(Sha1::default())
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_256_NAME) {
        Box::new(Sha256::default())
    } else if checksum_algorithm.eq_ignore_ascii_case(MD5_NAME) {
        // It's possible to create an MD5 and we do this in some situations for compatibility.
        // We deliberately hide this from users so that they don't go using it.
        Box::new(Md5::default())
    } else {
        panic!("unsupported checksum algorithm '{}'", checksum_algorithm)
    }
}

/// When a response has to be checksum-verified, we have to check possible headers until we find the
/// header with the precalculated checksum. Because a service may send back multiple headers, we have
/// to check them in order based on how fast each checksum is to calculate.
pub const CHECKSUM_ALGORITHMS_IN_PRIORITY_ORDER: [&str; 4] =
    [CRC_32_C_NAME, CRC_32_NAME, SHA_1_NAME, SHA_256_NAME];

/// Checksum algorithms are use to validate the integrity of data. Structs that implement this trait
/// can be used as checksum calculators. This trait requires Send + Sync because these checksums are
/// often used in a threaded context.
pub trait HttpChecksum: Checksum + Send + Sync {
    /// Either return this checksum as a `HeaderMap` containing one HTTP header, or return an error
    /// describing why checksum calculation failed.
    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError>;
    /// Return the `HeaderName` used to represent this checksum algorithm
    fn header_name(&self) -> HeaderName;
    /// Return the calculated checksum as a base64-encoded `HeaderValue`
    fn header_value(&self) -> HeaderValue;
}

impl HttpChecksum for Crc32 {
    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(self.header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn header_name(&self) -> HeaderName {
        CRC_32_HEADER_NAME.clone()
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("will always produce a valid header value from a CRC32 checksum")
    }
}

impl HttpChecksum for Crc32c {
    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(self.header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn header_name(&self) -> HeaderName {
        CRC_32_C_HEADER_NAME.clone()
    }

    fn header_value(&self) -> HeaderValue {
        // If no data was provided to this callback and no CRC was ever calculated, return zero as the checksum.
        let hash = self.state.unwrap_or_default();
        HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("will always produce a valid header value from a CRC32C checksum")
    }
}

impl HttpChecksum for Sha1 {
    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(self.header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn header_name(&self) -> HeaderName {
        SHA_1_HEADER_NAME.clone()
    }

    fn header_value(&self) -> HeaderValue {
        use sha1::Digest;
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("will always produce a valid header value from a SHA-1 checksum")
    }
}

impl HttpChecksum for Sha256 {
    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(self.header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn header_name(&self) -> HeaderName {
        SHA_256_HEADER_NAME.clone()
    }

    fn header_value(&self) -> HeaderValue {
        use sha2::Digest;
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("will always produce a valid header value from a SHA-256 checksum")
    }
}

impl HttpChecksum for Md5 {
    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(self.header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn header_name(&self) -> HeaderName {
        MD5_HEADER_NAME.clone()
    }

    fn header_value(&self) -> HeaderValue {
        use md5::Digest;
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("will always produce a valid header value from an MD5 checksum")
    }
}
