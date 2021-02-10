/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::signer::{OperationSigningConfig, RequestConfig, SigV4Signer, SigningError};
use aws_auth::{Credentials, CredentialsError, CredentialsProvider};
use aws_types::{SigningRegion, SigningService};
use smithy_http::middleware::MapRequest;
use smithy_http::operation::Request;
use smithy_http::property_bag::PropertyBag;
use std::time::SystemTime;

/// Middleware stage to sign requests with SigV4
///
/// SigV4RequestSignerStage will load configuration from the request property bag and add
/// a signature.
///
/// Prior to signing, the following fields MUST be present in the property bag:
/// - [`SigningRegion`](SigningRegion): The region used when signing the request, eg. `us-east-1`
/// - [`SigningService`](SigningService): The name of the service to use when signing the request, eg. `dynamodb`
/// - [`CredentialsProvider`](CredentialsProvider): A credentials provider to retrieve credentials
/// - [`OperationSigningConfig`](OperationSigningConfig): Operation specific signing configuration, eg.
///   changes to URL encoding behavior, or headers that must be omitted.
/// If any of these fields are missing, the middleware will return an error.
///
/// The following fields MAY be present in the property bag:
/// - [`SystemTime`](SystemTime): The timestamp to use when signing the request. If this field is not present
///   [`SystemTime::now`](SystemTime::now) will be used.
#[derive(Clone)]
pub struct SigV4SigningStage {
    signer: SigV4Signer,
}

impl SigV4SigningStage {
    pub fn new(signer: SigV4Signer) -> Self {
        Self { signer }
    }
}

use thiserror::Error;
#[derive(Debug, Error)]
pub enum SigningStageError {
    #[error("No credentials provider in the property bag")]
    MissingCredentialsProvider,
    #[error("No signing region in the property bag")]
    MissingSigningRegion,
    #[error("No signing service in the property bag")]
    MissingSigningService,
    #[error("No signing configuration in the property bag")]
    MissingSigningConfig,
    #[error("The request body could not be signed by this configuration")]
    InvalidBodyType,
    #[error("Signing failed")]
    SigningFailure(#[from] SigningError),
    #[error("Failed to load credentials from the credentials provider")]
    CredentialsLoadingError(#[from] CredentialsError),
}

/// Extract a signing config from a [`PropertyBag`](smithy_http::property_bag::PropertyBag)
fn signing_config(
    config: &PropertyBag,
) -> Result<(&OperationSigningConfig, RequestConfig, Credentials), SigningStageError> {
    let operation_config = config
        .get::<OperationSigningConfig>()
        .ok_or(SigningStageError::MissingSigningConfig)?;
    let cred_provider = config
        .get::<CredentialsProvider>()
        .ok_or(SigningStageError::MissingCredentialsProvider)?;
    let creds = cred_provider.credentials()?;
    let region = config
        .get::<SigningRegion>()
        .ok_or(SigningStageError::MissingSigningRegion)?;
    let signing_service = config
        .get::<SigningService>()
        .ok_or(SigningStageError::MissingSigningService)?;
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

impl MapRequest for SigV4SigningStage {
    type Error = SigningStageError;

    fn apply(&self, req: Request) -> Result<Request, Self::Error> {
        req.augment(|req, config| {
            let (operation_config, request_config, creds) = signing_config(config)?;

            // A short dance is required to extract a signable body from an SdkBody, which basically
            // amounts to verifying that it is in fact, a strict body based on a `Bytes` and not a stream
            // Streams must be signed with a different signing mode. Seperate support will be added for
            // this at a later date.
            let (parts, body) = req.into_parts();
            let signable_body = body.bytes().ok_or(SigningStageError::InvalidBodyType)?;
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
            Ok(http::Request::from_parts(signed_parts, body))
        })
    }
}
