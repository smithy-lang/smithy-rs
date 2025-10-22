/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_credential_types::Credentials;
use std::fmt;
use std::time::SystemTime;
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
