/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Checksum calculation and verification callbacks

use super::BodyCallback;

use aws_smithy_types::base64;

use http::header::{HeaderMap, HeaderName, HeaderValue};
use sha1::Digest;
use std::io::Write;

const CRC_32_NAME: &str = "x-amz-checksum-crc32";
const CRC_32_C_NAME: &str = "x-amz-checksum-crc32c";
const SHA_1_NAME: &str = "x-amz-checksum-sha1";
const SHA_256_NAME: &str = "x-amz-checksum-sha256";

#[derive(Debug, Default)]
struct Crc32ChecksumCallback {
    hasher: crc32fast::Hasher,
}

impl Crc32ChecksumCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), super::BoxError> {
        self.hasher.update(bytes);

        Ok(())
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, super::BoxError> {
        let mut header_map = HeaderMap::new();
        let key = HeaderName::from_static(CRC_32_NAME);
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        let value = HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("base64 will always produce valid header values from checksums");

        header_map.insert(key, value);

        Ok(Some(header_map))
    }
}

impl BodyCallback for Crc32ChecksumCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), super::BoxError> {
        self.update(bytes)
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, super::BoxError> {
        self.trailers()
    }

    fn make_new(&self) -> Box<dyn BodyCallback> {
        Box::new(Crc32ChecksumCallback::default())
    }
}

#[derive(Debug, Default)]
struct Crc32CastagnoliChecksumCallback {
    state: Option<u32>,
}

impl Crc32CastagnoliChecksumCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), super::BoxError> {
        self.state = match self.state {
            Some(crc) => Some(crc32c::crc32c_append(crc, bytes)),
            None => Some(crc32c::crc32c(bytes)),
        };

        Ok(())
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, super::BoxError> {
        let mut header_map = HeaderMap::new();
        let key = HeaderName::from_static(CRC_32_C_NAME);
        // TODO should we send checksums for empty bodies?
        // If no data was provided to this callback and no CRC was ever calculated, return zero as the checksum.
        let hash = self.state.unwrap_or_default();
        let value = HeaderValue::from_str(&base64::encode(u32::to_be_bytes(hash)))
            .expect("base64 will always produce valid header values from checksums");

        header_map.insert(key, value);

        Ok(Some(header_map))
    }
}

impl BodyCallback for Crc32CastagnoliChecksumCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), super::BoxError> {
        self.update(bytes)
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, super::BoxError> {
        self.trailers()
    }

    fn make_new(&self) -> Box<dyn BodyCallback> {
        Box::new(Crc32CastagnoliChecksumCallback::default())
    }
}

#[derive(Debug, Default)]
struct Sha1ChecksumCallback {
    hasher: sha1::Sha1,
}

impl Sha1ChecksumCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), super::BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, super::BoxError> {
        let mut header_map = HeaderMap::new();
        let key = HeaderName::from_static(SHA_1_NAME);
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        let value = HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("base64 will always produce valid header values from checksums");

        header_map.insert(key, value);

        Ok(Some(header_map))
    }
}

impl BodyCallback for Sha1ChecksumCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), super::BoxError> {
        self.update(bytes)
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, super::BoxError> {
        self.trailers()
    }

    fn make_new(&self) -> Box<dyn BodyCallback> {
        Box::new(Sha1ChecksumCallback::default())
    }
}

#[derive(Debug, Default)]
struct Sha256ChecksumCallback {
    hasher: sha2::Sha256,
}

impl Sha256ChecksumCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), super::BoxError> {
        self.hasher.write_all(bytes)?;

        Ok(())
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, super::BoxError> {
        let mut header_map = HeaderMap::new();
        let key = HeaderName::from_static(SHA_256_NAME);
        // We clone the hasher because `Hasher::finalize` consumes `self`
        let hash = self.hasher.clone().finalize();
        let value = HeaderValue::from_str(&base64::encode(&hash[..]))
            .expect("base64 will always produce valid header values from checksums");

        header_map.insert(key, value);

        Ok(Some(header_map))
    }
}

impl BodyCallback for Sha256ChecksumCallback {
    fn update(&mut self, bytes: &[u8]) -> Result<(), super::BoxError> {
        self.update(bytes)
    }

    fn trailers(&self) -> Result<Option<HeaderMap<HeaderValue>>, super::BoxError> {
        self.trailers()
    }

    fn make_new(&self) -> Box<dyn BodyCallback> {
        Box::new(Sha256ChecksumCallback::default())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Crc32CastagnoliChecksumCallback, Crc32ChecksumCallback, Sha1ChecksumCallback,
        Sha256ChecksumCallback, CRC_32_C_NAME, CRC_32_NAME, SHA_1_NAME, SHA_256_NAME,
    };

    use aws_smithy_types::base64;
    use http::HeaderValue;
    use pretty_assertions::assert_eq;

    const TEST_DATA: &str = r#"test data"#;

    fn header_value_as_checksum_string(header_value: &HeaderValue) -> String {
        let decoded_checksum = base64::decode(header_value.to_str().unwrap()).unwrap();
        let decoded_checksum = decoded_checksum
            .into_iter()
            .map(|byte| format!("{:02X?}", byte))
            .collect::<String>();

        format!("0x{}", decoded_checksum)
    }

    #[test]
    fn test_crc32_checksum() {
        let mut checksum_callback = Crc32ChecksumCallback::default();
        checksum_callback.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_callback_result = checksum_callback.trailers().unwrap().unwrap();
        let encoded_checksum = checksum_callback_result.get(CRC_32_NAME).unwrap();
        let decoded_checksum = header_value_as_checksum_string(encoded_checksum);

        let expected_checksum = "0xD308AEB2";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_crc32c_checksum() {
        let mut checksum_callback = Crc32CastagnoliChecksumCallback::default();
        checksum_callback.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_callback_result = checksum_callback.trailers().unwrap().unwrap();
        let encoded_checksum = checksum_callback_result.get(CRC_32_C_NAME).unwrap();
        let decoded_checksum = header_value_as_checksum_string(encoded_checksum);

        let expected_checksum = "0x3379B4CA";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_sha1_checksum() {
        let mut checksum_callback = Sha1ChecksumCallback::default();
        checksum_callback.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_callback_result = checksum_callback.trailers().unwrap().unwrap();
        let encoded_checksum = checksum_callback_result.get(SHA_1_NAME).unwrap();
        let decoded_checksum = header_value_as_checksum_string(encoded_checksum);

        let expected_checksum = "0xF48DD853820860816C75D54D0F584DC863327A7C";

        assert_eq!(decoded_checksum, expected_checksum);
    }

    #[test]
    fn test_sha256_checksum() {
        let mut checksum_callback = Sha256ChecksumCallback::default();
        checksum_callback.update(TEST_DATA.as_bytes()).unwrap();
        let checksum_callback_result = checksum_callback.trailers().unwrap().unwrap();
        let encoded_checksum = checksum_callback_result.get(SHA_256_NAME).unwrap();
        let decoded_checksum = header_value_as_checksum_string(encoded_checksum);

        let expected_checksum =
            "0x916F0027A575074CE72A331777C3478D6513F786A591BD892DA1A577BF2335F9";

        assert_eq!(decoded_checksum, expected_checksum);
    }
}
