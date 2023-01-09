use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use aws_config::{
    imds::{self, credentials::ImdsCredentialsProvider},
    provider_config::ProviderConfig,
    SdkConfig,
};
use aws_credential_types::{
    cache::CredentialsCache,
    time_source::{TestingTimeSource, TimeSource},
};
use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_client::{
    dvr::{Event, ReplayingConnection},
    erase::DynConnector,
};
use aws_types::region::Region;

pub(crate) struct TestFixture {
    replayer: ReplayingConnection,
    time_source: TestingTimeSource,
}

impl TestFixture {
    #[allow(dead_code)]
    pub(crate) fn new(http_traffic_json_str: &str, start_time: SystemTime) -> Self {
        let events: Vec<Event> = serde_json::from_str(http_traffic_json_str).unwrap();
        Self {
            replayer: ReplayingConnection::new(events),
            time_source: TestingTimeSource::new(start_time),
        }
    }

    #[allow(dead_code)]
    pub(crate) async fn setup(&self) -> SdkConfig {
        let time_source = TimeSource::testing(&self.time_source);

        let provider_config = ProviderConfig::empty()
            .with_http_connector(DynConnector::new(self.replayer.clone()))
            .with_sleep(TokioSleep::new())
            .with_time_source(time_source.clone());

        let client = imds::client::Client::builder()
            .configure(&provider_config)
            .build()
            .await
            .unwrap();

        let provider = ImdsCredentialsProvider::builder()
            .configure(&provider_config)
            .imds_client(client)
            .build();

        SdkConfig::builder()
            .region(Region::from_static("us-east-1"))
            .credentials_cache(
                CredentialsCache::lazy_builder()
                    .time_source(time_source)
                    .into_credentials_cache(),
            )
            .credentials_provider(Arc::new(provider))
            .http_connector(self.replayer.clone())
            .build()
    }

    #[allow(dead_code)]
    pub(crate) fn advance_time(&mut self, delta: Duration) {
        self.time_source.advance(delta);
    }

    #[allow(dead_code)]
    pub(crate) async fn verify(self, headers: &[&str]) {
        self.replayer
            .validate(headers, |_, _| Ok(()))
            .await
            .unwrap();
    }
}
