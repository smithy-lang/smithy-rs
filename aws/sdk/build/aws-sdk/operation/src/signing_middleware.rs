/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
use crate::extensions::Extensions;
use crate::middleware::OperationMiddleware;
use crate::SdkBody;
use auth::{HttpSigner, ProvideCredentials, SigningConfig};
use std::error::Error;
use std::sync::Arc;

#[derive(Clone)]
pub struct SigningMiddleware {
    signer: HttpSigner,
}

impl Default for SigningMiddleware {
    fn default() -> Self {
        SigningMiddleware::new()
    }
}

impl SigningMiddleware {
    pub fn new() -> Self {
        SigningMiddleware {
            signer: HttpSigner {},
        }
    }
}

pub trait SigningConfigExt {
    fn signing_config(&self) -> Option<&SigningConfig>;
    fn insert_signing_config(&mut self, signing_config: SigningConfig) -> Option<SigningConfig>;
}

impl SigningConfigExt for Extensions {
    fn signing_config(&self) -> Option<&SigningConfig> {
        self.get()
    }

    fn insert_signing_config(&mut self, signing_config: SigningConfig) -> Option<SigningConfig> {
        self.insert(signing_config)
    }
}

pub trait CredentialProviderExt {
    fn credentials_provider(&self) -> Option<&Arc<dyn ProvideCredentials>>;
    fn insert_credentials_provider(
        &mut self,
        provider: Arc<dyn ProvideCredentials>,
    ) -> Option<Arc<dyn ProvideCredentials>>;
}

impl CredentialProviderExt for Extensions {
    fn credentials_provider(&self) -> Option<&Arc<dyn ProvideCredentials>> {
        self.get()
    }

    fn insert_credentials_provider(
        &mut self,
        provider: Arc<dyn ProvideCredentials>,
    ) -> Option<Arc<dyn ProvideCredentials>> {
        self.insert(provider)
    }
}

impl OperationMiddleware for SigningMiddleware {
    fn apply(&self, request: &mut crate::Request) -> Result<(), Box<dyn Error>> {
        let signing_config = request
            .config
            .signing_config()
            .ok_or("Missing signing config")?;
        let cred_provider = request
            .config
            .credentials_provider()
            .ok_or("Missing credentials provider")?;
        let creds = cred_provider.credentials()?;
        let body = match request.base.body() {
            SdkBody::Once(Some(bytes)) => bytes.clone(),
            SdkBody::Once(None) => bytes::Bytes::new(),
            // in the future, chan variants which will cause an error
        };
        match signing_config {
            SigningConfig::Http(config) => {
                if let Err(e) = self.signer.sign(config, &creds, &mut request.base, body) {
                    return Err(e);
                }
            }
        };
        Ok(())
    }
}
