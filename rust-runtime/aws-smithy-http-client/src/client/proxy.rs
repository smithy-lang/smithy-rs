/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Proxy configuration for HTTP clients
//!
//! This module provides types and utilities for configuring HTTP and HTTPS proxies,
//! including support for environment variable detection, authentication, and bypass rules.
//!
//! The implementation delegates to hyper-util's proven proxy functionality while providing
//! a stable, user-friendly API that doesn't expose hyper-util's potentially unstable interfaces.

use http_1x::Uri;
use std::fmt;

/// Proxy configuration for HTTP clients
///
/// Supports HTTP and HTTPS proxy configuration with authentication and bypass rules.
/// Can be configured programmatically or automatically detected from environment variables.
///
/// # Examples
///
/// ```rust
/// use aws_smithy_http_client::client::proxy::ProxyConfig;
///
/// // HTTP proxy for all traffic
/// let config = ProxyConfig::http("http://proxy.example.com:8080")?;
///
/// // HTTPS proxy with authentication
/// let config = ProxyConfig::https("https://proxy.example.com:8080")?
///     .with_basic_auth("username", "password")
///     .no_proxy("localhost,*.internal");
///
/// // Detect from environment variables
/// let config = ProxyConfig::from_env();
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Internal configuration representation
    inner: ProxyConfigInner,
}

/// Internal configuration that will be converted to hyper-util types
#[derive(Debug, Clone)]
enum ProxyConfigInner {
    /// Use hyper-util's environment detection
    FromEnvironment,
    /// Explicit HTTP proxy
    Http {
        uri: Uri,
        auth: Option<ProxyAuth>,
        no_proxy: Option<String>,
    },
    /// Explicit HTTPS proxy
    Https {
        uri: Uri,
        auth: Option<ProxyAuth>,
        no_proxy: Option<String>,
    },
    /// Proxy for all traffic
    All {
        uri: Uri,
        auth: Option<ProxyAuth>,
        no_proxy: Option<String>,
    },
    /// Explicitly disabled
    Disabled,
}

/// Proxy authentication configuration
///
/// Stored for later conversion to hyper-util's authentication format.
#[derive(Debug, Clone)]
struct ProxyAuth {
    /// Username for authentication
    username: String,
    /// Password for authentication
    password: String,
}

/// Errors that can occur during proxy configuration
#[derive(Debug)]
pub enum ProxyError {
    /// Invalid proxy URL
    InvalidUrl(String),
    /// Environment variable parsing error
    EnvVarError(String),
}

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyError::InvalidUrl(url) => write!(f, "invalid proxy URL: {}", url),
            ProxyError::EnvVarError(msg) => write!(f, "environment variable error: {}", msg),
        }
    }
}

impl std::error::Error for ProxyError {}

impl ProxyConfig {
    /// Create a new proxy configuration for HTTP traffic only
    ///
    /// # Arguments
    /// * `proxy_url` - The HTTP proxy URL (e.g., "http://proxy.example.com:8080")
    ///
    /// # Examples
    /// ```rust
    /// use aws_smithy_http_client::client::proxy::ProxyConfig;
    ///
    /// let config = ProxyConfig::http("http://proxy.example.com:8080")?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn http<U>(proxy_url: U) -> Result<Self, ProxyError>
    where
        U: TryInto<Uri>,
        U::Error: fmt::Display,
    {
        let uri = proxy_url
            .try_into()
            .map_err(|e| ProxyError::InvalidUrl(e.to_string()))?;

        Self::validate_proxy_uri(&uri)?;

        Ok(ProxyConfig {
            inner: ProxyConfigInner::Http {
                uri,
                auth: None,
                no_proxy: None,
            },
        })
    }

    /// Create a new proxy configuration for HTTPS traffic only
    ///
    /// # Arguments
    /// * `proxy_url` - The HTTPS proxy URL (e.g., "http://proxy.example.com:8080")
    ///
    /// # Examples
    /// ```rust
    /// use aws_smithy_http_client::client::proxy::ProxyConfig;
    ///
    /// let config = ProxyConfig::https("http://proxy.example.com:8080")?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn https<U>(proxy_url: U) -> Result<Self, ProxyError>
    where
        U: TryInto<Uri>,
        U::Error: fmt::Display,
    {
        let uri = proxy_url
            .try_into()
            .map_err(|e| ProxyError::InvalidUrl(e.to_string()))?;

        Self::validate_proxy_uri(&uri)?;

        Ok(ProxyConfig {
            inner: ProxyConfigInner::Https {
                uri,
                auth: None,
                no_proxy: None,
            },
        })
    }

    /// Create a new proxy configuration for all HTTP and HTTPS traffic
    ///
    /// # Arguments
    /// * `proxy_url` - The proxy URL (e.g., "http://proxy.example.com:8080")
    ///
    /// # Examples
    /// ```rust
    /// use aws_smithy_http_client::client::proxy::ProxyConfig;
    ///
    /// let config = ProxyConfig::all("http://proxy.example.com:8080")?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn all<U>(proxy_url: U) -> Result<Self, ProxyError>
    where
        U: TryInto<Uri>,
        U::Error: fmt::Display,
    {
        let uri = proxy_url
            .try_into()
            .map_err(|e| ProxyError::InvalidUrl(e.to_string()))?;

        Self::validate_proxy_uri(&uri)?;

        Ok(ProxyConfig {
            inner: ProxyConfigInner::All {
                uri,
                auth: None,
                no_proxy: None,
            },
        })
    }

