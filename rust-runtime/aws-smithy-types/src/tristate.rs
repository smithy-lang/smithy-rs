/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! This module defines the `TriState` struct: a struct for tracking whether something is
//! set, unset, or explicitly disabled

/// Utility for tracking set vs. unset vs explicitly disabled
///
/// If someone explicitly disables something, we don't need to warn them that it may be missing. This
/// enum impls `From`/`Into` `Option<T>` for ease of use.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TriState<T> {
    /// A state representing something that was explicitly disabled
    Disabled,
    /// A state representing something that has not yet been set
    Unset,
    /// A state representing something that has been deliberately set
    Set(T),
}

impl<T> TriState<T> {
    /// Create a TriState, returning `Unset` when `None` is passed
    pub fn or_unset(t: Option<T>) -> Self {
        match t {
            Some(t) => Self::Set(t),
            None => Self::Unset,
        }
    }
}

impl<T> Default for TriState<T> {
    fn default() -> Self {
        Self::Unset
    }
}

impl<T> From<Option<T>> for TriState<T> {
    fn from(t: Option<T>) -> Self {
        match t {
            Some(t) => TriState::Set(t),
            None => TriState::Disabled,
        }
    }
}

impl<T> From<TriState<T>> for Option<T> {
    fn from(t: TriState<T>) -> Self {
        match t {
            TriState::Disabled | TriState::Unset => None,
            TriState::Set(t) => Some(t),
        }
    }
}
