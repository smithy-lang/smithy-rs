/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::error::SigningError;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::RsaPrivateKey;
use sha1::{Digest, Sha1};

#[cfg(feature = "rt-tokio")]
use std::path::Path;

#[derive(Debug)]
pub(crate) enum PrivateKey {
    Rsa(RsaPrivateKey),
    Ecdsa(p256::ecdsa::SigningKey),
}

impl PrivateKey {
    pub(crate) fn from_pem(bytes: &[u8]) -> Result<Self, SigningError> {
        let pem_str = std::str::from_utf8(bytes)
            .map_err(|e| SigningError::InvalidKey { source: e.into() })?;

        // Try PKCS#1 RSA first (most common for RSA)
        if let Ok(key) = RsaPrivateKey::from_pkcs1_pem(pem_str) {
            return Ok(PrivateKey::Rsa(key));
        }

        // Try PKCS#8 (supports both RSA and ECDSA)
        if let Ok(key) = p256::ecdsa::SigningKey::from_pkcs8_pem(pem_str) {
            return Ok(PrivateKey::Ecdsa(key));
        }

        // Try PKCS#8 for RSA as fallback
        use p256::pkcs8::DecodePrivateKey;
        if let Ok(key) = RsaPrivateKey::from_pkcs8_pem(pem_str) {
            return Ok(PrivateKey::Rsa(key));
        }

        Err(SigningError::InvalidKey {
            source: "Key must be RSA (PKCS#1 or PKCS#8) or ECDSA P-256 (PKCS#8)".into(),
        })
    }

    #[cfg(feature = "rt-tokio")]
    pub async fn from_pem_file(path: impl AsRef<Path>) -> Result<Self, SigningError> {
        let bytes = tokio::fs::read(path.as_ref())
            .await
            .map_err(|e| SigningError::InvalidKey { source: e.into() })?;

        Self::from_pem(&bytes)
    }

    pub(crate) fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SigningError> {
        match self {
            PrivateKey::Rsa(key) => {
                let mut hasher = Sha1::new();
                hasher.update(message);
                let digest = hasher.finalize();

                let signature = key
                    .sign(rsa::Pkcs1v15Sign::new::<Sha1>(), &digest)
                    .map_err(|e| SigningError::SigningFailure { source: e.into() })?;

                Ok(signature)
            }
            PrivateKey::Ecdsa(key) => {
                use p256::ecdsa::signature::Signer;
                use sha2::Sha256;

                let mut hasher = Sha256::new();
                hasher.update(message);
                let digest = hasher.finalize();

                let signature: p256::ecdsa::Signature = key
                    .try_sign(&digest)
                    .map_err(|e| SigningError::SigningFailure { source: e.into() })?;

                Ok(signature.to_vec())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_RSA_KEY_PEM: &[u8] = b"-----BEGIN RSA PRIVATE KEY-----
MIIBPAIBAAJBANW8WjQksUoX/7nwOfRDNt1XQpLCueHoXSt91MASMOSAqpbzZvXO
g2hW2gCFUIFUPCByMXPoeRe6iUZ5JtjepssCAwEAAQJBALR7ybwQY/lKTLKJrZab
D4BXCCt/7ZFbMxnftsC+W7UHef4S4qFW8oOOLeYfmyGZK1h44rXf2AIp4PndKUID
1zECIQD1suunYw5U22Pa0+2dFThp1VMXdVbPuf/5k3HT2/hSeQIhAN6yX0aT/N6G
gb1XlBKw6GQvhcM0fXmP+bVXV+RtzFJjAiAP+2Z2yeu5u1egeV6gdCvqPnUcNobC
FmA/NMcXt9xMSQIhALEMMJEFAInNeAIXSYKeoPNdkMPDzGnD3CueuCLEZCevAiEA
j+KnJ7pJkTvOzFwE8RfNLli9jf6/OhyYaLL4et7Ng5k=
-----END RSA PRIVATE KEY-----";

    const TEST_ECDSA_KEY_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg4//aTM1/HqiVWagy
01cAx3EaegJ0Y5KLRoTtub8T8EWhRANCAARV/wa477wYpyWB5LCrCdS5M9bEAvD+
VORtjoydSpheKlsa+gE4PcFG88G2gE1Lilb8f6wEq/Lz+5kFa2S8gZmb
-----END PRIVATE KEY-----";

    #[test]
    fn test_from_pem_invalid() {
        let result = PrivateKey::from_pem(b"invalid pem data");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SigningError::InvalidKey { .. }
        ));
    }

    #[test]
    fn test_rsa_key_parsing() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY_PEM).expect("valid RSA key");
        assert!(matches!(key, PrivateKey::Rsa(_)));
    }

    #[test]
    fn test_ecdsa_key_parsing() {
        let key = PrivateKey::from_pem(TEST_ECDSA_KEY_PEM).expect("valid ECDSA key");
        assert!(matches!(key, PrivateKey::Ecdsa(_)));
    }

    #[test]
    fn test_rsa_sign() {
        let key = PrivateKey::from_pem(TEST_RSA_KEY_PEM).expect("valid test key");
        let message = b"test message";
        let signature = key.sign(message).expect("signing should succeed");
        assert!(!signature.is_empty());
    }

    #[test]
    fn test_ecdsa_sign() {
        let key = PrivateKey::from_pem(TEST_ECDSA_KEY_PEM).expect("valid test key");
        let message = b"test message";
        let signature = key.sign(message).expect("signing should succeed");
        assert!(!signature.is_empty());
    }
}
