/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::tristate::TriState;
use std::time::Duration;

/// HTTP timeouts used by `DynConnector`s
#[non_exhaustive]
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Http {
    /// A limit on the amount of time after making an initial connect attempt on a socket to complete the connect-handshake.
    connect: TriState<Duration>,
    write: TriState<Duration>,
    /// A limit on the amount of time an application takes to attempt to read the first byte over an
    /// established, open connection after write request. This is also known as the
    /// "time to first byte" timeout.
    read: TriState<Duration>,
    /// A limit on the amount of time a TLS handshake takes from when the `CLIENT HELLO` message is
    /// sent to the time the client and server have fully negotiated ciphers and exchanged keys.
    tls_negotiation: TriState<Duration>,
}

impl Http {
    /// Create a new HTTP timeout config with no timeouts set
    pub fn new() -> Self {
        Default::default()
    }

    /// Return this config's read timeout
    pub fn read_timeout(&self) -> TriState<Duration> {
        self.read.clone()
    }

    /// Mutate this `timeout::Http` config, setting the HTTP read timeout
    pub fn with_read_timeout(mut self, timeout: TriState<Duration>) -> Self {
        self.read = timeout;
        self
    }

    /// Return this config's read timeout
    pub fn connect_timeout(&self) -> TriState<Duration> {
        self.connect.clone()
    }

    /// Mutate this `timeout::Http` config, setting the HTTP connect timeout
    pub fn with_connect_timeout(mut self, timeout: TriState<Duration>) -> Self {
        self.connect = timeout;
        self
    }

    /// Return true if any timeouts are intentionally set or disabled
    pub fn has_timeouts(&self) -> bool {
        !self.is_unset()
    }

    /// Return true if all timeouts are unset
    fn is_unset(&self) -> bool {
        self.connect.is_unset()
            && self.write.is_unset()
            && self.read.is_unset()
            && self.tls_negotiation.is_unset()
    }

    /// Merges two HTTP timeout configs together.
    pub fn take_unset_from(self, other: Self) -> Self {
        Self {
            connect: self.connect.or(other.connect),
            write: self.write.or(other.write),
            read: self.read.or(other.read),
            tls_negotiation: self.tls_negotiation.or(other.tls_negotiation),
        }
    }
}

impl From<super::Config> for Http {
    fn from(config: super::Config) -> Self {
        config.http
    }
}
