/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
use crate::extensions::Extensions;
use crate::middleware::OperationMiddleware;
use crate::SdkBody;
use auth::{HttpSigner, ProvideCredentials, SigningConfig, OperationSigningConfig, RequestConfig, HttpSigningConfig};
use std::error::Error;
use std::sync::Arc;
use std::time::SystemTime;
use crate::region::RegionExt;

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
    fn signing_config(&self) -> Option<&OperationSigningConfig>;
    fn insert_signing_config(&mut self, signing_config: OperationSigningConfig) -> Option<OperationSigningConfig>;
}

impl SigningConfigExt for Extensions {
    fn signing_config(&self) -> Option<&OperationSigningConfig> {
        self.get()
    }

    fn insert_signing_config(&mut self, signing_config: OperationSigningConfig) -> Option<OperationSigningConfig> {
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
    fn apply(&self, request: crate::Request) -> Result<crate::Request, Box<dyn Error>> {
        request.augment(|mut request, config| {
            let operation_config = config.signing_config().ok_or("Missing signing config")?;
            let cred_provider = config
                .credentials_provider()
                .ok_or("Missing credentials provider")?;
            let creds = match cred_provider.credentials() {
                Ok(creds) => creds,
                Err(e) => return Err(e as _),
            };
            let body = match request.body() {
                SdkBody::Once(Some(bytes)) => bytes.clone(),
                SdkBody::Once(None) => bytes::Bytes::new(),
                // in the future, chan variants which will cause an error
            };
            let region = config.signing_region().ok_or("Can't sign; No region defined")?.to_string();
            let request_config = RequestConfig {
                request_ts: SystemTime::now(), // TODO: replace with Extensions.now();
                region: region.into()
            };
            let signing_config = SigningConfig::Http(HttpSigningConfig {
                operation_config: operation_config.clone(),
                request_config
            });

            match signing_config {
                SigningConfig::Http(config) => {
                    if let Err(e) = self.signer.sign(&config, &creds, &mut request, body) {
                        return Err(e as _);
                    }
                }
            };
            Ok(request)
        })
    }
}
