/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

#![deny(missing_docs)]

//! AWS Shared Config
//!
//! This module contains an shared configuration representation that is agnostic from a specific service.

use crate::region::Region;

/// AWS Shared Configuration
pub struct Config {
    region: Option<Region>,
}

/// Builder for AWS Shared Configuration
#[derive(Default)]
pub struct Builder {
    region: Option<Region>,
}

impl Builder {
    /// Set the region for the builder
    ///
    /// # Example
    /// ```rust
    /// use aws_types::config::Config;
    /// use aws_types::region::Region;
    /// let config = Config::builder().region(Region::new("us-east-1")).build();
    /// ```
    pub fn region(mut self, region: impl Into<Option<Region>>) -> Self {
        self.set_region(region);
        self
    }

    /// Set the region for the builder
    ///
    /// # Example
    /// ```rust
    /// fn region_override() -> Option<Region> {
    ///     // ...
    ///     # None
    /// }
    /// use aws_types::config::Config;
    /// use aws_types::region::Region;
    /// let mut builder = Config::builder();
    /// if let Some(region) = region_override() {
    ///     builder.set_region(region);
    /// }
    /// let config = builder.build();
    /// ```
    pub fn set_region(&mut self, region: impl Into<Option<Region>>) -> &mut Self {
        self.region = region.into();
        self
    }

    /// Build a [`Config`](Config) from this builder
    pub fn build(self) -> Config {
        Config {
            region: self.region,
        }
    }
}

impl Config {
    /// Configured region
    pub fn region(&self) -> Option<&Region> {
        self.region.as_ref()
    }

    /// Config builder
    pub fn builder() -> Builder {
        Builder::default()
    }
}