    /// Create a proxy configuration that disables all proxy usage
    ///
    /// This is useful for explicitly disabling proxy support even when
    /// environment variables are set.
    ///
    /// # Examples
    /// ```rust
    /// use aws_smithy_http_client::client::proxy::ProxyConfig;
    ///
    /// let config = ProxyConfig::disabled();
    /// ```
    pub fn disabled() -> Self {
        ProxyConfig {
            inner: ProxyConfigInner::Disabled,
        }
    }

    /// Add basic authentication to this proxy configuration
    ///
    /// # Arguments
    /// * `username` - Username for proxy authentication
    /// * `password` - Password for proxy authentication
    ///
    /// # Examples
    /// ```rust
    /// use aws_smithy_http_client::client::proxy::ProxyConfig;
    ///
    /// let config = ProxyConfig::http("http://proxy.example.com:8080")?
    ///     .with_basic_auth("username", "password");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn with_basic_auth<U, P>(mut self, username: U, password: P) -> Self
    where
        U: Into<String>,
        P: Into<String>,
    {
        let auth = ProxyAuth {
            username: username.into(),
            password: password.into(),
        };

        match &mut self.inner {
            ProxyConfigInner::Http {
                auth: ref mut a, ..
            } => *a = Some(auth),
            ProxyConfigInner::Https {
                auth: ref mut a, ..
            } => *a = Some(auth),
            ProxyConfigInner::All {
                auth: ref mut a, ..
            } => *a = Some(auth),
            ProxyConfigInner::FromEnvironment | ProxyConfigInner::Disabled => {
                // Cannot add auth to environment or disabled configs
                // This is a design decision - auth must be set on explicit proxy configs
            }
        }

        self
    }

    /// Add NO_PROXY rules to this configuration
    ///
    /// NO_PROXY rules specify hosts that should bypass the proxy and connect directly.
    ///
    /// # Arguments
    /// * `rules` - Comma-separated list of bypass rules
    ///
    /// # Examples
    /// ```rust
    /// use aws_smithy_http_client::client::proxy::ProxyConfig;
    ///
    /// let config = ProxyConfig::http("http://proxy.example.com:8080")?
    ///     .no_proxy("localhost,127.0.0.1,*.internal,10.0.0.0/8");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn no_proxy<S: AsRef<str>>(mut self, rules: S) -> Self {
        let rules_str = rules.as_ref().to_string();

        match &mut self.inner {
            ProxyConfigInner::Http {
                no_proxy: ref mut n,
                ..
            } => *n = Some(rules_str),
            ProxyConfigInner::Https {
                no_proxy: ref mut n,
                ..
            } => *n = Some(rules_str),
            ProxyConfigInner::All {
                no_proxy: ref mut n,
                ..
            } => *n = Some(rules_str),
            ProxyConfigInner::FromEnvironment | ProxyConfigInner::Disabled => {
                // Cannot add no_proxy to environment or disabled configs
                // Environment configs will use NO_PROXY env var
            }
        }

        self
    }

    /// Create proxy configuration from environment variables
    ///
    /// Reads standard proxy environment variables using hyper-util's implementation:
    /// - `HTTP_PROXY` / `http_proxy`: HTTP proxy URL
    /// - `HTTPS_PROXY` / `https_proxy`: HTTPS proxy URL
    /// - `ALL_PROXY` / `all_proxy`: Proxy for all protocols (fallback)
    /// - `NO_PROXY` / `no_proxy`: Comma-separated bypass rules
    ///
    /// Returns `None` if no proxy environment variables are set.
    ///
    /// # Examples
    /// ```rust
    /// use aws_smithy_http_client::client::proxy::ProxyConfig;
    ///
    /// // Set environment: HTTP_PROXY=http://proxy:8080
    /// if let Some(config) = ProxyConfig::from_env() {
    ///     // Use proxy configuration
    /// }
    /// ```
    pub fn from_env() -> Option<Self> {
        // Check if any proxy environment variables are set
        // This is a simple check - the actual parsing will be done by hyper-util
        if Self::has_proxy_env_vars() {
            Some(ProxyConfig {
                inner: ProxyConfigInner::FromEnvironment,
            })
        } else {
            None
        }
    }

    /// Check if proxy is disabled (no proxy configuration)
    pub fn is_disabled(&self) -> bool {
        matches!(self.inner, ProxyConfigInner::Disabled)
    }

    /// Check if this configuration uses environment variables
    pub fn is_from_env(&self) -> bool {
        matches!(self.inner, ProxyConfigInner::FromEnvironment)
    }

    // Private helper methods

    fn validate_proxy_uri(uri: &Uri) -> Result<(), ProxyError> {
        // Validate scheme
        match uri.scheme_str() {
            Some("http") | Some("https") => {}
            Some(scheme) => {
                return Err(ProxyError::InvalidUrl(format!(
                    "unsupported proxy scheme: {}",
                    scheme
                )));
            }
            None => {
                return Err(ProxyError::InvalidUrl(
                    "proxy URL must include scheme (http:// or https://)".to_string(),
                ));
            }
        }

        // Validate host
        if uri.host().is_none() {
            return Err(ProxyError::InvalidUrl(
                "proxy URL must include host".to_string(),
            ));
        }

        Ok(())
    }

    fn has_proxy_env_vars() -> bool {
        // Check for any of the standard proxy environment variables
        let proxy_vars = [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ];

        proxy_vars
            .iter()
            .any(|var| std::env::var(var).map(|v| !v.is_empty()).unwrap_or(false))
    }
}

