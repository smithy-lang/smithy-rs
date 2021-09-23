//! IMDS Region Provider
//!
//! Load region from IMDS from `/latest/meta-data/placement/region`
//! This provider has a 5 second timeout.

use crate::imds;
use crate::imds::client::LazyClient;
use crate::meta::region::{future, ProvideRegion};
use crate::provider_config::ProviderConfig;
use aws_types::os_shim_internal::Env;
use aws_types::region::Region;
use smithy_async::future::timeout::Timeout;
use smithy_async::rt::sleep::AsyncSleep;
use std::sync::Arc;
use std::time::Duration;
use tracing::Instrument;

/// IMDSv2 Region Provider
///
/// This provider is included in the default region chain, so it does not need to be used manually.
///
/// This provider has a 5 second timeout.
#[derive(Debug)]
pub struct ImdsRegionProvider {
    client: LazyClient,
    sleep: Arc<dyn AsyncSleep>,
    env: Env,
}

const REGION_PATH: &str = "/latest/meta-data/placement/region";

impl ImdsRegionProvider {
    fn imds_disabled(&self) -> bool {
        match self.env.get(super::env::EC2_METADATA_DISABLED) {
            Ok(value) => value.eq_ignore_ascii_case("true"),
            _ => false,
        }
    }

    /// Load a region from IMDS
    ///
    /// This provider uses the API `/latest/meta-data/placement/region`
    pub async fn region(&self) -> Option<Region> {
        if self.imds_disabled() {
            return None;
        }
        let client = self.client.client().await.ok()?;
        // TODO: IMDS clients should use a 1 second connect timeout
        let timeout_fut = Timeout::new(
            client.get(REGION_PATH),
            self.sleep.sleep(Duration::from_secs(5)),
        );
        let imds_result = match timeout_fut.await {
            Ok(res) => res,
            Err(_) => {
                tracing::warn!("imds timed out after 5 seconds");
                return None;
            }
        };
        match imds_result {
            Ok(region) => {
                tracing::info!(region = % region, "loaded region from IMDS");
                Some(Region::new(region))
            }
            Err(err) => {
                tracing::warn!(err = % err, "failed to load region from IMDS");
                None
            }
        }
    }
}

impl ProvideRegion for ImdsRegionProvider {
    fn region(&self) -> future::ProvideRegion {
        future::ProvideRegion::new(
            self.region()
                .instrument(tracing::info_span!("imds_load_region")),
        )
    }
}

/// Builder for [`ImdsRegionProvider`]
#[derive(Default)]
pub struct Builder {
    provider_config: Option<ProviderConfig>,
    imds_client_override: Option<imds::Client>,
}

impl Builder {
    /// Set configuration options of the [`Builder`]
    pub fn configure(self, provider_config: &ProviderConfig) -> Self {
        Self {
            provider_config: Some(provider_config.clone()),
            ..self
        }
    }

    /// Override the IMDS client used to load the region
    pub fn imds_client(mut self, imds_client: imds::Client) -> Self {
        self.imds_client_override = Some(imds_client);
        self
    }

    /// Create an [`ImdsRegionProvider`] from this builder
    pub fn build(self) -> ImdsRegionProvider {
        let provider_config = self.provider_config.unwrap_or_default();
        let client = self
            .imds_client_override
            .map(LazyClient::from_ready_client)
            .unwrap_or_else(|| {
                imds::Client::builder()
                    .configure(&provider_config)
                    .build_lazy()
            });
        ImdsRegionProvider {
            client,
            env: provider_config.env(),
            sleep: provider_config
                .sleep()
                .expect("no default sleep implementation provided"),
        }
    }
}
