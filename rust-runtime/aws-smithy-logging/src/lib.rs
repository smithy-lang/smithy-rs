/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![deny(missing_docs, missing_debug_implementations)]

//! Provides [`InstrumentOperation`] and a variety of helpers structures for dealing with sensitive
//! data.

mod headers;
mod sensitive;
mod service;
mod uri;

use std::fmt::{Debug, Display, Formatter};

pub use headers::*;
pub use sensitive::*;
pub use service::*;
pub use uri::*;

enum OrFmt<Left, Right> {
    Left(Left),
    Right(Right),
}

impl<Left, Right> Debug for OrFmt<Left, Right>
where
    Left: Debug,
    Right: Debug,
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Left(left) => left.fmt(f),
            Self::Right(right) => right.fmt(f),
        }
    }
}

impl<Left, Right> Display for OrFmt<Left, Right>
where
    Left: Display,
    Right: Display,
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Left(left) => left.fmt(f),
            Self::Right(right) => right.fmt(f),
        }
    }
}

/// The string placeholder for redacted data.
pub const REDACTED: &str = "{redacted}";
