/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::connector::must_have_connector;
use crate::{default_provider, meta};
use aws_types::config::Config;
use aws_types::credential::ProvideCredentials;
use aws_types::region::Region;
use smithy_client::DynConnector;
use std::sync::Arc;

pub mod region {
    use crate::meta::region::{ProvideRegion, ProviderChain};

    use crate::profile;
    pub fn default_provider() -> impl ProvideRegion {
        ProviderChain::first_try(crate::environment::region::Provider::new())
            .or_else(profile::region::Provider::new())
    }
}

pub mod credential;

pub fn env_loader() -> Loader {
    Loader {
        region: None,
        credential_provider: None,
        connector: None,
    }
}

pub struct Loader {
    region: Option<Region>,
    credential_provider: Option<Arc<dyn ProvideCredentials>>,
    connector: Option<DynConnector>,
}

impl Loader {
    pub fn with_region(mut self, region: impl Into<Option<Region>>) -> Self {
        self.region = region.into();
        self
    }

    pub async fn load(self) -> aws_types::config::Config {
        let chained = meta::region::ProviderChain::first_try(self.region)
            .or_else(default_provider::region::default_provider());
        let credential_provider = match self.credential_provider {
            Some(provider) => provider,
            None => Arc::new(default_provider::credential::default_provider().await),
        };
        Config::builder()
            .region(chained.region().await)
            .credentials_provider(credential_provider)
            .connector(self.connector.unwrap_or_else(must_have_connector))
            .build()
    }
}
