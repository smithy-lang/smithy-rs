/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt;

/// Errors related to a token bucket.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    /// A token was requested but there were no tokens left in the bucket.
    NoTokens,
    /// This error should never occur and is a bug. Please report it.
    Bug(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ErrorKind::*;
        match &self.kind {
            NoTokens => write!(f, "No more tokens are left in the bucket."),
            Bug(msg) => write!(f, "you've encountered a bug that needs reporting: {}", msg),
        }
    }
}

impl std::error::Error for Error {}
