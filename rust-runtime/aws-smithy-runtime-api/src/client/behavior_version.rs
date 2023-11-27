/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Behavior major-version of the client

/// Behavior major-version of the client
///
/// Over time, new best-practice behaviors are introduced. However, these behaviors might not be backwards
/// compatible. For example, a change which introduces new default timeouts or a new retry-mode for
/// all operations might be the ideal behavior but could break existing applications.
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub struct BehaviorVersion {
    version: Version,
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
enum Version {
    V2023_11_09,
    V2023_11_27,
}

impl BehaviorVersion {
    /// This method will always return the latest major version.
    ///
    /// This is the recommend choice for customers who aren't reliant on extremely specific behavior
    /// characteristics. For example, if you are writing a CLI app, the latest behavior major version
    /// is probably the best setting for you.
    ///
    /// If, however, you're writing a service that is very latency sensitive, or that has written
    /// code to tune Rust SDK behaviors, consider pinning to a specific major version.
    ///
    /// The latest version is currently [`BehaviorVersion::v2023_11_09`]
    pub fn latest() -> Self {
        Self {
            version: Version::V2023_11_27,
        }
    }

    /// This method returns the behavior configuration for November 27th, 2023
    ///
    /// When a new behavior major version is released, this method will be deprecated.
    ///
    /// # Changes
    ///
    /// The default HTTP client is now lazy initialized so that the price of loading
    /// trusted certificates isn't paid when overriding the default client. This means
    /// that the first request after client initialization will be slower for the default
    /// client as it needs to load those certs upon that first request.
    ///
    /// This first request cost can be eliminated by priming the client immediately after
    /// initialization, or by overriding the default HTTP client with one that doesn't lazy
    /// initialize.
    pub const fn v2023_11_27() -> Self {
        Self {
            version: Version::V2023_11_27,
        }
    }

    /// This method returns the behavior configuration for November 9th, 2023
    ///
    /// This behavior version has been superceded by v2023_11_27.
    #[deprecated(
        note = "Superceded by v2023_11_27. See doc comment on v2023_11_27 for details on behavior changes."
    )]
    pub const fn v2023_11_09() -> Self {
        Self {
            version: Version::V2023_11_09,
        }
    }

    /// True if this version is greater than or equal to the given `version`.
    pub fn is_at_least(&self, version: BehaviorVersion) -> bool {
        self >= &version
    }

    /// True if this version is before the given `version`.
    pub fn is_before(&self, version: BehaviorVersion) -> bool {
        self < &version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(deprecated)]
    #[test]
    fn comparison() {
        assert!(BehaviorVersion::v2023_11_09() < BehaviorVersion::v2023_11_27());
        assert!(BehaviorVersion::v2023_11_27() == BehaviorVersion::v2023_11_27());
        assert!(BehaviorVersion::v2023_11_27() > BehaviorVersion::v2023_11_09());
    }
}
