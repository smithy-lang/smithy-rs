/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Checksum calculation and verification callbacks

use bytes::Bytes;
use std::io::Write;

pub mod body;
pub mod http;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Checksum algorithms are use to validate the integrity of data. Structs that implement this trait
/// can be used as checksum calculators. This trait requires Send + Sync because these checksums are
/// often used in a threaded context.
pub trait Checksum: Send + Sync {
    /// Given a slice of bytes, update this checksum's internal state.
    fn update(&mut self, bytes: &[u8]) -> Result<(), BoxError>;
    /// "Finalize" this checksum, returning the calculated value as `Bytes` or an error that
    /// occurred during checksum calculation. To print this value in a human-readable hexadecimal
    /// format, you can print it using Rust's builtin [formatter].
    ///
    /// _**NOTE:** typically in Rust, "finalizing" a checksum in Rust will take ownership of the
    /// checksum struct. In this method, we clone the checksum's state before finalizing because
    /// these checksums may be used in a situation where taking ownership is not possible._
    ///
    /// [formatter]: https://doc.rust-lang.org/std/fmt/trait.UpperHex.html
    fn finalize(&self) -> Result<Bytes, BoxError>;
    /// Return the size of this checksum algorithms resulting checksum, in bytes. For example, the
    /// CRC32 checksum algorithm calculates a 32 bit checksum, so a CRC32 checksum struct
    /// implementing this trait method would return 4.
    fn size(&self) -> u64;
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

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            &self.hasher.clone().finalize().to_be_bytes(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        4
    }
}

impl Checksum for Crc32 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
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

    fn finalize(&self) -> Result<Bytes, BoxError> {
        Ok(Bytes::copy_from_slice(
            &self.state.unwrap_or_default().to_be_bytes(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        4
    }
}

impl Checksum for Crc32c {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
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

    fn finalize(&self) -> Result<Bytes, BoxError> {
        use sha1::Digest;
        Ok(Bytes::copy_from_slice(
            self.hasher.clone().finalize().as_slice(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        use sha1::Digest;
        sha1::Sha1::output_size() as u64
    }
}

impl Checksum for Sha1 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
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

    fn finalize(&self) -> Result<Bytes, BoxError> {
        use sha2::Digest;
        Ok(Bytes::copy_from_slice(
            self.hasher.clone().finalize().as_slice(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        use sha2::Digest;
        sha2::Sha256::output_size() as u64
    }
}

impl Checksum for Sha256 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
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

    fn finalize(&self) -> Result<Bytes, BoxError> {
        use md5::Digest;
        Ok(Bytes::copy_from_slice(
            self.hasher.clone().finalize().as_slice(),
        ))
    }

    // Size of the checksum in bytes
    fn size() -> u64 {
        use md5::Digest;
        md5::Md5::output_size() as u64
    }
}

impl Checksum for Md5 {
    fn update(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), Box<(dyn std::error::Error + Send + Sync + 'static)>> {
        Self::update(self, bytes)
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
        http::{
            CRC_32_C_HEADER_NAME, CRC_32_HEADER_NAME, MD5_HEADER_NAME, SHA_1_HEADER_NAME,
            SHA_256_HEADER_NAME,
        },
        Crc32, Crc32c, Md5, Sha1, Sha256,
    };

    use crate::http::HttpChecksum;
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
        let encoded_checksum = checksum_result.get(&CRC_32_HEADER_NAME).unwrap();
        let decoded_checksum = base64_encoded_checksum_to_hex_string(encoded_checksum);

        let expected_checksum = "0xD308AEB2";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_crc32c_checksum() {
        let mut checksum = Crc32c::default();
        checksum.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_result = checksum.headers().unwrap().unwrap();
        let encoded_checksum = checksum_result.get(&CRC_32_C_HEADER_NAME).unwrap();
        let decoded_checksum = base64_encoded_checksum_to_hex_string(encoded_checksum);

        let expected_checksum = "0x3379B4CA";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_sha1_checksum() {
        let mut checksum = Sha1::default();
        checksum.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_result = checksum.headers().unwrap().unwrap();
        let encoded_checksum = checksum_result.get(&SHA_1_HEADER_NAME).unwrap();
        let decoded_checksum = base64_encoded_checksum_to_hex_string(encoded_checksum);

        let expected_checksum = "0xF48DD853820860816C75D54D0F584DC863327A7C";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_sha256_checksum() {
        let mut checksum = Sha256::default();
        checksum.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_result = checksum.headers().unwrap().unwrap();
        let encoded_checksum = checksum_result.get(&SHA_256_HEADER_NAME).unwrap();
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
        let encoded_checksum = checksum_result.get(&MD5_HEADER_NAME).unwrap();
        let decoded_checksum = base64_encoded_checksum_to_hex_string(encoded_checksum);

        let expected_checksum = "0xEB733A00C0C9D336E65691A37AB54293";

        assert_eq!(decoded_checksum, expected_checksum);
    }
}
