/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
use crate::middleware::RequestStage;
use crate::region::RegionExt;
use aws_sig_auth::{
    HttpSigner, HttpSigningConfig, OperationSigningConfig, RequestConfig,
    SigningConfig,
};
use http::Request;
use smithy_http::operation;
use smithy_http::property_bag::PropertyBag;
use std::sync::Arc;
use std::time::SystemTime;
use tower::BoxError;
use auth::ProvideCredentials;

#[derive(Clone)]
pub struct SignRequestStage {
    signer: HttpSigner,
}

impl Default for SignRequestStage {
    fn default() -> Self {
        SignRequestStage::new()
    }
}

impl SignRequestStage {
    pub fn new() -> Self {
        SignRequestStage {
            signer: HttpSigner {},
        }
    }
}

pub trait SigningConfigExt {
    fn signing_config(&self) -> Option<&OperationSigningConfig>;
    fn insert_signing_config(
        &mut self,
        signing_config: OperationSigningConfig,
    ) -> Option<OperationSigningConfig>;
}

impl SigningConfigExt for PropertyBag {
    fn signing_config(&self) -> Option<&OperationSigningConfig> {
        self.get()
    }

    fn insert_signing_config(
        &mut self,
        signing_config: OperationSigningConfig,
    ) -> Option<OperationSigningConfig> {
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

impl CredentialProviderExt for PropertyBag {
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

impl RequestStage for SignRequestStage {
    type Error = BoxError;
    fn apply(&self, request: operation::Request) -> Result<operation::Request, Self::Error> {
        request.augment(|request, config| {
            let operation_config = config.signing_config().ok_or("Missing signing config")?;
            let cred_provider = config
                .credentials_provider()
                .ok_or("Missing credentials provider")?;
            let creds = match cred_provider.credentials() {
                Ok(creds) => creds,
                Err(e) => return Err(e.into()),
            };
            let region = config
                .signing_region()
                .ok_or("Can't sign; No region defined")?
                .to_string();
            let request_config = RequestConfig {
                request_ts: config
                    .get::<SystemTime>()
                    .copied()
                    .unwrap_or_else(SystemTime::now),
                region: region.into(),
            };
            let signing_config = SigningConfig::Http(HttpSigningConfig {
                operation_config: operation_config.clone(),
                request_config,
            });

            let (parts, body) = request.into_parts();
            let signable_body = match body.bytes() {
                Some(bytes) => bytes,
                None => {
                    return Err("Cannot convert body to signing payload (body is streaming)".into())
                } // in the future, chan variants which will cause an error
            };
            let mut signable_request = http::Request::from_parts(parts, signable_body);

            match signing_config {
                SigningConfig::Http(config) => {
                    if let Err(e) = self.signer.sign(&config, &creds, &mut signable_request) {
                        return Err(e as _);
                    }
                }
            };
            let (signed_parts, _) = signable_request.into_parts();
            Ok(Request::from_parts(signed_parts, body))
        })
    }
}
