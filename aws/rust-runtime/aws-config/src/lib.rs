#![deny(missing_docs)]

//! aws-config provides implementations of region, credential, and connector resolution.
//!
//! These implementations can be used adhoc, or via [`from_env`](from_env) to assemble a
//! [`Config`](aws_types::config::Config). With a [`Config`](aws_types::config::Config) you can configure
//! an AWS service client.

/// Providers that implement the default AWS provider chain
#[cfg(feature = "default-provider")]
pub mod default_provider;

/// Providers that load configuration from environment variables
pub mod environment;

/// Meta-Providers that combine multiple providers into a single provider
#[cfg(feature = "meta")]
pub mod meta;

/// Create an environment loader for AWS Configuration
///
/// ## Example
/// ```rust
/// # async fn create_config() {
/// use aws_types::region::Region;
/// let config = aws_config::from_env().region("us-east-1").load().await;
/// # }
/// ```
#[cfg(feature = "default-provider")]
pub fn from_env() -> ConfigLoader {
    ConfigLoader::default()
}

/// Load a default configuration from the environment
///
/// Convenience wrapper equivalent to `aws_config::from_env().load().await`
#[cfg(feature = "default-provider")]
pub async fn load_from_env() -> aws_types::config::Config {
    from_env().load().await
}

#[cfg(feature = "default-provider")]
/// Load default sources for all configuration with override support
pub use loader::ConfigLoader;

mod loader {
    use crate::default_provider::region;
    use crate::meta::region::ProvideRegion;
    use aws_types::config::Config;

    /// Load a cross-service [`Config`](aws_types::config::Config) from the environment
    ///
    /// This builder supports overriding individual components of the generated config. Overriding a component
    /// will skip the standard resolution chain from **for that component**. For example,
    /// if you override the region provider, _even if that provider returns None_, the default region provider
    /// chain will not be used.
    #[derive(Default, Debug)]
    pub struct ConfigLoader {
        region: Option<Box<dyn ProvideRegion>>,
    }

    impl ConfigLoader {
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
}
