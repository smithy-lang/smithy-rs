/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::signer::{OperationSigningConfig, RequestConfig, SigV4Signer, SigningError};
use aws_auth::{Credentials, CredentialsProvider};
use aws_types::{SigningRegion, SigningService};
use smithy_http::middleware::MapRequest;
use smithy_http::operation::Request;
use smithy_http::property_bag::PropertyBag;
use std::time::SystemTime;

#[derive(Clone)]
pub struct SigV4RequestSignerStage {
    signer: SigV4Signer,
}

impl SigV4RequestSignerStage {
    pub fn new(signer: SigV4Signer) -> Self {
        Self { signer }
    }
}

enum SigningStageError {
    MissingCredentialsProvider,
    MissingSigningRegion,
    MissingSigningService,
    MissingSigningConfig,
    InvalidBodyType,
    SigningFailure(SigningError),
}

fn signing_config(
    config: &PropertyBag,
) -> Result<(&OperationSigningConfig, RequestConfig, Credentials), SigningStageError> {
    let operation_config = config
        .get::<OperationSigningConfig>()
        .ok_or(SigningError::MissingSigningConfig)?;
    let cred_provider = config.get::<CredentialsProvider>();
    let creds = match cred_provider.credentials() {
        Ok(creds) => creds,
        Err(e) => return Err(e.into()),
    };
    let region = config
        .signing_region()
        .ok_or(SigningError::MissingSigningRegion)?
        .to_string();
    let signing_service = config
        .get::<SigningService>()
        .ok_or(SigningError::MissingSigningService)?;
    let request_config = RequestConfig {
        request_ts: config
            .get::<SystemTime>()
            .copied()
            .unwrap_or_else(SystemTime::now),
        region: region.into(),
        service: signing_service,
    };
    Ok((operation_config, request_config, creds))
}

impl MapRequest for SigV4RequestSignerStage {
    type Error = SigningError;

    fn apply(&self, req: Request) -> Result<Request, Self::Error> {
        req.augment(|req, config| {
            let (operation_config, request_config, creds) = signing_config(config)?;

            // A short dance is required to extract a signable body from an SdkBody, which basically
            // amounts to verifying that it is in fact, a strict body based on a `Bytes` and not a stream
            // Streams must be signed with a different signing mode. Seperate support will be added for
            // this at a later date.
            let (parts, body) = request.into_parts();
            let signable_body = match body.bytes() {
                Some(bytes) => bytes,
                None => {
                    return Err("Cannot convert body to signing payload (body is streaming)".into());
                } // in the future, chan variants which will cause an error
            };
            let mut signable_request = http::Request::from_parts(parts, signable_body);

            self.signer
                .sign(
                    &operation_config,
                    &request_config,
                    &creds,
                    &mut signable_request,
                )
                .map_err(|err| SigningStageError::SigningFailure(err))?;
            let (signed_parts, _) = signable_request.into_parts();
            Ok(Request::from_parts(signed_parts, body))
        })
    }
}
