/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![deny(missing_docs, missing_debug_implementations)]
#![warn(
    missing_debug_implementations,
    missing_docs,
    rust_2018_idioms,
    rustdoc::missing_crate_level_docs,
    unreachable_pub
)]

//! TODO: Add docs

pub mod sensitivity;

use std::fmt::{Debug, Display};

/// A standard interface for taking some component of the HTTP request/response and transforming it into new struct
/// which enjoys [`Debug`] or [`Display`]. This allows for polymorphism over formatting approaches.
pub trait MakeFmt<T> {
    /// Target of the `fmt` transformation.
    type Target;

    /// Transforms a source into a target, altering it's [`Display`] or [`Debug`] implementation.
    fn make(&self, source: T) -> Self::Target;
}

impl<'a, T, U> MakeFmt<T> for &'a U
where
    U: MakeFmt<T>,
{
    type Target = U::Target;

    fn make(&self, source: T) -> Self::Target {
        U::make(self, source)
    }
}

/// Identical to [`MakeFmt`] but with a [`Display`] bound on the associated type.
pub trait MakeDisplay<T> {
    /// Mirrors [`MakeFmt::Target`].
    type Target: Display;

    /// Mirrors [`MakeFmt::make`].
    fn make_display(&self, source: T) -> Self::Target;
}

impl<T, U> MakeDisplay<T> for U
where
    U: MakeFmt<T>,
    U::Target: Display,
{
    type Target = U::Target;

    fn make_display(&self, source: T) -> Self::Target {
        U::make(self, source)
    }
}

/// Identical to [`MakeFmt`] but with a [`Debug`] bound on the associated type.
pub trait MakeDebug<T> {
    /// Mirrors [`MakeFmt::Target`].
    type Target: Debug;

    /// Mirrors [`MakeFmt::make`].
    fn make_debug(&self, source: T) -> Self::Target;
}

impl<T, U> MakeDebug<T> for U
where
    U: MakeFmt<T>,
    U::Target: Debug,
{
    type Target = U::Target;

    fn make_debug(&self, source: T) -> Self::Target {
        U::make(self, source)
    }
}

/// A blanket, identity, [`MakeFmt`] implementation. Applies no changes to the [`Display`]/[`Debug`] implementation.
#[derive(Debug, Clone, Default)]
pub struct MakeIdentity;

impl<T> MakeFmt<T> for MakeIdentity {
    type Target = T;

    fn make(&self, source: T) -> Self::Target {
        source
    }
}
