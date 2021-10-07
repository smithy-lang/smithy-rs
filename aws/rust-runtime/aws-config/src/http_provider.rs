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

use crate::provider_config::ProviderConfig;
use tower::layer::util::Identity;

pub(crate) struct HttpCredentialProvider {
    uri: Uri,
    client: smithy_client::Client<DynConnector, Identity>,
    provider_name: &'static str,
}

pub(crate) struct Builder {
    provider_config: Option<ProviderConfig>,
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
                format!("failed to load credentials [{}]: {}", code, message).into(),
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

#[cfg(test)]
mod test {
    use crate::http_provider::{CredentialsResponseParser, HttpCredentialRetryPolicy};
    use aws_hyper::SdkSuccess;
    use aws_types::credentials::CredentialsError;
    use aws_types::Credentials;
    use bytes::Bytes;
    use smithy_http::body::SdkBody;
    use smithy_http::operation;
    use smithy_http::response::ParseStrictResponse;
    use smithy_http::result::SdkError;
    use smithy_http::retry::ClassifyResponse;
    use smithy_types::retry::{ErrorKind, RetryKind};

    fn sdk_resp(
        resp: http::Response<&'static str>,
    ) -> Result<SdkSuccess<Credentials>, SdkError<CredentialsError>> {
        let resp = resp.map(|data| Bytes::from_static(data.as_bytes()));
        match (CredentialsResponseParser {
            provider_name: "test",
        })
        .parse(&resp)
        {
            Ok(creds) => Ok(SdkSuccess {
                raw: operation::Response::new(resp.map(SdkBody::from)),
                parsed: creds,
            }),
            Err(err) => Err(SdkError::ServiceError {
                err,
                raw: operation::Response::new(resp.map(SdkBody::from)),
            }),
        }
    }

    #[test]
    fn non_parseable_is_retriable() {
        let bad_response = http::Response::builder()
            .status(200)
            .body("notjson")
            .unwrap();

        assert_eq!(
            HttpCredentialRetryPolicy.classify(sdk_resp(bad_response).as_ref()),
            RetryKind::Error(ErrorKind::ServerError)
        );
    }

    #[test]
    fn ok_response_not_retriable() {
        let ok_response = http::Response::builder()
            .status(200)
            .body(
                r#" {
   "AccessKeyId" : "MUA...",
   "SecretAccessKey" : "/7PC5om....",
   "Token" : "AQoDY....=",
   "Expiration" : "2016-02-25T06:03:31Z"
 }"#,
            )
            .unwrap();
        let sdk_result = sdk_resp(ok_response);

        assert_eq!(
            HttpCredentialRetryPolicy.classify(sdk_result.as_ref()),
            RetryKind::NotRetryable
        );

        assert!(sdk_result.is_ok(), "should be ok: {:?}", sdk_result)
    }

    #[test]
    fn explicit_error_not_retriable() {
        let error_response = http::Response::builder()
            .status(400)
            .body(r#"{ "Code": "Error", "Message": "There was a problem, it was your fault" }"#)
            .unwrap();
        let sdk_result = sdk_resp(error_response);
        assert_eq!(
            HttpCredentialRetryPolicy.classify(sdk_result.as_ref()),
            RetryKind::NotRetryable
        );

        assert!(sdk_result.is_ok(), "should be ok: {:?}", sdk_result)
    }
}
