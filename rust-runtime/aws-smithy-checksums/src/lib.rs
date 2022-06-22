/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Checksum calculation and verification callbacks

use aws_smithy_types::base64;

use bytes::Bytes;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use sha1::Digest;
use std::io::Write;

pub mod body;

// Valid checksum algorithm names
pub const CRC_32_NAME: &str = "crc32";
pub const CRC_32_C_NAME: &str = "crc32c";
pub const SHA_1_NAME: &str = "sha1";
pub const SHA_256_NAME: &str = "sha256";

pub const CRC_32_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-crc32");
pub const CRC_32_C_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-crc32c");
pub const SHA_1_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-sha1");
pub const SHA_256_HEADER_NAME: HeaderName = HeaderName::from_static("x-amz-checksum-sha256");

// Preserved for compatibility purposes. This should never be used by users, only within smithy-rs
const MD5_NAME: &str = "md5";
const MD5_HEADER_NAME: HeaderName = HeaderName::from_static("content-md5");

/// Given a `&str` representing a checksum algorithm, return the corresponding `HeaderName`
/// for that checksum algorithm.
pub fn algorithm_to_header_name(checksum_algorithm: &str) -> HeaderName {
    if checksum_algorithm.eq_ignore_ascii_case(CRC_32_NAME) {
        CRC_32_HEADER_NAME
    } else if checksum_algorithm.eq_ignore_ascii_case(CRC_32_C_NAME) {
        CRC_32_C_HEADER_NAME
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_1_NAME) {
        SHA_1_HEADER_NAME
    } else if checksum_algorithm.eq_ignore_ascii_case(SHA_256_NAME) {
        SHA_256_HEADER_NAME
    } else if checksum_algorithm.eq_ignore_ascii_case(MD5_NAME) {
        MD5_HEADER_NAME
    } else {
        // TODO what's the best way to handle this case?
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
        // TODO what's the best way to handle this case?
        "unknown-checksum-algorithm"
    }
}

/// When a response has to be checksum-verified, we have to check possible headers until we find the
/// header with the precalculated checksum. Because a service may send back multiple headers, we have
/// to check them in order based on how fast each checksum is to calculate.
pub const CHECKSUM_ALGORITHMS_IN_PRIORITY_ORDER: [&str; 4] =
    [CRC_32_C_NAME, CRC_32_NAME, SHA_1_NAME, SHA_256_NAME];

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Checksum algorithms are use to validate the integrity of data. Structs that implement this trait
/// can be used as checksum calculators. This trait requires Send + Sync because these checksums are
/// often used in a threaded context.
pub trait Checksum: Send + Sync {
    /// Given a slice of bytes, update this checksum's internal state.
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError>;
    /// Either return this checksum as a `HeaderMap` containing one HTTP header, or return an error
    /// describing why checksum calculation failed.
    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError>;
    /// Return the `HeaderName` used to represent this checksum algorithm
    fn header_name(&self) -> HeaderName;
    /// "Finalize" this checksum, returning the calculated value as `Bytes` or an error that
    /// occurred during checksum calculation. To print this value in a human-readable hexadecimal
    /// format, you can print it using Rust's builtin [formatter].
    ///
    /// _**NOTE:** typically, "finalizing" a checksum in Rust will take ownership of the checksum
    /// struct. In this method, we clone the checksum's state before finalizing because checksums
    /// may be used in a situation where taking ownership is not possible._
    ///
    /// [formatter]: https://doc.rust-lang.org/std/fmt/trait.UpperHex.html
    fn finalize(&self) -> Result<Bytes, BoxError>;
    /// Return the size of this checksum algorithms resulting checksum, in bytes. For example, the
    /// CRC32 checksum algorithm calculates a 32 bit checksum, so a CRC32 checksum struct
    /// implementing this trait method would return 4.
    fn size(&self) -> u64;
}

