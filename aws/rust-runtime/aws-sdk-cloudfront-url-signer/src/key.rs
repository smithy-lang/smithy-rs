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
pub(crate) struct PrivateKey {
    key: RsaPrivateKey,
}

impl PrivateKey {
    pub(crate) fn from_pem(bytes: &[u8]) -> Result<Self, SigningError> {
        let key = RsaPrivateKey::from_pkcs1_pem(
            std::str::from_utf8(bytes)
                .map_err(|e| SigningError::InvalidKey { source: e.into() })?,
        )
        .map_err(|e| SigningError::InvalidKey { source: e.into() })?;

        Ok(Self { key })
    }

    #[cfg(feature = "rt-tokio")]
    pub async fn from_pem_file(path: impl AsRef<Path>) -> Result<Self, SigningError> {
        let bytes = tokio::fs::read(path.as_ref())
            .await
            .map_err(|e| SigningError::InvalidKey { source: e.into() })?;

        Self::from_pem(&bytes)
    }

    pub(crate) fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SigningError> {
        let mut hasher = Sha1::new();
        hasher.update(message);
        let digest = hasher.finalize();

        let signature = self
            .key
            .sign(rsa::Pkcs1v15Sign::new::<Sha1>(), &digest)
            .map_err(|e| SigningError::SigningFailure { source: e.into() })?;

        Ok(signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY_PEM: &[u8] = b"-----BEGIN RSA PRIVATE KEY-----
MIIBPAIBAAJBANW8WjQksUoX/7nwOfRDNt1XQpLCueHoXSt91MASMOSAqpbzZvXO
g2hW2gCFUIFUPCByMXPoeRe6iUZ5JtjepssCAwEAAQJBALR7ybwQY/lKTLKJrZab
D4BXCCt/7ZFbMxnftsC+W7UHef4S4qFW8oOOLeYfmyGZK1h44rXf2AIp4PndKUID
1zECIQD1suunYw5U22Pa0+2dFThp1VMXdVbPuf/5k3HT2/hSeQIhAN6yX0aT/N6G
gb1XlBKw6GQvhcM0fXmP+bVXV+RtzFJjAiAP+2Z2yeu5u1egeV6gdCvqPnUcNobC
FmA/NMcXt9xMSQIhALEMMJEFAInNeAIXSYKeoPNdkMPDzGnD3CueuCLEZCevAiEA
j+KnJ7pJkTvOzFwE8RfNLli9jf6/OhyYaLL4et7Ng5k=
-----END RSA PRIVATE KEY-----";

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
    fn test_sign() {
        let key = PrivateKey::from_pem(TEST_KEY_PEM).expect("valid test key");
        let message = b"test message";
        let signature = key.sign(message).expect("signing should succeed");
        assert!(!signature.is_empty());
    }
}
