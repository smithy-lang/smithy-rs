/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::sign_in::PROVIDER_NAME;
use aws_credential_types::Credentials;
use aws_sdk_signin::types::CreateOAuth2TokenResponseBody;
use std::fmt;
use std::time::{Duration, SystemTime};
use zeroize::Zeroizing;

/// A login session token created by CLI and loaded from cache
#[derive(Clone)]
pub(super) struct SignInToken {
    pub(super) access_token: Credentials,
    pub(super) token_type: SessionTokenType,
    pub(super) identity_token: Option<String>,
    pub(super) refresh_token: Zeroizing<String>,
    pub(super) client_id: String,
    pub(super) dpop_key: Zeroizing<String>,
}

impl SignInToken {
    pub(super) fn expires_at(&self) -> SystemTime {
        self.access_token
            .expiry()
            .expect("sign-in token access token expected expiry")
    }

    pub(super) fn from_refresh(
        old_token: &SignInToken,
        resp: CreateOAuth2TokenResponseBody,
        now: SystemTime,
    ) -> Self {
        let access_token_output = resp.access_token().expect("accessToken in response");
        let expires_in = resp.expires_in();
        let expiry = now + Duration::from_secs(expires_in as u64);

        let mut credentials = Credentials::builder()
            .access_key_id(access_token_output.access_key_id())
            .secret_access_key(access_token_output.secret_access_key())
            .session_token(access_token_output.session_token())
            .provider_name(PROVIDER_NAME)
            .expiry(expiry);
        credentials.set_account_id(old_token.access_token.account_id().cloned());
        let credentials = credentials.build();

        Self {
            access_token: credentials,
            token_type: old_token.token_type.clone(),
            identity_token: old_token.identity_token.clone(),
            refresh_token: Zeroizing::new(resp.refresh_token().to_string()),
            client_id: old_token.client_id.clone(),
            dpop_key: old_token.dpop_key.clone(),
        }
    }
}

impl fmt::Debug for SignInToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CachedSsoToken")
            .field("access_token", &self.access_token)
            .field("token_type", &self.token_type)
            .field("identity_token", &self.identity_token)
            .field("refresh_token", &self.refresh_token)
            .field("client_id", &self.client_id)
            .field("dpop_key", &"** redacted **")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub(super) enum SessionTokenType {
    AwsSigv4,
}

impl fmt::Display for SessionTokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionTokenType::AwsSigv4 => write!(f, "aws_sigv4"),
        }
    }
}
