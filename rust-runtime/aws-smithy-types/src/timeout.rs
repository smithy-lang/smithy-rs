/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! This module defines types that describe timeouts for the various stages of an HTTP request.

use std::borrow::Cow;
use std::fmt::Display;

// TODO links to external crate members don't work, can this be fixed without directly linking to doc.rs?
/// Configuration for the various kinds of timeouts supported by [aws_smithy_client::Client].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimeoutConfig {
    connect_timeout: Option<f32>,
    tls_negotiation_timeout: Option<f32>,
    read_timeout: Option<f32>,
    api_call_attempt_timeout: Option<f32>,
    api_call_timeout: Option<f32>,
}

// TODO is having a builder for `TimeoutConfig` useful when you can just call methods like `with_read_timeout`?
//     What's our convention for things like this?

impl TimeoutConfig {
    /// Create a new `TimeoutConfig` with no timeouts set
    pub fn new() -> Self {
        Default::default()
    }

    /// Create a new [TimeoutConfigBuilder]
    /// A limit on the amount of time after making an initial connect attempt on a socket to complete the connect-handshake.
    pub fn connect_timeout(&self) -> Option<f32> {
        self.connect_timeout
    }

    /// A limit on the amount of time a TLS handshake takes from when the `CLIENT HELLO` message is
    /// sent to the time the client and server have fully negotiated ciphers and exchanged keys.
    pub fn tls_negotiation_timeout(&self) -> Option<f32> {
        self.tls_negotiation_timeout
    }

    /// A limit on the amount of time an application takes to attempt to read the first byte over an
    /// established, open connection after write request. A.K.A. the "time to first byte" timeout.
    pub fn read_timeout(&self) -> Option<f32> {
        self.read_timeout
    }

    // TODO review this doc and try to improve the wording
    /// A limit on the amount of time it takes for the first byte to be sent over an established,
    /// open connection and when the last byte is received from the service for a single request
    /// attempt. Multiple attempts may be made depending on an app's
    /// [super::retry::RetryConfig].
    pub fn api_call_attempt_timeout(&self) -> Option<f32> {
        self.api_call_attempt_timeout
    }

    // TODO review this doc and try to improve the wording
    /// A limit on the amount of time it takes for the first byte to be sent over an established,
    /// open connection and when the last byte is received from the service for all attempts made
    /// for a single request. Multiple attempts may be made depending on an app's
    /// [super::retry::RetryConfig].
    pub fn api_call_timeout(&self) -> Option<f32> {
        self.api_call_timeout
    }

    /// Consume a [TimeoutConfig] to creat a new one, setting the connect timeout
    pub fn with_connect_timeout(mut self, timeout: f32) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Consume a [TimeoutConfig] to creat a new one, setting the TLS negotiation timeout
    pub fn with_tls_negotiation_timeout(mut self, timeout: f32) -> Self {
        self.tls_negotiation_timeout = Some(timeout);
        self
    }

    /// Consume a [TimeoutConfig] to creat a new one, setting the read timeout
    pub fn with_read_timeout(mut self, timeout: f32) -> Self {
        self.read_timeout = Some(timeout);
        self
    }

    /// Consume a [TimeoutConfig] to creat a new one, setting the api call attempt timeout
    pub fn with_api_call_attempt_timeout(mut self, timeout: f32) -> Self {
        self.api_call_attempt_timeout = Some(timeout);
        self
    }

    /// Consume a [TimeoutConfig] to creat a new one, setting the api call timeout
    pub fn with_api_call_timeout(mut self, timeout: f32) -> Self {
        self.api_call_timeout = Some(timeout);
        self
    }
}

/// A builder for [TimeoutConfig]s
#[derive(Default, Debug, Clone, PartialEq)]
pub struct TimeoutConfigBuilder {
    connect_timeout: Option<f32>,
    tls_negotiation_timeout: Option<f32>,
    read_timeout: Option<f32>,
    api_call_attempt_timeout: Option<f32>,
    api_call_timeout: Option<f32>,
}

