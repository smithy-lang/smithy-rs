/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Default connectors based on what TLS features are active. Also contains HTTP-related abstractions
//! that enable passing HTTP connectors around.

use crate::bounds::SmithyConnector;
use crate::erase::DynConnector;

use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_types::timeout;
use aws_smithy_types::BoxError;
use http::version::Version as HttpVersion;

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::{fmt::Debug, sync::Arc};

/// Type alias for a Connector factory function.
pub type MakeConnectorFn = dyn Fn(&MakeConnectorSettings, Option<Arc<dyn AsyncSleep>>) -> Result<DynConnector, BoxError>
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

    /// Attempt to create an [HttpConnector] from defaults. This will return an
    /// [`Err(HttpConnectorError::NoAvailableDefault)`](HttpConnectorError) if default features
    /// are disabled.
    pub fn try_default() -> Result<Self, HttpConnectorError> {
        if cfg!(feature = "rustls") {
            todo!("How do I actually create this?");
            // Ok(HttpConnector::ConnectorFn(Arc::new(
            //     |settings: &MakeConnectorSettings, sleep_impl: Option<Arc<dyn AsyncSleep>>| {
            //         Ok(DynConnector::new(crate::conns::https()))
            //     },
            // )))
        } else {
            Err(HttpConnectorError::NoAvailableDefault)
        }
    }
}

/// A trait allowing for the easy conversion of various testing connectors into an `HttpConnector`
pub trait MakeTestConnector {
    /// Convert
    fn make_test_connector(self) -> HttpConnector;
}

// TODO can we do some downcasting magic to avoid double-wrapping this if it's already a `DynConnector`?
impl<C> MakeTestConnector for C
where
    C: SmithyConnector,
{
    fn make_test_connector(self) -> HttpConnector {
        HttpConnector::Prebuilt(Some(DynConnector::new(self)))
    }
}

/// Errors related to the creation and use of [HttpConnector]s
#[non_exhaustive]
#[derive(Debug)]
pub enum HttpConnectorError {
    /// Tried to create a new [HttpConnector] from default but couldn't because default features were disabled
    NoAvailableDefault,
    /// Expected an [HttpConnector] to be set but none was set
    NoConnectorDefined,
    /// Expected at least one [http::Version] to be set but none was set
    NoHttpVersionsSpecified,
}

impl Display for HttpConnectorError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        use HttpConnectorError::*;
        match self {
            NoAvailableDefault => {
                // TODO Update this error message with a link to an example demonstrating how to fix it
                write!(
                    f,
                    "When default features are disabled, an HttpConnector must be set manually."
                )
            }
            NoConnectorDefined => {
                write!(
                    f,
                    // TODO in what cases does this error actually appear?
                    "No connector was defined"
                )
            }
            // TODO should this really be an error?
            NoHttpVersionsSpecified => {
                write!(f, "Couldn't get or create a client because no HTTP versions were specified as valid.")
            }
        }
    }
}

impl std::error::Error for HttpConnectorError {}

/// HttpSettings for HTTP Connectors
#[non_exhaustive]
#[derive(Default, Debug, Clone, Hash, PartialEq, Eq)]
pub struct MakeConnectorSettings {
    /// Timeout configuration used when making HTTP connections
    pub http_timeout_config: timeout::Http,
    /// Timeout configuration used when creating TCP connections
    pub tcp_timeout_config: timeout::Tcp,
}

impl MakeConnectorSettings {
    /// Create a new `MakeConnectorSettings` from defaults
    pub fn new() -> Self {
        Default::default()
    }

    /// Set the HTTP timeouts to be used when making HTTP connections
    pub fn with_http_timeout_config(mut self, http_timeout_config: timeout::Http) -> Self {
        self.http_timeout_config = http_timeout_config;
        self
    }

    /// Set the TCP timeouts to be used when creating TCP connections
    pub fn with_tcp_timeout_config(mut self, tcp_timeout_config: timeout::Tcp) -> Self {
        self.tcp_timeout_config = tcp_timeout_config;
        self
    }
}

/// A hashable struct used to key into a hashmap storing different HTTP Clients
#[derive(Debug, Hash, Eq, PartialEq)]
pub struct ConnectorKey {
    /// The HTTP-related settings that were used to create the client that this key points to
    pub make_connector_settings: MakeConnectorSettings,
    /// The desired HTTP version that was used to create the client that this key points to
    pub http_version: HttpVersion,
}

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