// Note: The actual conversion to hyper-util types will be implemented in Prompt 3
// This keeps the user-facing API clean while deferring the complex logic to hyper-util

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_proxy_config_http() {
        let config = ProxyConfig::http("http://proxy.example.com:8080").unwrap();
        assert!(!config.is_disabled());
        assert!(!config.is_from_env());
    }

    #[test]
    fn test_proxy_config_https() {
        let config = ProxyConfig::https("http://proxy.example.com:8080").unwrap();
        assert!(!config.is_disabled());
        assert!(!config.is_from_env());
    }

    #[test]
    fn test_proxy_config_all() {
        let config = ProxyConfig::all("http://proxy.example.com:8080").unwrap();
        assert!(!config.is_disabled());
        assert!(!config.is_from_env());
    }

    #[test]
    fn test_proxy_config_disabled() {
        let config = ProxyConfig::disabled();
        assert!(config.is_disabled());
        assert!(!config.is_from_env());
    }

    #[test]
    fn test_proxy_config_with_auth() {
        let config = ProxyConfig::http("http://proxy.example.com:8080")
            .unwrap()
            .with_basic_auth("user", "pass");

        // Auth is stored internally - we'll test the conversion in later prompts
        assert!(!config.is_disabled());
    }

    #[test]
    fn test_proxy_config_with_no_proxy() {
        let config = ProxyConfig::http("http://proxy.example.com:8080")
            .unwrap()
            .no_proxy("localhost,*.internal");

        // NO_PROXY rules are stored internally - we'll test the conversion in later prompts
        assert!(!config.is_disabled());
    }

    #[test]
    fn test_proxy_config_invalid_url() {
        let result = ProxyConfig::http("not-a-url");
        assert!(result.is_err());
    }

    #[test]
    fn test_proxy_config_invalid_scheme() {
        let result = ProxyConfig::http("ftp://proxy.example.com:8080");
        assert!(result.is_err());
    }

    #[test]
    fn test_proxy_config_from_env_with_vars() {
        // Save original environment
        let original_http = env::var("HTTP_PROXY");

        // Set test environment
        env::set_var("HTTP_PROXY", "http://test-proxy:8080");

        let config = ProxyConfig::from_env();
        assert!(config.is_some());
        assert!(config.unwrap().is_from_env());

        // Restore original environment
        match original_http {
            Ok(val) => env::set_var("HTTP_PROXY", val),
            Err(_) => env::remove_var("HTTP_PROXY"),
        }
    }

    #[test]
    fn test_proxy_config_from_env_without_vars() {
        // Save original environment
        let original_vars: Vec<_> = [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ]
        .iter()
        .map(|var| (*var, env::var(var)))
        .collect();

        // Clear all proxy environment variables
        for (var, _) in &original_vars {
            env::remove_var(var);
        }

        let config = ProxyConfig::from_env();
        assert!(config.is_none());

        // Restore original environment
        for (var, original_value) in original_vars {
            match original_value {
                Ok(val) => env::set_var(var, val),
                Err(_) => env::remove_var(var),
            }
        }
    }

    #[test]
    fn test_auth_cannot_be_added_to_env_config() {
        // Save original environment
        let original_http = env::var("HTTP_PROXY");
        env::set_var("HTTP_PROXY", "http://test-proxy:8080");

        let config = ProxyConfig::from_env()
            .unwrap()
            .with_basic_auth("user", "pass"); // This should be ignored

        assert!(config.is_from_env());

        // Restore original environment
        match original_http {
            Ok(val) => env::set_var("HTTP_PROXY", val),
            Err(_) => env::remove_var("HTTP_PROXY"),
        }
    }

    #[test]
    fn test_no_proxy_cannot_be_added_to_env_config() {
        // Save original environment
        let original_http = env::var("HTTP_PROXY");
        env::set_var("HTTP_PROXY", "http://test-proxy:8080");

        let config = ProxyConfig::from_env().unwrap().no_proxy("localhost"); // This should be ignored

        assert!(config.is_from_env());

        // Restore original environment
        match original_http {
            Ok(val) => env::set_var("HTTP_PROXY", val),
            Err(_) => env::remove_var("HTTP_PROXY"),
        }
    }
}
