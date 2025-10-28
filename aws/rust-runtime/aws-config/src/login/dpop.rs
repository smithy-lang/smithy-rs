/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::login::cache::LoginTokenError;
use aws_smithy_json::serialize::JsonObjectWriter;
use aws_smithy_types::Number;
use p256::ecdsa::signature::RandomizedSigner;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::SecretKey;
use rand::SeedableRng;
use std::time::SystemTime;

pub(super) struct Header {
    pub x_b64: String,
    pub y_b64: String,
}

impl Header {
    fn to_json(&self) -> String {
        let mut header = String::new();
        let mut writer = JsonObjectWriter::new(&mut header);
        writer.key("typ").string("dpop+jwt");
        writer.key("alg").string("ES256");
        let mut jwk = writer.key("jwk").start_object();
        jwk.key("kty").string("EC");
        jwk.key("x").string(&self.x_b64);
        jwk.key("y").string(&self.y_b64);
        jwk.key("crv").string("P-256");
        jwk.finish();
        writer.finish();
        header
    }
}

pub(super) struct Payload {
    pub jti: String,
    pub iat: u64,
    pub htu: String,
}

impl Payload {
    fn to_json(&self) -> String {
        let mut payload = String::new();
        let mut writer = JsonObjectWriter::new(&mut payload);
        writer.key("jti").string(&self.jti);
        writer.key("htm").string("POST");
        writer.key("htu").string(&self.htu);
        writer.key("iat").number(Number::PosInt(self.iat));
        writer.finish();
        payload
    }
}

fn header(private_key: &SecretKey) -> Result<Header, LoginTokenError> {
    let public_key = private_key.public_key();
    let point = public_key.to_encoded_point(false);

    let x_bytes = point
        .x()
        .ok_or_else(|| LoginTokenError::other("invalid private key: x coordinate", None))?;
    let y_bytes = point
        .y()
        .ok_or_else(|| LoginTokenError::other("invalid private key: y coordinate", None))?;

    Ok(Header {
        x_b64: base64_simd::URL_SAFE_NO_PAD.encode_to_string(x_bytes),
        y_b64: base64_simd::URL_SAFE_NO_PAD.encode_to_string(y_bytes),
    })
}

pub(super) fn payload(jti: String, iat: u64, htu: &str) -> Payload {
    Payload {
        jti,
        iat,
        htu: htu.to_string(),
    }
}

fn build_message(header: &Header, payload: &Payload) -> String {
    let header_json = header.to_json();
    let payload_json = payload.to_json();

    let header_b64 = base64_simd::URL_SAFE_NO_PAD.encode_to_string(header_json.as_bytes());
    let payload_b64 = base64_simd::URL_SAFE_NO_PAD.encode_to_string(payload_json.as_bytes());
    format!("{}.{}", header_b64, payload_b64)
}

fn sign(message: &str, private_key: &SecretKey) -> Result<String, LoginTokenError> {
    let signing_key = SigningKey::from(private_key);
    let mut rng = rand::rngs::StdRng::from_entropy();
    let signature: Signature = signing_key.sign_with_rng(&mut rng, message.as_bytes());
    let signature_b64 = base64_simd::URL_SAFE_NO_PAD.encode_to_string(signature.to_bytes());

    Ok(format!("{}.{}", message, signature_b64))
}

/// Calculate DPoP HTTP header using the private key.
///
/// See [RFC 9449: OAuth 2.0 Demonstrating Proof of Possession (DPoP)](https://datatracker.ietf.org/doc/html/rfc9449)
pub(super) fn calculate(
    private_key_pem: &str,
    endpoint: &str,
    now: SystemTime,
) -> Result<String, LoginTokenError> {
    let private_key = SecretKey::from_sec1_pem(private_key_pem)
        .map_err(|e| LoginTokenError::other("invalid secret key", Some(e.into())))?;
    let header = header(&private_key)?;
    let jti = uuid::Uuid::new_v4().to_string();
    let iat = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| LoginTokenError::other("system time before UNIX epoch", Some(e.into())))?
        .as_secs();
    let payload = payload(jti, iat, endpoint);
    let message = build_message(&header, &payload);
    sign(&message, &private_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const TEST_KEY: &str = "-----BEGIN EC PRIVATE KEY-----\nMHcCAQEEIBMB/RwQERsVoqWRQG4zK8CnaAa5dfrpbm+9tFdBh3z4oAoGCCqGSM49\nAwEHoUQDQgAEWb1VLi1EA2hJaTz4yYuxSELvY+1GAfL+8rUTCAdiFid87Bf6GY+s\n2+1RpqDv0RpZiDIMCrZrsAh+RK9S3QCaGA==\n-----END EC PRIVATE KEY-----\n";

    #[test]
    fn test_header_extracts_coordinates() {
        let private_key = SecretKey::from_sec1_pem(TEST_KEY).unwrap();
        let h = header(&private_key).unwrap();
        assert_eq!(h.x_b64, "Wb1VLi1EA2hJaTz4yYuxSELvY-1GAfL-8rUTCAdiFic");
        assert_eq!(h.y_b64, "fOwX-hmPrNvtUaag79EaWYgyDAq2a7AIfkSvUt0Amhg");
    }

    #[test]
    fn test_build_message() {
        let h = Header {
            x_b64: "test_x".to_string(),
            y_b64: "test_y".to_string(),
        };
        let p = payload(
            "test-jti".to_string(),
            1651516560,
            "https://example.com/token",
        );
        let message = build_message(&h, &p);
        let parts: Vec<&str> = message.split('.').collect();
        assert_eq!(parts.len(), 2);

        let header_json = String::from_utf8(
            base64_simd::URL_SAFE_NO_PAD
                .decode_to_vec(parts[0])
                .unwrap(),
        )
        .unwrap();
        assert!(header_json.contains("dpop+jwt"));
        assert!(header_json.contains("test_x"));

        let payload_json = String::from_utf8(
            base64_simd::URL_SAFE_NO_PAD
                .decode_to_vec(parts[1])
                .unwrap(),
        )
        .unwrap();
        assert!(payload_json.contains("test-jti"));
        assert!(payload_json.contains("https://example.com/token"));
    }

    #[test]
    fn test_calculate_valid_key() {
        let endpoint = "https://signin.aws.amazon.com/v1/token";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1651516560);

        let result = calculate(TEST_KEY, endpoint, now);
        assert!(result.is_ok());

        let dpop = result.unwrap();
        let parts: Vec<&str> = dpop.split('.').collect();
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn test_calculate_invalid_key() {
        let endpoint = "https://signin.aws.amazon.com/v1/token";
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1651516560);

        let result = calculate("invalid_key", endpoint, now);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid secret key"));
    }
}
