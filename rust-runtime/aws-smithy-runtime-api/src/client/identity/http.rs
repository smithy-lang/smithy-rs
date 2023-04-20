/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Identity types for HTTP auth

use std::fmt::Debug;
use std::sync::Arc;
use zeroize::Zeroizing;

/// Identity type required to sign requests using Smithy's token-based HTTP auth schemes
///
/// This `Token` type is used with Smithy's `@httpApiKeyAuth` and `@httpBearerAuth`
/// auth traits.
#[derive(Clone, Eq, PartialEq)]
pub struct Token(Arc<TokenInner>);

#[derive(Eq, PartialEq)]
struct TokenInner {
    token: Zeroizing<String>,
}

impl Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("token", &"** redacted **")
            .finish()
    }
}

impl Token {
    /// Constructs a new identity token for HTTP auth.
    pub fn new(token: impl Into<String>) -> Self {
        Self(Arc::new(TokenInner {
            token: Zeroizing::new(token.into()),
        }))
    }

    /// Returns the underlying identity token.
    pub fn token(&self) -> &str {
        &self.0.token
    }
}

impl From<&str> for Token {
    fn from(token: &str) -> Self {
        Self::from(token.to_owned())
    }
}

impl From<String> for Token {
    fn from(api_key: String) -> Self {
        Self(Arc::new(TokenInner {
            token: Zeroizing::new(api_key),
        }))
    }
}

/// Identity type required to sign requests using Smithy's login-based HTTP auth schemes
///
/// This `Login` type is used with Smithy's `@httpBasicAuth` and `@httpDigestAuth`
/// auth traits.
#[derive(Clone, Eq, PartialEq)]
pub struct Login(Arc<LoginInner>);

#[derive(Eq, PartialEq)]
struct LoginInner {
    user: String,
    pass: Zeroizing<String>,
}

impl Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Login")
            .field("user", &self.0.user)
            .field("password", &"** redacted **")
            .finish()
    }
}

impl Login {
    /// Constructs a new identity login for HTTP auth.
    pub fn new(user: impl Into<String>, password: impl Into<String>) -> Self {
        Self(Arc::new(LoginInner {
            user: user.into(),
            pass: Zeroizing::new(password.into()),
        }))
    }

    /// Returns the login user.
    pub fn user(&self) -> &str {
        &self.0.user
    }

    /// Returns the login password.
    pub fn password(&self) -> &str {
        &self.0.pass
    }
}
