/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Opaque error type used by generated code and runtime crates

use crate::error::display_context::DisplayErrorContext;
use std::error::Error;
use std::fmt;

/// Opaque error type
///
/// Provides a `Display` implementation to show the formatted contents of the inner error type,
/// but does not allow downcasting of the inner error type.
#[derive(Debug)]
pub struct OpaqueError(Box<dyn Error + Send + Sync + 'static>);

impl OpaqueError {
    /// Creates a new `OpaqueError` around the given `inner` error
    pub fn new(inner: impl Into<Box<dyn Error + Send + Sync + 'static>>) -> Self {
        Self(inner.into())
    }
}

impl fmt::Display for OpaqueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        DisplayErrorContext(self.0.as_ref()).fmt(f)
    }
}

impl Error for OpaqueError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // Intentionally don't reveal the source since this is an opaque error type
        None
    }
}

impl From<String> for OpaqueError {
    fn from(string: String) -> Self {
        Self::new(string)
    }
}

impl From<&str> for OpaqueError {
    fn from(string: &str) -> Self {
        Self::new(string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error as IOError, ErrorKind as IOErrorKind};

    #[test]
    fn send_sync() {
        fn verify_send_sync<T: Send + Sync>(_thing: Option<T>) {}
        verify_send_sync::<Option<OpaqueError>>(None);
    }

    #[test]
    fn source() {
        use std::error::Error as _;
        let err = OpaqueError::new(IOError::new(IOErrorKind::Other, "test"));
        assert!(err.source().is_none());
        assert_eq!("test", format!("{}", err));
    }
}
