/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_auth::refresh::{
    AsyncCredentialLoader, AsyncCredentialResult, RefreshingCredentialProvider,
};
use aws_auth::CredentialsError;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use sts::Credentials;

#[derive(Clone)]
struct StsCredentialLoader {
    client: sts::Client,
}

impl AsyncCredentialLoader for StsCredentialLoader {
    fn load_credentials(&self) -> Pin<Box<dyn Future<Output = AsyncCredentialResult> + Send>> {
        let this = self.clone();
        Box::pin(async move {
            let session_token = this
                .client
                .get_session_token()
                .duration_seconds(900)
                .send()
                .await
                .map_err(|err| CredentialsError::Unhandled(Box::new(err)))?;
            let sts_credentials = session_token
                .credentials
                .expect("should include credentials");
            Ok(Credentials::new(
                sts_credentials.access_key_id.unwrap(),
                sts_credentials.secret_access_key.unwrap(),
                sts_credentials.session_token,
                sts_credentials
                    .expiration
                    .map(|expiry| expiry.to_system_time().expect("sts sent a time < 0")),
                "Sts",
            ))
        })
    }
}

/// Implements a basic version of ProvideCredentials with AWS STS
/// and lists the tables in the region based on those credentials.
#[tokio::main]
async fn main() -> Result<(), dynamodb::Error> {
    tracing_subscriber::fmt::init();
    let client = sts::Client::from_env();

    let mut refresher = RefreshingCredentialProvider::new(Arc::new(StsCredentialLoader { client }));
    refresher
        .spawn_refresh_loop()
        .await
        .expect("spawn credential refresher");

    let dynamodb_conf = dynamodb::Config::builder()
        .credentials_provider(refresher)
        .build();

    let client = dynamodb::Client::from_conf(dynamodb_conf);
    loop {
        println!("tables: {:?}", client.list_tables().send().await?);
        println!("waiting 15 minutes for session credentials to expire... hit CTRL+C to stop");
        tokio::time::sleep(Duration::from_secs(915)).await;
    }
}