/// Create a new `Box<dyn Checksum>` from an algorithm name. Valid algorithm names are defined as
/// `const`s in this module.
pub fn new_checksum(checksum_algorithm: &str) -> Box<dyn Checksum> {
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

#[derive(Debug, Default)]
struct Crc32 {
    hasher: crc32fast::Hasher,
}

impl Crc32 {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.update(bytes);

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            &self.hasher.clone().finalize().to_be_bytes(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        4
    }

    fn header_name() -> HeaderName {
        CRC_32_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("will always produce a valid header value from a CRC32 checksum")
    }
}

impl Checksum for Crc32 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

#[derive(Debug, Default)]
struct Crc32c {
    state: Option<u32>,
}

impl Crc32c {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.state = match self.state {
            Some(crc) => Some(crc32c::crc32c_append(crc, bytes)),
            None => Some(crc32c::crc32c(bytes)),
        };

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            &self.state.unwrap_or_default().to_be_bytes(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        4
    }

    fn header_name() -> HeaderName {
        CRC_32_C_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // If no data was provided to this callback and no CRC was ever calculated, return zero as the checksum.
        let hash = self.state.unwrap_or_default();
        HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("will always produce a valid header value from a CRC32C checksum")
    }
}

impl Checksum for Crc32c {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

#[derive(Debug, Default)]
struct Sha1 {
    hasher: sha1::Sha1,
}

impl Sha1 {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            self.hasher.clone().finalize().as_slice(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        20
    }

    fn header_name() -> HeaderName {
        SHA_1_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("will always produce a valid header value from a SHA-1 checksum")
    }
}

impl Checksum for Sha1 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

#[derive(Debug, Default)]
struct Sha256 {
    hasher: sha2::Sha256,
}

impl Sha256 {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            self.hasher.clone().finalize().as_slice(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        32
    }

    fn header_name() -> HeaderName {
        SHA_256_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("will always produce a valid header value from a SHA-256 checksum")
    }
}

impl Checksum for Sha256 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

#[derive(Debug, Default)]
struct Md5 {
    hasher: md5::Md5,
}

impl Md5 {
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn headers(&self) -> Result<Option<HeaderMap<HeaderValue>>, BoxError> {
        let mut header_map = HeaderMap::new();
        header_map.insert(Self::header_name(), self.header_value());

        Ok(Some(header_map))
    }

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            self.hasher.clone().finalize().as_slice(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        16
    }

    fn header_name() -> HeaderName {
        MD5_HEADER_NAME
    }

    fn header_value(&self) -> HeaderValue {
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("will always produce a valid header value from an MD5 checksum")
    }
}

impl Checksum for Md5 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
    }
    fn headers(
        &self,
    ) -> Result<Option<HeaderMap>, Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::headers(self)
    }
    fn header_name(&self) -> HeaderName {
        Self::header_name()
    }
    fn finalize(&self) -> Result<Bytes, BoxError> {
        Self::finalize(self)
    }
    fn size(&self) -> u64 {
        Self::size()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Crc32, Crc32c, Md5, Sha1, Sha256, CRC_32_C_HEADER_NAME, CRC_32_HEADER_NAME,
        MD5_HEADER_NAME, SHA_1_HEADER_NAME, SHA_256_HEADER_NAME,
    };

    use aws_smithy_types::base64;
    use http::HeaderValue;
    use pretty_assertions::assert_eq;

    const TEST_DATA: &str = r#"test data"#;

    fn base64_encoded_checksum_to_hex_string(header_value: &HeaderValue) -> String {
        let decoded_checksum = base64::decode(header_value.to_str().unwrap()).unwrap();
        let decoded_checksum = decoded_checksum
            .into_iter()
            .map(|byte| format!("{:02X?}", byte))
            .collect::<String>();

        format!("0x{}", decoded_checksum)
    }

    #[test]
    fn test_crc32_checksum() {
        let mut checksum = Crc32::default();
        checksum.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_result = checksum.headers().unwrap().unwrap();
        let encoded_checksum = checksum_result.get(CRC_32_HEADER_NAME).unwrap();
        let decoded_checksum = base64_encoded_checksum_to_hex_string(encoded_checksum);

        let expected_checksum = "0xD308AEB2";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_crc32c_checksum() {
        let mut checksum = Crc32c::default();
        checksum.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_result = checksum.headers().unwrap().unwrap();
        let encoded_checksum = checksum_result.get(CRC_32_C_HEADER_NAME).unwrap();
        let decoded_checksum = base64_encoded_checksum_to_hex_string(encoded_checksum);

        let expected_checksum = "0x3379B4CA";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_sha1_checksum() {
        let mut checksum = Sha1::default();
        checksum.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_result = checksum.headers().unwrap().unwrap();
        let encoded_checksum = checksum_result.get(SHA_1_HEADER_NAME).unwrap();
        let decoded_checksum = base64_encoded_checksum_to_hex_string(encoded_checksum);

        let expected_checksum = "0xF48DD853820860816C75D54D0F584DC863327A7C";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_sha256_checksum() {
        let mut checksum = Sha256::default();
        checksum.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_result = checksum.headers().unwrap().unwrap();
        let encoded_checksum = checksum_result.get(SHA_256_HEADER_NAME).unwrap();
        let decoded_checksum = base64_encoded_checksum_to_hex_string(encoded_checksum);

        let expected_checksum =
            "0x916F0027A575074CE72A331777C3478D6513F786A591BD892DA1A577BF2335F9";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_md5_checksum() {
        let mut checksum = Md5::default();
        checksum.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_result = checksum.headers().unwrap().unwrap();
        let encoded_checksum = checksum_result.get(MD5_HEADER_NAME).unwrap();
        let decoded_checksum = base64_encoded_checksum_to_hex_string(encoded_checksum);

        let expected_checksum = "0xEB733A00C0C9D336E65691A37AB54293";

        assert_eq!(decoded_checksum, expected_checksum);
    }
}
