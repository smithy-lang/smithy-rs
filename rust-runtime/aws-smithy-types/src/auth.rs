/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
//! Smithy HTTP Auth Types

use std::fmt::Debug;

#[derive(Debug)]
enum AuthErrorKind {
    InvalidLocation,
    SchemeNotAllowed,
}

/// Error for Smithy authentication
#[derive(Debug)]
pub struct AuthError {
    kind: AuthErrorKind,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use AuthErrorKind::*;
        match &self.kind {
            InvalidLocation => write!(f, "invalid location: expected `header` or `query`"),
            SchemeNotAllowed => write!(f, "scheme only allowed when it is set into the `Authorization header`"),
        }
    }
}

impl From<AuthErrorKind> for AuthError {
    fn from(kind: AuthErrorKind) -> Self {
        Self { kind }
    }
}

/// Authentication configuration to connect to a Smithy Service
#[derive(Clone, Debug)]
pub struct AuthApiKey {
    api_key: String,
}

impl AuthApiKey {
    /// Constructs a new API key.
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    /// Returns the underlying api key.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Sets the value for the api key
    pub fn set_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = api_key.into();
        self
    }
}

/// An HTTP-specific authentication scheme that sends an arbitrary
/// auth value in a header or query string parameter.
// As described in the Smithy documentation:
// https://github.com/awslabs/smithy/blob/main/smithy-model/src/main/resources/software/amazon/smithy/model/loader/prelude.smithy
#[derive(Clone, Debug)]
pub struct HttpAuthDefinition {
    /// Defines the location of where the Auth is serialized. This value
    /// can be set to `"header"` or `"query"`.
    location: String,

    /// Defines the name of the HTTP header or query string parameter
    /// that contains the Auth.
    name: String,

    /// Defines the security scheme to use on the `Authorization` header value.
    /// This can only be set if the "in" property is set to `"header"`.
    scheme: Option<String>,
}

impl HttpAuthDefinition {
    /// Constructs a new HTTP auth definition.
    fn new(location: String, name: String, scheme: Option<String>) -> Result<Self, AuthError> {
        if location != "header" && location != "query" {
            return Err(AuthError::from(AuthErrorKind::InvalidLocation));
        }
        Ok(Self {
            location,
            name,
            scheme,
        })
    }

    /// Constructs a new HTTP auth definition in header.
    pub fn new_with_header(header_name: impl Into<String>, scheme: impl Into<Option<String>>) -> Result<Self, AuthError> {
        let name: String = header_name.into();
        let scheme: Option<String> = scheme.into();
        if scheme.is_some() && name.eq_ignore_ascii_case("authorization") {
            return Err(AuthError::from(AuthErrorKind::SchemeNotAllowed));
        }
        HttpAuthDefinition::new("header".to_owned(), name, scheme)
    }

    /// Constructs a new HTTP auth definition following the RFC 6750 for Bearer Auth.
    pub fn new_with_bearer_auth() -> Self {
        HttpAuthDefinition::new(
            "header".to_owned(),
            "Authorization".to_owned(),
            Some("Bearer".to_owned()),
        ).unwrap()
    }

    /// Constructs a new HTTP auth definition in header.
    pub fn new_with_query(name: impl Into<String>) -> Result<Self, AuthError> {
        HttpAuthDefinition::new("query".to_owned(), name.into(), None)
    }

    /// Returns the HTTP auth location.
    pub fn location(&self) -> &str {
        &self.location
    }

    /// Returns the HTTP auth name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the HTTP auth scheme.
    pub fn scheme(&self) -> &Option<String> {
        &self.scheme
    }
}
