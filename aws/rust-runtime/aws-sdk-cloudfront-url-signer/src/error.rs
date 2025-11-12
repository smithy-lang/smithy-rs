/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Error types for CloudFront URL signing operations.

use std::fmt;

/// Errors that can occur during CloudFront URL signing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SigningError {
    /// Invalid private key format or parsing failure.
    InvalidKey(String),
    /// Invalid policy configuration or validation failure.
    InvalidPolicy(String),
    /// Invalid input parameters.
    InvalidInput(String),
    /// Cryptographic signing operation failed.
    SigningFailure(String),
}

impl fmt::Display for SigningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SigningError::InvalidKey(msg) => write!(f, "invalid key: {msg}"),
            SigningError::InvalidPolicy(msg) => write!(f, "invalid policy: {msg}"),
            SigningError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            SigningError::SigningFailure(msg) => write!(f, "signing failure: {msg}"),
        }
    }
}

impl std::error::Error for SigningError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_key_display() {
        let err = SigningError::InvalidKey("malformed PEM".to_string());
        assert_eq!(err.to_string(), "invalid key: malformed PEM");
    }

    #[test]
    fn test_invalid_policy_display() {
        let err = SigningError::InvalidPolicy("missing expiration".to_string());
        assert_eq!(err.to_string(), "invalid policy: missing expiration");
    }

    #[test]
    fn test_invalid_input_display() {
        let err = SigningError::InvalidInput("empty URL".to_string());
        assert_eq!(err.to_string(), "invalid input: empty URL");
    }

    #[test]
    fn test_signing_failure_display() {
        let err = SigningError::SigningFailure("RSA operation failed".to_string());
        assert_eq!(err.to_string(), "signing failure: RSA operation failed");
    }

    #[test]
    fn test_error_trait_implemented() {
        let err = SigningError::InvalidKey("test".to_string());
        let _: &dyn std::error::Error = &err;
    }
}
