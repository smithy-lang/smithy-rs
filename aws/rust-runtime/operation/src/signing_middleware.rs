/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
use crate::middleware::OperationMiddleware;
use crate::SdkBody;
use auth::{HttpSigner, SigningConfig};
use std::error::Error;

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

impl OperationMiddleware for SigningMiddleware {
    fn apply(&self, request: &mut crate::Request) -> Result<(), Box<dyn Error>> {
        let signing_config = &request.signing_config;
        let creds = request.credentials_provider.credentials()?;
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
