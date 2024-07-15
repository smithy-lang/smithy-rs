/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::error::Error as StdError;
use std::fmt;
use std::fmt::Formatter;
use std::io::{Error as StdIoError, ErrorKind as StdIoErrorKind};
use tokio::task::JoinError;

#[derive(Debug)]
pub(crate) enum ErrorKind {
    UpperBoundSizeHintRequired,
    OffsetGreaterThanFileSize,
    TaskFailed(JoinError),
    IoError(StdIoError),
}

/// An I/O related error occurred
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    pub(crate) fn upper_bound_size_hint_required() -> Error {
        ErrorKind::UpperBoundSizeHintRequired.into()
    }
}
impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self { kind }
    }
}

impl From<StdIoError> for Error {
    fn from(err: StdIoError) -> Self {
        ErrorKind::IoError(err).into()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::UpperBoundSizeHintRequired => write!(
                f,
                "size hint upper bound (SizeHint::upper) is required but was None"
            ),
            ErrorKind::OffsetGreaterThanFileSize => write!(
                f,
                "offset must be less than or equal to file size but was greater than"
            ),
            ErrorKind::IoError(_) => write!(f, "I/O error"),
            ErrorKind::TaskFailed(_) => write!(f, "task failed"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.kind {
            ErrorKind::UpperBoundSizeHintRequired => None,
            ErrorKind::OffsetGreaterThanFileSize => None,
            ErrorKind::IoError(err) => Some(err as _),
            ErrorKind::TaskFailed(err) => Some(err as _),
        }
    }
}
impl From<Error> for StdIoError {
    fn from(err: Error) -> Self {
        StdIoError::new(StdIoErrorKind::Other, err)
    }
}

impl From<JoinError> for Error {
    fn from(value: JoinError) -> Self {
        ErrorKind::TaskFailed(value).into()
    }
}
