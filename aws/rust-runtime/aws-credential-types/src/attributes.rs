/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Types representing specific pieces of data contained within credentials or within token

use zeroize::Zeroizing;

/// Type representing a unique identifier representing an AWS account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountId {
    // Ensure it is zeroed in memory when dropped.
    inner: Zeroizing<String>,
}

impl AccountId {
    /// Return the string equivalent of this account id.
    pub fn as_str(&self) -> &str {
        &self.inner
    }
}

impl<T> From<T> for AccountId
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        Self {
            inner: Zeroizing::new(value.into()),
        }
    }
}
