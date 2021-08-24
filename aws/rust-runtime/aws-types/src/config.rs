/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::credential::ProvideCredentials;
use crate::region::Region;
use smithy_client::DynConnector;
use std::sync::Arc;

#[allow(dead_code)]
pub struct Config {
    region: Option<Region>,
    credentials_provider: Option<Arc<dyn ProvideCredentials>>,
    connector: DynConnector,
}

#[derive(Default)]
pub struct Builder {
    region: Option<Region>,
    credentials_provider: Option<Arc<dyn ProvideCredentials>>,
    connector: Option<DynConnector>,
}

impl Builder {
    pub fn region(mut self, region: impl Into<Option<Region>>) -> Self {
        self.set_region(region);
        self
    }

    pub fn set_region(&mut self, region: impl Into<Option<Region>>) -> &mut Self {
        self.region = region.into();
        self
    }

    pub fn connector(mut self, connector: DynConnector) -> Self {
        self.set_connector(Some(connector));
        self
    }

    pub fn set_connector(&mut self, connector: Option<DynConnector>) -> &mut Self {
        self.connector = connector;
        self
    }

    pub fn credentials_provider(
        mut self,
        credentials_provider: impl ProvideCredentials + 'static,
    ) -> Self {
        self.set_credentials_provider(credentials_provider);
        self
    }

    pub fn set_credentials_provider(
        &mut self,
        credentials_provider: impl ProvideCredentials + 'static,
    ) -> &mut Self {
        self.credentials_provider = Some(Arc::new(credentials_provider));
        self
    }

    pub fn build(self) -> Config {
        Config {
            region: self.region,
            credentials_provider: self.credentials_provider,
            connector: self.connector.expect("A connector must be defined"),
        }
    }

    pub fn build_with_fallback(self, fallback: Builder) -> Config {
        Config {
            region: self.region.or(fallback.region),
            credentials_provider: self.credentials_provider.or(fallback.credentials_provider),
            connector: self
                .connector
                .or(fallback.connector)
                .expect("a connector must be provided"),
        }
    }
}

impl Config {
    pub fn region(&self) -> Option<&Region> {
        self.region.as_ref()
    }

    pub fn credentials_provider(&self) -> Option<Arc<dyn ProvideCredentials>> {
        self.credentials_provider.clone()
    }

    pub fn connector(&self) -> &DynConnector {
        &self.connector
    }

    pub fn builder() -> Builder {
        Builder::default()
    }
}