impl TimeoutConfigBuilder {
    /// Create a new `TimeoutConfigBuilder`
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets the connect timeout if `Some(f32)` is passed. Unsets the timeout when `None` is passed.
    /// Timeout must be a non-negative number.
    pub fn set_connect_timeout(&mut self, connect_timeout: Option<f32>) -> &mut Self {
        self.connect_timeout = connect_timeout;
        self
    }

    /// Set a limit on the amount of time after making an initial connect attempt on a socket to
    /// complete the connect-handshake. Timeout must be a non-negative number.
    pub fn connect_timeout(mut self, connect_timeout: f32) -> Self {
        self.set_connect_timeout(Some(connect_timeout));
        self
    }

    /// Sets the TLS negotiation timeout if `Some(f32)` is passed. Unsets the timeout when `None` is passed.
    /// Timeout must be a non-negative number.
    pub fn set_tls_negotiation_timeout(
        &mut self,
        tls_negotiation_timeout: Option<f32>,
    ) -> &mut Self {
        self.tls_negotiation_timeout = tls_negotiation_timeout;
        self
    }

    /// Sets a limit on the amount of time a TLS handshake takes from when the `CLIENT HELLO` message
    /// is sent to the time the client and server have fully negotiated ciphers and exchanged keys.
    /// Timeout must be a non-negative number.
    pub fn tls_negotiation_timeout(mut self, tls_negotiation_timeout: f32) -> Self {
        self.set_tls_negotiation_timeout(Some(tls_negotiation_timeout));
        self
    }

    /// Sets the read timeout if `Some(f32)` is passed. Unsets the timeout when `None` is passed.
    /// Timeout must be a non-negative number.
    pub fn set_read_timeout(&mut self, read_timeout: Option<f32>) -> &mut Self {
        self.read_timeout = read_timeout;
        self
    }

    /// Sets a limit on the amount of time an application takes to attempt to read the first byte
    /// over an established, open connection after write request. A.K.A. time to first byte timeout.
    /// Timeout must be a non-negative number.
    pub fn read_timeout(mut self, read_timeout: f32) -> Self {
        self.set_read_timeout(Some(read_timeout));
        self
    }

    /// Sets the HTTP request single-attempt timeout if `Some(f32)` is passed. Unsets the timeout
    /// when `None` is passed. Timeout must be a non-negative number.
    pub fn set_api_call_attempt_timeout(
        &mut self,
        api_call_attempt_timeout: Option<f32>,
    ) -> &mut Self {
        self.api_call_attempt_timeout = api_call_attempt_timeout;
        self
    }

    /// Sets the HTTP request single-attempt timeout. If a call must be retried, this timeout will
    /// apply to each individual attempt. Timeout must be a non-negative number.
    pub fn api_call_attempt_timeout(mut self, api_call_attempt_timeout: f32) -> Self {
        self.set_api_call_attempt_timeout(Some(api_call_attempt_timeout));
        self
    }

    /// Sets the HTTP request multiple-attempt timeout if `Some(f32)` is passed. Unsets the timeout
    /// when `None` is passed. Timeout must be a non-negative number.
    pub fn set_api_call_timeout(&mut self, api_call_timeout: Option<f32>) -> &mut Self {
        self.api_call_timeout = api_call_timeout;
        self
    }

    /// Sets the HTTP request multiple-attempt timeout. This will limit the total amount of time a
    /// request can take, including any retry attempts. Timeout must be a non-negative number.
    pub fn api_call_timeout(mut self, api_call_timeout: f32) -> Self {
        self.set_api_call_timeout(Some(api_call_timeout));
        self
    }

