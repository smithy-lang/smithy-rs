/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;

#[derive(Debug)]
pub(crate) enum ErrorKind {
    InvalidKey,
    InvalidPolicy,
    InvalidInput,
    SigningFailure,
}

/// Error type for CloudFront signing operations
#[derive(Debug)]
pub struct SigningError {
    kind: ErrorKind,
    source: Option<Box<dyn StdError + Send + Sync>>,
    message: Option<Cow<'static, str>>,
}

impl SigningError {
    pub(crate) fn new(
        kind: ErrorKind,
        source: Option<Box<dyn StdError + Send + Sync>>,
        message: Option<Cow<'static, str>>,
    ) -> Self {
        Self {
            kind,
            source,
            message,
        }
    }

    pub(crate) fn invalid_key(source: impl Into<Box<dyn StdError + Send + Sync>>) -> Self {
        Self::new(ErrorKind::InvalidKey, Some(source.into()), None)
    }

    pub(crate) fn invalid_policy(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(ErrorKind::InvalidPolicy, None, Some(message.into()))
    }

    pub(crate) fn invalid_input(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(ErrorKind::InvalidInput, None, Some(message.into()))
    }

    pub(crate) fn signing_failure(source: impl Into<Box<dyn StdError + Send + Sync>>) -> Self {
        Self::new(ErrorKind::SigningFailure, Some(source.into()), None)
    }
}

impl fmt::Display for SigningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ErrorKind::InvalidKey => write!(f, "invalid private key"),
            ErrorKind::InvalidPolicy => {
                write!(f, "invalid policy")?;
                if let Some(ref msg) = self.message {
                    write!(f, ": {msg}")?;
                }
                Ok(())
            }
            ErrorKind::InvalidInput => {
                write!(f, "invalid input")?;
                if let Some(ref msg) = self.message {
                    write!(f, ": {msg}")?;
                }
                Ok(())
            }
            ErrorKind::SigningFailure => write!(f, "signing operation failed"),
        }
    }
}

impl StdError for SigningError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_ref().map(|e| e.as_ref() as _)
    }
}

impl From<ErrorKind> for SigningError {
    fn from(kind: ErrorKind) -> Self {
        Self::new(kind, None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_key_display() {
        let err = SigningError::invalid_key("test error");
        assert_eq!(err.to_string(), "invalid private key");
        assert!(err.source().is_some());
    }

    #[test]
    fn test_invalid_policy_display() {
        let err = SigningError::invalid_policy("missing expires_at");
        assert_eq!(err.to_string(), "invalid policy: missing expires_at");
        assert!(err.source().is_none());
    }

    #[test]
    fn test_invalid_input_display() {
        let err = SigningError::invalid_input("empty URL");
        assert_eq!(err.to_string(), "invalid input: empty URL");
        assert!(err.source().is_none());
    }

    #[test]
    fn test_signing_failure_display() {
        let err = SigningError::signing_failure("RSA error");
        assert_eq!(err.to_string(), "signing operation failed");
        assert!(err.source().is_some());
    }
}
