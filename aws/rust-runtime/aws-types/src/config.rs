/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::credential::ProvideCredentials;
use crate::region::Region;
use std::sync::Arc;

#[allow(dead_code)]
pub struct Config {
    region: Option<Region>,
    credentials_provider: Option<Arc<dyn ProvideCredentials>>,
}

#[derive(Default)]
pub struct Builder {
    region: Option<Region>,
    credentials_provider: Option<Arc<dyn ProvideCredentials>>,
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
        }
    }

    pub fn build_with_fallback(self, fallback: Builder) -> Config {
        Config {
            region: self.region.or(fallback.region),
            credentials_provider: self.credentials_provider.or(fallback.credentials_provider),
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

    pub fn builder() -> Builder {
        Builder::default()
    }
}