    /// Merge two builders together. Values from `other` will only be used as a fallback for values
    /// from `self` Useful for merging configs from different sources together when you want to
    /// handle "precedence" per value instead of at the config level
    ///
    /// # Example
    ///
    /// ```rust
    /// # use aws_smithy_types::timeout::TimeoutConfigBuilder;
    /// let a = TimeoutConfigBuilder::new().read_timeout(3.0);
    /// let b = TimeoutConfigBuilder::new().read_timeout(10.0).connect_timeout(2.0);
    /// let timeout_config = a.merge_with(b).build();
    /// // A's value take precedence over B's value
    /// assert_eq!(timeout_config.read_timeout(), 3.0);
    /// // A never set a connect timeout so B's value was used
    /// assert_eq!(timeout_config.connect_timeout(), 2.0);
    /// ```
    pub fn merge_with(self, other: Self) -> Self {
        Self {
            connect_timeout: self.connect_timeout.or(other.connect_timeout),
            tls_negotiation_timeout: self
                .tls_negotiation_timeout
                .or(other.tls_negotiation_timeout),
            read_timeout: self.read_timeout.or(other.read_timeout),
            api_call_attempt_timeout: self
                .api_call_attempt_timeout
                .or(other.api_call_attempt_timeout),
            api_call_timeout: self.api_call_timeout.or(other.api_call_timeout),
        }
    }

    /// Consume the builder to create a new [TimeoutConfig]
    pub fn build(self) -> TimeoutConfig {
        TimeoutConfig {
            connect_timeout: self.connect_timeout,
            tls_negotiation_timeout: self.tls_negotiation_timeout,
            read_timeout: self.read_timeout,
            api_call_attempt_timeout: self.api_call_attempt_timeout,
            api_call_timeout: self.api_call_timeout,
        }
    }
}

#[non_exhaustive]
#[derive(Debug)]
/// An error that occurs during construction of a `TimeoutConfig`
pub enum TimeoutConfigError {
    /// When any timeout is set, it must be a non-negative `f32` and cannot be `NaN` or `infinity`.
    InvalidTimeout {
        /// The name of the invalid value
        name: Cow<'static, str>,
        /// The reason that why the timeout was considered invalid
        reason: Cow<'static, str>,
        /// Where the invalid value originated from
        set_by: Cow<'static, str>,
    },
    /// The timeout value couln't be parsed as an f32
    CouldntParseTimeout {
        /// The name of the invalid value
        name: Cow<'static, str>,
        /// Where the invalid value originated from
        set_by: Cow<'static, str>,
    },
}

impl Display for TimeoutConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use TimeoutConfigError::*;
        match self {
            InvalidTimeout {
                name,
                set_by,
                reason,
            } => {
                write!(
                    f,
                    "invalid timeout '{}' set by {} is invalid: {}",
                    name, set_by, reason
                )
            }
            CouldntParseTimeout { name, set_by } => {
                write!(
                    f,
                    "timeout '{}' set by {} could not be parsed as an f32",
                    name, set_by
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TimeoutConfigBuilder;

    #[test]
    fn retry_config_builder_merge_with_favors_self_values_over_other_values() {
        let self_builder = TimeoutConfigBuilder::new()
            .connect_timeout(1.0)
            .read_timeout(1.0)
            .tls_negotiation_timeout(1.0)
            .api_call_timeout(1.0)
            .api_call_attempt_timeout(1.0);
        let other_builder = TimeoutConfigBuilder::new()
            .connect_timeout(2.0)
            .read_timeout(2.0)
            .tls_negotiation_timeout(2.0)
            .api_call_timeout(2.0)
            .api_call_attempt_timeout(2.0);
        let timeout_config = self_builder.merge_with(other_builder).build();

        assert_eq!(timeout_config.connect_timeout(), Some(1.0));
        assert_eq!(timeout_config.read_timeout(), Some(1.0));
        assert_eq!(timeout_config.tls_negotiation_timeout(), Some(1.0));
        assert_eq!(timeout_config.api_call_timeout(), Some(1.0));
        assert_eq!(timeout_config.api_call_attempt_timeout(), Some(1.0));
    }
}
