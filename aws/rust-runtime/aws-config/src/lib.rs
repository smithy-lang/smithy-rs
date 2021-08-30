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
pub fn from_env() -> default_provider::EnvLoader {
    default_provider::EnvLoader::default()
}

/// Load a default configuration from the environment
///
/// Convenience wrapper equivalent to `aws_config::from_env().load().await`
#[cfg(feature = "default-provider")]
pub async fn load_from_env() -> aws_types::config::Config {
    from_env().load().await
}
