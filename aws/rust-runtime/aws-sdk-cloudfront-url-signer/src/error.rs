/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::error::Error as StdError;
use std::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum SigningError {
    InvalidKey {
        source: Box<dyn StdError + Send + Sync>,
    },
    InvalidPolicy {
        message: String,
    },
    InvalidInput {
        message: String,
    },
    SigningFailure {
        source: Box<dyn StdError + Send + Sync>,
    },
}

impl fmt::Display for SigningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey { .. } => write!(f, "invalid private key"),
            Self::InvalidPolicy { message } => write!(f, "invalid policy: {message}"),
            Self::InvalidInput { message } => write!(f, "invalid input: {message}"),
            Self::SigningFailure { .. } => write!(f, "signing operation failed"),
        }
    }
}

impl StdError for SigningError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            SigningError::InvalidKey { source } => Some(source.as_ref()),
            SigningError::InvalidPolicy { .. } => None,
            SigningError::InvalidInput { .. } => None,
            SigningError::SigningFailure { source } => Some(source.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_key_display() {
        let err = SigningError::InvalidKey {
            source: "test error".into(),
        };
        assert_eq!(err.to_string(), "invalid private key");
        assert!(err.source().is_some());
    }

    #[test]
    fn test_invalid_policy_display() {
        let err = SigningError::InvalidPolicy {
            message: "missing expires_at".to_string(),
        };
        assert_eq!(err.to_string(), "invalid policy: missing expires_at");
        assert!(err.source().is_none());
    }

    #[test]
    fn test_invalid_input_display() {
        let err = SigningError::InvalidInput {
            message: "empty URL".to_string(),
        };
        assert_eq!(err.to_string(), "invalid input: empty URL");
        assert!(err.source().is_none());
    }

    #[test]
    fn test_signing_failure_display() {
        let err = SigningError::SigningFailure {
            source: "RSA error".into(),
        };
        assert_eq!(err.to_string(), "signing operation failed");
        assert!(err.source().is_some());
    }
}
