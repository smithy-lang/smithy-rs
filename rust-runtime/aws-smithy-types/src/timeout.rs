/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! This module defines types that describe timeouts for the various stages of an HTTP request.

use std::borrow::Cow;
use std::fmt::Display;
use std::time::Duration;

/// Configuration for the various kinds of timeouts supported by aws_smithy_client::Client.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimeoutConfig {
    connect_timeout: Option<Duration>,
    tls_negotiation_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    api_call_attempt_timeout: Option<Duration>,
    api_call_timeout: Option<Duration>,
}

impl TimeoutConfig {
    /// Create a new `TimeoutConfig` with no timeouts set
    pub fn new() -> Self {
        Default::default()
    }

    /// Create a new [TimeoutConfigBuilder]
    /// A limit on the amount of time after making an initial connect attempt on a socket to complete the connect-handshake.
    pub fn connect_timeout(&self) -> Option<Duration> {
        self.connect_timeout
    }

    /// A limit on the amount of time a TLS handshake takes from when the `CLIENT HELLO` message is
    /// sent to the time the client and server have fully negotiated ciphers and exchanged keys.
    pub fn tls_negotiation_timeout(&self) -> Option<Duration> {
        self.tls_negotiation_timeout
    }

    /// A limit on the amount of time an application takes to attempt to read the first byte over an
    /// established, open connection after write request. This is also known as the
    /// "time to first byte" timeout.
    pub fn read_timeout(&self) -> Option<Duration> {
        self.read_timeout
    }

    /// A limit on the amount of time it takes for the first byte to be sent over an established,
    /// open connection and when the last byte is received from the service for a single attempt.
    /// If you want to set a timeout for an entire request including retry attempts,
    /// use [TimeoutConfig::api_call_timeout] instead.
    pub fn api_call_attempt_timeout(&self) -> Option<Duration> {
        self.api_call_attempt_timeout
    }

    /// A limit on the amount of time it takes for request to complete. A single request may be
    /// comprised of several attemps depending on an app's [super::retry::RetryConfig]. If you want
    /// to control timeouts for a single attempt, use [TimeoutConfig::api_call_attempt_timeout].
    pub fn api_call_timeout(&self) -> Option<Duration> {
        self.api_call_timeout
    }

    /// Generate a human-readable list of the timeouts that are currently set. This is used for
    /// logging and error reporting.
    pub fn list_of_set_timeouts(&self) -> Vec<&'static str> {
        let mut vec = Vec::new();
        if self.api_call_timeout.is_some() {
            vec.push("api call")
        }
        if self.api_call_attempt_timeout.is_some() {
            vec.push("api call attempt")
        }
        if self.tls_negotiation_timeout.is_some() {
            vec.push("TLS negotiation")
        }
        if self.connect_timeout.is_some() {
            vec.push("connect")
        }
        if self.read_timeout.is_some() {
            vec.push("read")
        }

        vec
    }

    /// Consume a [`TimeoutConfig`] to createa new one, setting the connect timeout
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Consume a [`TimeoutConfig`] to createa new one, setting the TLS negotiation timeout
    pub fn with_tls_negotiation_timeout(mut self, timeout: Duration) -> Self {
        self.tls_negotiation_timeout = Some(timeout);
        self
    }

    /// Consume a [`TimeoutConfig`] to createa new one, setting the read timeout
    pub fn with_read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = Some(timeout);
        self
    }

    /// Consume a [`TimeoutConfig`] to createa new one, setting the API call attempt timeout
    pub fn with_api_call_attempt_timeout(mut self, timeout: Duration) -> Self {
        self.api_call_attempt_timeout = Some(timeout);
        self
    }

    /// Consume a [`TimeoutConfig`] to createa new one, setting the API call timeout
    pub fn with_api_call_timeout(mut self, timeout: Duration) -> Self {
        self.api_call_timeout = Some(timeout);
        self
    }
}

/// A builder for [`TimeoutConfig`]s
#[derive(Default, Debug, Clone, PartialEq)]
pub struct TimeoutConfigBuilder {
    connect_timeout: Option<Duration>,
    tls_negotiation_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    api_call_attempt_timeout: Option<Duration>,
    api_call_timeout: Option<Duration>,
}

impl TimeoutConfigBuilder {
    /// Create a new `TimeoutConfigBuilder`
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets the connect timeout if `Some(Duration)` is passed. Unsets the timeout when `None` is passed.
    /// Timeout must be a non-negative number.
    pub fn set_connect_timeout(&mut self, connect_timeout: Option<Duration>) -> &mut Self {
        self.connect_timeout = connect_timeout;
        self
    }

    /// Set a limit on the amount of time after making an initial connect attempt on a socket to
    /// complete the connect-handshake. Timeout must be a non-negative number.
    pub fn connect_timeout(mut self, connect_timeout: Duration) -> Self {
        self.set_connect_timeout(Some(connect_timeout));
        self
    }

