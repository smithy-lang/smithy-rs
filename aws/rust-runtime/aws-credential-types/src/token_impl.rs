/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_types::date_time::Format;
use aws_smithy_types::DateTime;
use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

/// AWS Access Token
///
/// Provides token which is used to securely authorize requests to AWS
/// services. A token is a string that the OAuth client uses to make requests to
/// the resource server.
///
/// For more details on tokens, see: <https://oauth.net/2/access-tokens>
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct AccessToken(Arc<Inner>);

impl AccessToken {
    /// Create a new access token
    pub fn new(token: impl Into<String>, expires_after: Option<SystemTime>) -> Self {
        Self(Arc::new(Inner {
            token: Zeroizing::new(token.into()),
            expires_after,
        }))
    }

    /// Get the access token
    pub fn token(&self) -> &str {
        &self.0.token
    }

    /// Get the token expiration time
    pub fn expires_after(&self) -> Option<SystemTime> {
        self.0.expires_after
    }
}

struct Inner {
    token: Zeroizing<String>,
    expires_after: Option<SystemTime>,
}

impl fmt::Debug for Inner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut dbg = f.debug_struct("AccessToken");
        dbg.field("token", &"** redacted **");
        if let Some(expires_after) = self.expires_after {
            if let Some(formatted) = expires_after
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|dur| {
                    DateTime::from_secs(dur.as_secs() as _)
                        .fmt(Format::DateTime)
                        .ok()
                })
            {
                dbg.field("expires_after", &formatted);
            } else {
                dbg.field("expires_after", &expires_after);
            }
        } else {
            dbg.field("expires_after", &"never");
        }
        dbg.finish()
    }
}
