/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Default Provider chains for [`region`](default_provider::region) and credentials (TODO)

use aws_types::config::Config;

use crate::meta::region::ProvideRegion;

pub mod region {
    //! Default region provider chain

    use crate::environment::region::EnvironmentVariableRegionProvider;
    use crate::meta::region::ProvideRegion;

    /// Default Region Provider chain
    ///
    /// This provider will load region from environment variables.
    pub fn default_provider() -> impl ProvideRegion {
        EnvironmentVariableRegionProvider::new()
    }
}

/// Load a cross-service [`Config`](aws_types::config::Config) from the environment
///
/// This builder supports overriding individual components of the generated config. Overriding a component
/// will skip the standard resolution chain from **for that component**. For example,
/// if you override the region provider, _even if that provider returns None_, the default region provider
/// chain will not be used.
#[derive(Default, Debug)]
pub struct EnvLoader {
    region: Option<Box<dyn ProvideRegion>>,
}

impl EnvLoader {
    /// Override the region used to construct the [`Config`](aws_types::config::Config).
    ///
    /// ## Example
    /// ```rust
    /// # async fn create_config() {
    /// use aws_types::region::Region;
    /// let config = aws_config::from_env()
    ///     .region(Region::new("us-east-1"))
    ///     .load().await;
    /// # }
    /// ```
    pub fn region(mut self, region: impl ProvideRegion + 'static) -> Self {
        self.region = Some(Box::new(region));
        self
    }

    /// Load the default configuration chain
    ///
    /// If fields have been overridden during builder construction, the override values will be used.
    ///
    /// Otherwise, the default values for each field will be provided.
    ///
    /// NOTE: When an override is provided, the default implementation is **not** used as a fallback.
    /// This means that if you provide a region provider that does not return a region, no region will
    /// be set in the resulting [`Config`](aws_types::config::Config)
    pub async fn load(self) -> aws_types::config::Config {
        let region = if let Some(provider) = self.region {
            provider.region().await
        } else {
            region::default_provider().region().await
        };
        Config::builder().region(region).build()
    }
}