    /// Sets the TLS negotiation timeout if `Some(Duration)` is passed.
    /// Unsets the timeout when `None` is passed.
    pub fn set_tls_negotiation_timeout(
        &mut self,
        tls_negotiation_timeout: Option<Duration>,
    ) -> &mut Self {
        self.tls_negotiation_timeout = tls_negotiation_timeout;
        self
    }

    /// Sets a limit on the amount of time a TLS handshake takes from when the `CLIENT HELLO` message
    /// is sent to the time the client and server have fully negotiated ciphers and exchanged keys.
    /// Timeout must be a non-negative number.
    pub fn tls_negotiation_timeout(mut self, tls_negotiation_timeout: Duration) -> Self {
        self.set_tls_negotiation_timeout(Some(tls_negotiation_timeout));
        self
    }

    /// Sets the read timeout if `Some(Duration)` is passed. Unsets the timeout when `None` is passed.
    pub fn set_read_timeout(&mut self, read_timeout: Option<Duration>) -> &mut Self {
        self.read_timeout = read_timeout;
        self
    }

    /// Sets a limit on the amount of time an application takes to attempt to read the first byte
    /// over an established, open connection after write request. A.K.A. time to first byte timeout.
    /// Timeout must be a non-negative number.
    pub fn read_timeout(mut self, read_timeout: Duration) -> Self {
        self.set_read_timeout(Some(read_timeout));
        self
    }

    /// Sets the HTTP request single-attempt timeout if `Some(Duration)` is passed.
    /// Unsets the timeout when `None` is passed.
    pub fn set_api_call_attempt_timeout(
        &mut self,
        api_call_attempt_timeout: Option<Duration>,
    ) -> &mut Self {
        self.api_call_attempt_timeout = api_call_attempt_timeout;
        self
    }

    /// Sets the HTTP request single-attempt timeout. If a call must be retried, this timeout will
    /// apply to each individual attempt. Timeout must be a non-negative number.
    pub fn api_call_attempt_timeout(mut self, api_call_attempt_timeout: Duration) -> Self {
        self.set_api_call_attempt_timeout(Some(api_call_attempt_timeout));
        self
    }

    /// Sets the HTTP request multiple-attempt timeout if `Some(Duration)` is passed.
    /// Unsets the timeout when `None` is passed.
    pub fn set_api_call_timeout(&mut self, api_call_timeout: Option<Duration>) -> &mut Self {
        self.api_call_timeout = api_call_timeout;
        self
    }

    /// Sets the HTTP request multiple-attempt timeout. This will limit the total amount of time a
    /// request can take, including any retry attempts. Timeout must be a non-negative number.
    pub fn api_call_timeout(mut self, api_call_timeout: Duration) -> Self {
        self.set_api_call_timeout(Some(api_call_timeout));
        self
    }

    /// Merge two builders together. Values from `other` will only be used as a fallback for values
    /// from `self`. Useful for merging configs from different sources together when you want to
    /// handle "precedence" per value instead of at the config level
    ///
    /// # Example
    ///
    /// ```rust
    /// # use std::time::Duration;
    /// # use aws_smithy_types::timeout::TimeoutConfigBuilder;
    /// let a = TimeoutConfigBuilder::new().read_timeout(Duration::from_secs(2));
    /// let b = TimeoutConfigBuilder::new().read_timeout(Duration::from_secs(10)).connect_timeout(Duration::from_secs(3));
    /// let timeout_config = a.merge_with(b).build();
    /// // A's value take precedence over B's value
    /// assert_eq!(timeout_config.read_timeout(), Some(Duration::from_secs(2)));
    /// // A never set a connect timeout so B's value was used
    /// assert_eq!(timeout_config.connect_timeout(), Some(Duration::from_secs(3)));
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

    /// Consume the builder to create a new [`TimeoutConfig`]
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
    /// A timeout value was set to an invalid value:
    /// - Any number less than 0
    /// - Infinity or negative infinity
    /// - `NaN`
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
    use std::time::Duration;

    #[test]
    fn retry_config_builder_merge_with_favors_self_values_over_other_values() {
        let one_second = Duration::from_secs(1);
        let two_seconds = Duration::from_secs(2);

        let self_builder = TimeoutConfigBuilder::new()
            .connect_timeout(one_second)
            .read_timeout(one_second)
            .tls_negotiation_timeout(one_second)
            .api_call_timeout(one_second)
            .api_call_attempt_timeout(one_second);
        let other_builder = TimeoutConfigBuilder::new()
            .connect_timeout(two_seconds)
            .read_timeout(two_seconds)
            .tls_negotiation_timeout(two_seconds)
            .api_call_timeout(two_seconds)
            .api_call_attempt_timeout(two_seconds);
        let timeout_config = self_builder.merge_with(other_builder).build();

        assert_eq!(timeout_config.connect_timeout(), Some(one_second));
        assert_eq!(timeout_config.read_timeout(), Some(one_second));
        assert_eq!(timeout_config.tls_negotiation_timeout(), Some(one_second));
        assert_eq!(timeout_config.api_call_timeout(), Some(one_second));
        assert_eq!(timeout_config.api_call_attempt_timeout(), Some(one_second));
    }
}
