/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Default connectors based on what TLS features are active. Also contains HTTP-related abstractions
//! that enable passing HTTP connectors around.

use crate::erase::DynConnector;
use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_types::timeout::TimeoutConfig;
use std::{fmt::Debug, sync::Arc};

#[cfg(any(feature = "rustls", feature = "native-tls"))]
fn base(settings: &HttpSettings, sleep: Option<Arc<dyn AsyncSleep>>) -> crate::hyper_ext::Builder {
    let mut hyper = crate::hyper_ext::Adapter::builder().timeout(&settings.timeout_config);
    if let Some(sleep) = sleep {
        hyper = hyper.sleep_impl(sleep);
    }
    hyper
}

/// Given `HttpSettings` and an `AsyncSleep`, create a `DynConnector` from defaults depending on what cargo features are activated.
#[cfg(feature = "rustls")]
pub fn default_connector(
    settings: &HttpSettings,
    sleep: Option<Arc<dyn AsyncSleep>>,
) -> Option<DynConnector> {
    let hyper = base(settings, sleep).build(crate::conns::https());
    Some(DynConnector::new(hyper))
}

/// Given `HttpSettings` and an `AsyncSleep`, create a `DynConnector` from defaults depending on what cargo features are activated.
#[cfg(all(not(feature = "rustls"), feature = "native-tls"))]
pub fn default_connector(
    settings: &HttpSettings,
    sleep: Option<Arc<dyn AsyncSleep>>,
) -> Option<DynConnector> {
    let hyper = base(settings, sleep).build(crate::conns::native_tls());
    Some(DynConnector::new(hyper))
}

/// Given `HttpSettings` and an `AsyncSleep`, create a `DynConnector` from defaults depending on what cargo features are activated.
#[cfg(not(any(feature = "rustls", feature = "native-tls")))]
pub fn default_connector(
    _settings: &HttpSettings,
    _sleep: Option<Arc<dyn AsyncSleep>>,
) -> Option<DynConnector> {
    None
}

/// Type alias for a Connector factory function.
pub type MakeConnectorFn =
    dyn Fn(&HttpSettings, Option<Arc<dyn AsyncSleep>>) -> Option<DynConnector> + Send + Sync;

/// Enum for describing the two "kinds" of HTTP Connectors in smithy-rs.
#[derive(Clone)]
pub enum HttpConnector {
    /// A `DynConnector` to be used for all requests.
    Prebuilt(Option<DynConnector>),
    /// A factory function that will be used to create new `DynConnector`s whenever one is needed.
    ConnectorFn(Arc<MakeConnectorFn>),
}

impl Default for HttpConnector {
    fn default() -> Self {
        Self::ConnectorFn(Arc::new(
            |settings: &HttpSettings, sleep: Option<Arc<dyn AsyncSleep>>| {
                default_connector(settings, sleep)
            },
        ))
    }
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
    /// If `HttpConnector` is `ConnectorFn`, generate a new connector from settings and return it.
    pub fn connector(
        &self,
        settings: &HttpSettings,
        sleep: Option<Arc<dyn AsyncSleep>>,
    ) -> Option<DynConnector> {
        match self {
            HttpConnector::Prebuilt(conn) => conn.clone(),
            HttpConnector::ConnectorFn(func) => func(settings, sleep),
        }
    }
}

/// HttpSettings for HTTP Connectors
#[non_exhaustive]
#[derive(Default, Debug)]
pub struct HttpSettings {
    /// Timeout configuration used when sending out requests
    pub timeout_config: TimeoutConfig,
}

impl HttpSettings {
    /// Set the Timeout Config to be used when making HTTP requests
    pub fn with_timeout_config(mut self, timeout_config: TimeoutConfig) -> Self {
        self.timeout_config = timeout_config;
        self
    }
}
