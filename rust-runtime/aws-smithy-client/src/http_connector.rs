/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Default connectors based on what TLS features are active. Also contains HTTP-related abstractions
//! that enable passing HTTP connectors around.

use crate::erase::DynConnector;
use aws_smithy_async::rt::sleep::AsyncSleep;
use http::version::Version as HttpVersion;
use std::collections::HashSet;
use std::error::Error;
use std::time::Duration;
use std::{fmt::Debug, sync::Arc};

/// Type alias for a Connector factory function.
pub type MakeConnectorFn = dyn Fn(
        &MakeConnectorSettings,
        Option<Arc<dyn AsyncSleep>>,
    ) -> Result<DynConnector, Box<dyn Error + Send + Sync>>
    + Send
    + Sync;

/// Enum for describing the two "kinds" of HTTP Connectors in smithy-rs.
#[derive(Clone)]
pub enum HttpConnector {
    /// A `DynConnector` to be used for all requests.
    Prebuilt(Option<DynConnector>),
    /// A factory function that will be used to create new `DynConnector`s whenever one is needed.
    ConnectorFn(Arc<MakeConnectorFn>),
}

impl Debug for HttpConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Prebuilt(Some(connector)) => {
                write!(f, "Prebuilt({:?})", connector)
            }
            Self::Prebuilt(None) => {
                write!(f, "Prebuilt(None)")
            }
            Self::ConnectorFn(_) => {
                write!(f, "ConnectorFn(<function pointer>)")
            }
        }
    }
}

impl HttpConnector {
    /// If `HttpConnector` is `Prebuilt`, return a clone of that connector.
    ///
    /// **NOTE:** `Prebuilt` connections won't respect `HttpSetting` and, as such, are only really
    /// useful for testing. **Avoid using them** unless you don't care about configuring retry
    /// behavior, timeouts etc.
    ///
    /// If `HttpConnector` is `ConnectorFn`, generate a new connector from settings and return it.
    pub fn load(
        &self,
        settings: &MakeConnectorSettings,
        sleep: Option<Arc<dyn AsyncSleep>>,
    ) -> Result<DynConnector, Box<dyn Error + Send + Sync>> {
        match self {
            HttpConnector::Prebuilt(Some(conn)) => Ok(conn.clone()),
            HttpConnector::Prebuilt(None) => todo!("What's the use case for this?"),
            HttpConnector::ConnectorFn(func) => func(settings, sleep),
        }
    }
}

/// HttpSettings for HTTP Connectors
#[non_exhaustive]
#[derive(Default, Debug, Clone, Hash, Eq, PartialEq)]
pub struct MakeConnectorSettings {
    /// Set a timeout for reading a stream of bytes
    pub read_timeout: Option<Duration>,
    /// Set a timeout for the connection phase of an HTTP request
    pub connect_timeout: Option<Duration>,
}

impl MakeConnectorSettings {
    /// Set the read timeout
    pub fn with_read_timeout(mut self, read_timeout: Option<Duration>) -> Self {
        self.read_timeout = read_timeout;
        self
    }

    /// Set the connect timeout
    pub fn with_connect_timeout(mut self, connect_timeout: Option<Duration>) -> Self {
        self.connect_timeout = connect_timeout;
        self
    }
}

/// A hashable struct used to key into a hashmap storing different HTTP Clients
#[derive(Debug, Hash, Eq, PartialEq)]
pub struct ConnectorKey {
    /// The HTTP-related settings that were used to create the client that this key points to
    pub make_connector_settings: MakeConnectorSettings,
    /// The desired HTTP version that was uset to create the client that this key points to
    pub http_version: HttpVersion,
}

/// A list of supported or desired HttpVersions. Typically use when requesting an HTTP Client from a
/// client cache.
pub type HttpVersionList = Vec<HttpVersion>;

// // The referenced version of ConnectorKey
// // TODO I'm not sure how to make these lifetimes work yet
// struct HttpRequirements<'a> {
//     http_settings: Cow<'a, HttpSettings>,
//     http_version: HttpVersion,
// }
//
// impl<'a> HttpRequirements<'a> {
//     // Needed for converting a borrowed HttpRequirements into an owned cache key for cache population
//     pub fn into_owned(self) -> HttpRequirements<'static> {
//         Self {
//             http_settings: Cow::Owned(self.http_settings.into_owned()),
//             http_version: self.http_version,
//         }
//     }
// }
