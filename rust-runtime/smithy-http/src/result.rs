/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::operation;
use smithy_types::retry::ErrorKind;
use std::error::Error;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};

type BoxError = Box<dyn Error + Send + Sync>;

/// Successful SDK Result
#[derive(Debug)]
pub struct SdkSuccess<O> {
    pub raw: operation::Response,
    pub parsed: O,
}

/// Failed SDK Result
#[derive(Debug)]
pub enum SdkError<E, R = operation::Response> {
    /// The request failed during construction. It was not dispatched over the network.
    ConstructionFailure(BoxError),

    /// The request failed during dispatch. An HTTP response was not received. The request MAY
    /// have been sent.
    DispatchFailure(ClientError),

    /// A response was received but it was not parseable according the the protocol (for example
    /// the server hung up while the body was being read)
    ResponseError { err: BoxError, raw: R },

    /// An error response was received from the service
    ServiceError { err: E, raw: R },
}

#[derive(Debug)]
pub struct ClientError {
    err: BoxError,
    kind: ClientErrorKind,
}

impl Display for ClientError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.err)
    }
}

impl Error for ClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.err.as_ref())
    }
}

impl ClientError {
    pub fn timeout(err: BoxError) -> Self {
        Self {
            err,
            kind: ClientErrorKind::Timeout,
        }
    }

    pub fn user(err: BoxError) -> Self {
        Self {
            err,
            kind: ClientErrorKind::User,
        }
    }

    pub fn io(err: BoxError) -> Self {
        Self {
            err,
            kind: ClientErrorKind::Io,
        }
    }

    pub fn other(err: BoxError, kind: Option<ErrorKind>) -> Self {
        Self {
            err,
            kind: ClientErrorKind::Other(kind),
        }
    }

    pub fn is_io(&self) -> bool {
        matches!(self.kind, ClientErrorKind::Io)
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self.kind, ClientErrorKind::Timeout)
    }

    pub fn is_user(&self) -> bool {
        matches!(self.kind, ClientErrorKind::User)
    }

    pub fn is_other(&self) -> Option<ErrorKind> {
        match &self.kind {
            ClientErrorKind::Other(ek) => *ek,
            _ => None,
        }
    }
}

#[derive(Debug)]
enum ClientErrorKind {
    /// A timeout occurred while processing the request
    Timeout,

    /// A user-caused error (eg. invalid HTTP request)
    User,

    /// Socket/IO error
    Io,

    Other(Option<ErrorKind>),
}

impl Display for ClientErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ClientErrorKind::Timeout => write!(f, "timeout"),
            ClientErrorKind::User => write!(f, "user error"),
            ClientErrorKind::Io => write!(f, "io error"),
            ClientErrorKind::Other(Some(kind)) => write!(f, "{:?}", kind),
            ClientErrorKind::Other(None) => write!(f, "other"),
        }
    }
}

impl<E, R> Display for SdkError<E, R>
where
    E: Error,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            SdkError::ConstructionFailure(err) => write!(f, "failed to construct request: {}", err),
            SdkError::DispatchFailure(err) => Display::fmt(&err, f),
            SdkError::ResponseError { err, .. } => Display::fmt(&err, f),
            SdkError::ServiceError { err, .. } => Display::fmt(&err, f),
        }
    }
}

impl<E, R> Error for SdkError<E, R>
where
    E: Error + 'static,
    R: Debug,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            SdkError::ConstructionFailure(err) | SdkError::ResponseError { err, .. } => {
                Some(err.as_ref())
            }
            SdkError::DispatchFailure(err) => Some(err),
            SdkError::ServiceError { err, .. } => Some(err),
        }
    }
}
