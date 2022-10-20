/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::escape::EscapeError;
use std::borrow::Cow;
use std::fmt;
use std::str::Utf8Error;

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(in crate::deserialize) enum DeserializeErrorKind {
    Custom(Cow<'static, str>),
    ExpectedLiteral(String),
    InvalidEscape(char),
    InvalidNumber,
    InvalidUtf8,
    UnescapeFailed(EscapeError),
    UnexpectedControlCharacter(u8),
    UnexpectedEos,
    UnexpectedToken(char, &'static str),
}

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct DeserializeError {
    kind: DeserializeErrorKind,
    offset: Option<usize>,
}

impl DeserializeError {
    pub(in crate::deserialize) fn new(kind: DeserializeErrorKind, offset: Option<usize>) -> Self {
        Self { kind, offset }
    }

    /// Returns a custom error without an offset.
    pub fn custom(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(DeserializeErrorKind::Custom(message.into()), None)
    }
}

impl std::error::Error for DeserializeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use DeserializeErrorKind::*;
        match &self.kind {
            UnescapeFailed(source) => Some(source),
            Custom(_)
            | ExpectedLiteral(_)
            | InvalidEscape(_)
            | InvalidNumber
            | InvalidUtf8
            | UnexpectedControlCharacter(_)
            | UnexpectedToken(..)
            | UnexpectedEos => None,
        }
    }
}

impl fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DeserializeErrorKind::*;
        if let Some(offset) = self.offset {
            write!(f, "Error at offset {}: ", offset)?;
        }
        match &self.kind {
            Custom(msg) => write!(f, "failed to parse JSON: {msg}"),
            ExpectedLiteral(literal) => write!(f, "expected literal: {literal}"),
            InvalidEscape(escape) => write!(f, "invalid JSON escape: \\{escape}"),
            InvalidNumber => write!(f, "invalid number"),
            InvalidUtf8 => write!(f, "invalid UTF-8 codepoint in JSON stream"),
            UnescapeFailed(_) => write!(f, "failed to unescape JSON string"),
            UnexpectedControlCharacter(value) => write!(
                f,
                "encountered unescaped control character in string: 0x{value:X}"
            ),
            UnexpectedToken(token, expected) => {
                write!(f, "unexpected token '{token}'. Expected one of {expected}",)
            }
            UnexpectedEos => write!(f, "unexpected end of stream"),
        }
    }
}

impl From<Utf8Error> for DeserializeErrorKind {
    fn from(_: Utf8Error) -> Self {
        DeserializeErrorKind::InvalidUtf8
    }
}

impl From<EscapeError> for DeserializeError {
    fn from(err: EscapeError) -> Self {
        Self {
            kind: DeserializeErrorKind::UnescapeFailed(err),
            offset: None,
        }
    }
}

impl From<aws_smithy_types::TryFromNumberError> for DeserializeError {
    fn from(_: aws_smithy_types::TryFromNumberError) -> Self {
        Self {
            kind: DeserializeErrorKind::InvalidNumber,
            offset: None,
        }
    }
}
