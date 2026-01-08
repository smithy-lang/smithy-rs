/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Integration test to verify CRC checksums work correctly across architectures.
//! This test specifically exercises crc-fast to catch issues like the 1.4 SIGILL bug.

use aws_smithy_checksums::ChecksumAlgorithm;
use bytes::Bytes;

#[test]
fn test_crc32_checksum() {
    let data = Bytes::from_static(b"hello world");
    let mut hasher = ChecksumAlgorithm::Crc32.into_impl();
    hasher.update(&data);
    let checksum = hasher.finalize();

    // Verify we get a valid checksum (not testing exact value, just that it doesn't crash)
    assert!(!checksum.is_empty());
}

#[test]
fn test_crc32c_checksum() {
    let data = Bytes::from_static(b"hello world");
    let mut hasher = ChecksumAlgorithm::Crc32c.into_impl();
    hasher.update(&data);
    let checksum = hasher.finalize();

    assert!(!checksum.is_empty());
}

#[test]
fn test_crc64_checksum() {
    let data = Bytes::from_static(b"hello world");
    let mut hasher = ChecksumAlgorithm::Crc64Nvme.into_impl();
    hasher.update(&data);
    let checksum = hasher.finalize();

    assert!(!checksum.is_empty());
}

#[test]
fn test_large_data_crc32() {
    // Test with larger data to ensure AVX-512 code paths are exercised
    let data = vec![0u8; 1024 * 1024]; // 1MB
    let data = Bytes::from(data);
    let mut hasher = ChecksumAlgorithm::Crc32.into_impl();
    hasher.update(&data);
    let checksum = hasher.finalize();

    assert!(!checksum.is_empty());
}

#[test]
fn test_large_data_crc64() {
    // Test with larger data to ensure AVX-512 code paths are exercised
    let data = vec![0u8; 1024 * 1024]; // 1MB
    let data = Bytes::from(data);
    let mut hasher = ChecksumAlgorithm::Crc64Nvme.into_impl();
    hasher.update(&data);
    let checksum = hasher.finalize();

    assert!(!checksum.is_empty());
}
