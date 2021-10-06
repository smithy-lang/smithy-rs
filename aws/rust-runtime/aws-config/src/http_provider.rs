/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Generalized HTTP credential provider. Currently, this cannot be used directly and can only
//! be used via the ECS credential provider.

use crate::json_credentials::{parse_json_credentials, JsonCredentials};
use aws_hyper::{DynConnector, SdkSuccess};
use aws_types::credentials::CredentialsError;
use aws_types::{credentials, Credentials};
use bytes::Bytes;
use http::{Response, Uri};
use smithy_http::body::SdkBody;
use smithy_http::operation::{Operation, Request};
use smithy_http::response::ParseStrictResponse;
use smithy_http::result::SdkError;
use smithy_http::retry::ClassifyResponse;
use smithy_types::retry::{ErrorKind, RetryKind};

use tower::layer::util::Identity;

pub(crate) struct HttpCredentialProvider {
    uri: Uri,
    client: smithy_client::Client<DynConnector, Identity>,
    provider_name: &'static str,
}

impl HttpCredentialProvider {
    pub async fn credentials(&self) -> credentials::Result {
        let credentials = self.client.call(self.operation()).await;
        match credentials {
            Ok(creds) => Ok(creds),
            Err(SdkError::ServiceError { err, .. }) => Err(err),
            Err(other) => Err(CredentialsError::Unhandled(other.into())),
        }
    }

    fn operation(&self) -> Operation<CredentialsResponseParser, HttpCredentialRetryPolicy> {
        let http_req = http::Request::builder()
            .uri(&self.uri)
            .body(SdkBody::empty())
            .expect("valid request");
        Operation::new(
            Request::new(http_req),
            CredentialsResponseParser {
                provider_name: self.provider_name,
            },
        )
        .with_retry_policy(HttpCredentialRetryPolicy)
    }
}

#[derive(Clone, Debug)]
struct CredentialsResponseParser {
    provider_name: &'static str,
}
impl ParseStrictResponse for CredentialsResponseParser {
    type Output = credentials::Result;

    fn parse(&self, response: &Response<Bytes>) -> Self::Output {
        if !response.status().is_success() {
            return Err(CredentialsError::ProviderError(
                "non-200 status from HTTP prpvider".into(),
            ));
        }
        let str_resp = std::str::from_utf8(response.body().as_ref())
            .map_err(|err| CredentialsError::Unhandled(err.into()))?;
        let json_creds = parse_json_credentials(str_resp)
            .map_err(|err| CredentialsError::Unhandled(err.into()))?;
        match json_creds {
            JsonCredentials::RefreshableCredentials {
                access_key_id,
                secret_access_key,
                session_token,
                expiration,
            } => Ok(Credentials::new(
                access_key_id,
                secret_access_key,
                Some(session_token.to_string()),
                Some(expiration),
                self.provider_name,
            )),
            JsonCredentials::Error { code, message } => Err(CredentialsError::ProviderError(
                format!("failed to load credentials[{}]: {}", code, message).into(),
            )),
        }
    }
}

#[derive(Clone, Debug)]
struct HttpCredentialRetryPolicy;

impl ClassifyResponse<SdkSuccess<Credentials>, SdkError<CredentialsError>>
    for HttpCredentialRetryPolicy
{
    fn classify(
        &self,
        response: Result<&SdkSuccess<credentials::Credentials>, &SdkError<CredentialsError>>,
    ) -> RetryKind {
        /* The following errors are retryable:
         *   - Socket errors
         *   - Networking timeouts
         *   - 5xx errors
         *   - Non-parseable 200 responses.
         *  */
        match response {
            Ok(_) => RetryKind::NotRetryable,
            // socket errors, networking timeouts
            Err(SdkError::DispatchFailure(client_err))
                if client_err.is_timeout() || client_err.is_io() =>
            {
                RetryKind::Error(ErrorKind::TransientError)
            }
            // non-parseable 200s
            Err(SdkError::ServiceError {
                err: CredentialsError::Unhandled(_),
                raw,
            }) if raw.http().status().is_success() => RetryKind::Error(ErrorKind::ServerError),
            // 5xx errors
            Err(SdkError::ServiceError { raw, .. } | SdkError::ResponseError { raw, .. })
                if raw.http().status().is_server_error() =>
            {
                RetryKind::Error(ErrorKind::ServerError)
            }
            Err(_) => RetryKind::NotRetryable,
        }
    }
}
