/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
//! Smithy HTTP Auth Types

use std::cmp::PartialEq;
use std::fmt::Debug;
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Debug, PartialEq)]
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
            SchemeNotAllowed => write!(
                f,
                "scheme only allowed when it is set into the `Authorization header`"
            ),
        }
    }
}

impl From<AuthErrorKind> for AuthError {
    fn from(kind: AuthErrorKind) -> Self {
        Self { kind }
    }
}

/// Authentication configuration to connect to a Smithy Service
#[derive(Clone, Eq, PartialEq)]
pub struct AuthApiKey(Arc<Inner>);

#[derive(Clone, Eq, PartialEq)]
struct Inner {
    api_key: Zeroizing<String>,
}

impl Debug for AuthApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut auth_api_key = f.debug_struct("AuthApiKey");
        auth_api_key.field("api_key", &"** redacted **");
        auth_api_key.finish()
    }
}

impl AuthApiKey {
    /// Constructs a new API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self(Arc::new(Inner {
            api_key: Zeroizing::new(api_key.into()),
        }))
    }

    /// Returns the underlying api key.
    pub fn api_key(&self) -> &str {
        &self.0.api_key
    }
}

impl From<&str> for AuthApiKey {
    fn from(api_key: &str) -> Self {
        Self(Arc::new(Inner {
            api_key: Zeroizing::new(api_key.to_owned()),
        }))
    }
}

impl From<String> for AuthApiKey {
    fn from(api_key: String) -> Self {
        Self(Arc::new(Inner {
            api_key: Zeroizing::new(api_key),
        }))
    }
}

/// An HTTP-specific authentication scheme that sends an arbitrary
/// auth value in a header or query string parameter.
// As described in the Smithy documentation:
// https://github.com/awslabs/smithy/blob/main/smithy-model/src/main/resources/software/amazon/smithy/model/loader/prelude.smithy
#[derive(Clone, Debug, PartialEq)]
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
    fn new<S>(location: String, name: String, scheme: S) -> Result<Self, AuthError>
    where
        S: Into<Option<String>>,
    {
        if location != "header" && location != "query" {
            return Err(AuthError::from(AuthErrorKind::InvalidLocation));
        }
        Ok(Self {
            location,
            name,
            scheme: scheme.into(),
        })
    }

    /// Constructs a new HTTP auth definition in header.
    pub fn new_with_header<N, S>(header_name: N, scheme: S) -> Result<Self, AuthError>
    where
        N: Into<String>,
        S: Into<Option<String>>,
    {
        let name: String = header_name.into();
        let scheme: Option<String> = scheme.into();
        if scheme.is_some() && !name.eq_ignore_ascii_case("authorization") {
            return Err(AuthError::from(AuthErrorKind::SchemeNotAllowed));
        }
        HttpAuthDefinition::new("header".to_owned(), name, scheme)
    }

    /// Constructs a new HTTP auth definition following the RFC 2617 for Basic Auth.
    pub fn new_with_basic_auth() -> Self {
        HttpAuthDefinition::new(
            "header".to_owned(),
            "Authorization".to_owned(),
            "Basic".to_owned(),
        )
        .unwrap()
    }

    /// Constructs a new HTTP auth definition following the RFC 2617 for Digest Auth.
    pub fn new_with_digest_auth() -> Self {
        HttpAuthDefinition::new(
            "header".to_owned(),
            "Authorization".to_owned(),
            "Digest".to_owned(),
        )
        .unwrap()
    }

    /// Constructs a new HTTP auth definition following the RFC 6750 for Bearer Auth.
    pub fn new_with_bearer_auth() -> Self {
        HttpAuthDefinition::new(
            "header".to_owned(),
            "Authorization".to_owned(),
            "Bearer".to_owned(),
        )
        .unwrap()
    }

    /// Constructs a new HTTP auth definition in query string.
    pub fn new_with_query(name: impl Into<String>) -> Self {
        HttpAuthDefinition::new("query".to_owned(), name.into(), None).unwrap()
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
    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use crate::auth::AuthApiKey;
    use crate::auth::AuthErrorKind;
    use crate::auth::HttpAuthDefinition;

    #[test]
    fn api_key_is_equal() {
        let api_key_a: AuthApiKey = "some-api-key".into();
        let api_key_b = AuthApiKey::new("some-api-key");
        assert_eq!(api_key_a, api_key_b);
    }

    #[test]
    fn api_key_is_different() {
        let api_key_a = AuthApiKey::new("some-api-key");
        let api_key_b: AuthApiKey = String::from("another-api-key").into();
        assert_ne!(api_key_a, api_key_b);
    }

    #[test]
    fn auth_definition_fails_with_invalid_location() {
        let result =
            HttpAuthDefinition::new("invalid".to_owned(), "".to_owned(), None).map_err(|e| e.kind);
        let expected = Err(AuthErrorKind::InvalidLocation);
        assert_eq!(expected, result);
    }

    #[test]
    fn auth_definition_for_header_without_scheme() {
        let definition = HttpAuthDefinition::new_with_header("Header", None).unwrap();
        assert_eq!(definition.location, "header");
        assert_eq!(definition.name, "Header");
        assert_eq!(definition.scheme, None);
    }

    #[test]
    fn auth_definition_for_authorization_header_with_scheme() {
        let definition =
            HttpAuthDefinition::new_with_header("authorization", "Scheme".to_owned()).unwrap();
        assert_eq!(definition.location(), "header");
        assert_eq!(definition.name(), "authorization");
        assert_eq!(definition.scheme(), Some("Scheme"));
    }

    #[test]
    fn auth_definition_fails_with_scheme_not_allowed() {
        let result = HttpAuthDefinition::new_with_header("Invalid".to_owned(), "Scheme".to_owned())
            .map_err(|e| e.kind);
        let expected = Err(AuthErrorKind::SchemeNotAllowed);
        assert_eq!(expected, result);
    }

    #[test]
    fn auth_definition_for_basic() {
        let definition = HttpAuthDefinition::new_with_basic_auth();
        assert_eq!(
            definition,
            HttpAuthDefinition {
                location: "header".to_owned(),
                name: "Authorization".to_owned(),
                scheme: Some("Basic".to_owned()),
            }
        );
    }

    #[test]
    fn auth_definition_for_digest() {
        let definition = HttpAuthDefinition::new_with_digest_auth();
        assert_eq!(definition.location(), "header");
        assert_eq!(definition.name(), "Authorization");
        assert_eq!(definition.scheme(), Some("Digest"));
    }

    #[test]
    fn auth_definition_for_bearer_token() {
        let definition = HttpAuthDefinition::new_with_bearer_auth();
        assert_eq!(definition.location(), "header");
        assert_eq!(definition.name(), "Authorization");
        assert_eq!(definition.scheme(), Some("Bearer"));
    }

    #[test]
    fn auth_definition_for_query() {
        let definition = HttpAuthDefinition::new_with_query("query_key");
        assert_eq!(definition.location(), "query");
        assert_eq!(definition.name(), "query_key");
        assert_eq!(definition.scheme(), None);
    }
}
