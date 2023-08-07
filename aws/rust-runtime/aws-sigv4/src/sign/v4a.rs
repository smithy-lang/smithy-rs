/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use bytes::{BufMut, BytesMut};
use num_bigint::BigInt;
use once_cell::sync::Lazy;
use p256::ecdsa::{signature::Signer, DerSignature, SigningKey};
use std::io::Write;
use zeroize::Zeroizing;

const ALGORITHM: &[u8] = b"AWS4-ECDSA-P256-SHA256";
static BIG_N_MINUS_2: Lazy<BigInt> = Lazy::new(|| {
    const ORDER: &[u32] = &[
        0xFFFFFFFF, 0x00000000, 0xFFFFFFFF, 0xFFFFFFFF, 0xBCE6FAAD, 0xA7179E84, 0xF3B9CAC2,
        0xFC632551,
    ];
    let big_n = BigInt::from_slice(num_bigint::Sign::Plus, ORDER);
    big_n - BigInt::from(2i32)
});

/// Calculates a Sigv4a signature
pub fn calculate_signature(signing_key: &SigningKey, string_to_sign: &[u8]) -> String {
    let signature: DerSignature = signing_key.sign(string_to_sign);
    hex::encode(signature.as_ref())
}

/// Generates a signing key for Sigv4a.
pub fn generate_signing_key(access_key: &str, secret_access_key: &str) -> SigningKey {
    // Capacity is the secret access key length plus the length of "AWS4A"
    let mut input_key = Zeroizing::new(Vec::with_capacity(secret_access_key.len() + 5));
    write!(input_key, "AWS4A{secret_access_key}").unwrap();

    // Capacity is the access key length plus the counter byte
    let mut kdf_context = Zeroizing::new(Vec::with_capacity(access_key.len() + 1));
    let mut counter = Zeroizing::new(1u8);
    let key = loop {
        write!(kdf_context, "{access_key}").unwrap();
        kdf_context.push(*counter);

        let mut fis = ALGORITHM.to_vec();
        fis.push(0);
        fis.append(&mut kdf_context);
        fis.put_i32(256);

        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &input_key);

        let mut buf = BytesMut::new();
        buf.put_i32(1);
        buf.put_slice(&fis);
        let tag = ring::hmac::sign(&key, &buf);
        let tag = &tag.as_ref()[0..32];

        let k0 = BigInt::from_bytes_be(num_bigint::Sign::Plus, tag);

        // It would be more secure for this to be a constant time comparison, but because this
        // is for client usage, that's not as big a deal.
        if k0 <= *BIG_N_MINUS_2 {
            let pk = k0 + BigInt::from(1i32);
            let d = Zeroizing::new(pk.to_bytes_be().1);
            break SigningKey::from_slice(&d).unwrap();
        }

        *counter = counter
            .checked_add(1)
            .expect("counter will never get to 255");
    };

    key
}
