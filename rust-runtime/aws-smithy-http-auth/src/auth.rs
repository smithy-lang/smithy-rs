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
                "scheme only allowed when it is set into the `Authorization` header"
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

/// Enum for describing where the HTTP Auth can be placed.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum HttpAuthLocation {
    /// In the HTTP header.
    #[default] Header,
    /// In the query string of the URL.
    Query,
}

impl HttpAuthLocation {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Query => "query"
        }
    }
}

impl TryFrom<&str> for HttpAuthLocation {
    type Error = AuthError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "header" => Ok(Self::Header),
            "query" => Ok(Self::Query),
            _ => Err(AuthError::from(AuthErrorKind::InvalidLocation))
        }
    }
}

impl TryFrom<String> for HttpAuthLocation {
    type Error = AuthError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl AsRef<str> for HttpAuthLocation {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for HttpAuthLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.as_str(), f)
    }
}

/// A HTTP-specific authentication scheme that sends an arbitrary
/// auth value in a header or query string parameter.
// As described in the Smithy documentation:
// https://github.com/awslabs/smithy/blob/main/smithy-model/src/main/resources/software/amazon/smithy/model/loader/prelude.smithy
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HttpAuthDefinition {
    /// Defines the location of where the Auth is serialized.
    location: HttpAuthLocation,

    /// Defines the name of the HTTP header or query string parameter
    /// that contains the Auth.
    name: String,

    /// Defines the security scheme to use on the `Authorization` header value.
    /// This can only be set if the "location" property is set to [`HttpAuthLocation::Header`].
    scheme: Option<String>,
}

impl HttpAuthDefinition {
    /// Returns a builder for `HttpAuthDefinition`.
    pub fn builder() -> http_auth_definition::Builder {
        http_auth_definition::Builder::default()
    }

    /// Constructs a new HTTP auth definition in header.
    pub fn header<N, S>(header_name: N, scheme: S) -> Result<Self, AuthError>
    where
        N: Into<String>,
        S: Into<Option<String>>,
    {
        Self::builder()
            .location(HttpAuthLocation::Header)
            .name(header_name)
            .scheme(scheme)
            .build()
    }

    /// Constructs a new HTTP auth definition following the RFC 2617 for Basic Auth.
    pub fn basic_auth() -> Result<Self, AuthError> {
        Self::builder()
            .location(HttpAuthLocation::Header)
            .name("Authorization".to_owned())
            .scheme("Basic".to_owned())
            .build()
    }

    /// Constructs a new HTTP auth definition following the RFC 2617 for Digest Auth.
    pub fn digest_auth() -> Result<Self, AuthError> {
        Self::builder()
            .location(HttpAuthLocation::Header)
            .name("Authorization".to_owned())
            .scheme("Digest".to_owned())
            .build()
    }

    /// Constructs a new HTTP auth definition following the RFC 6750 for Bearer Auth.
    pub fn bearer_auth() -> Result<Self, AuthError> {
        Self::builder()
            .location(HttpAuthLocation::Header)
            .name("Authorization".to_owned())
            .scheme("Bearer".to_owned())
            .build()
    }

    /// Constructs a new HTTP auth definition in query string.
    pub fn query(name: impl Into<String>) -> Result<Self, AuthError> {
        Self::builder()
            .location(HttpAuthLocation::Query)
            .name(name.into())
            .build()
    }

    /// Returns the HTTP auth location.
    pub fn location(&self) -> HttpAuthLocation {
        self.location
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

/// Types associated with [`HttpAuthDefinition`].
pub mod http_auth_definition {
    use super::AuthError;
    use super::AuthErrorKind;
    use super::HttpAuthDefinition;
    use super::HttpAuthLocation;

    /// A builder for [`HttpAuthDefinition`].
    #[derive(Debug, Default)]
    pub struct Builder {
        location: HttpAuthLocation,
        name: String,
        scheme: Option<String>,
    }

    impl Builder {
        /// Sets the HTTP auth location.
        pub fn location(mut self, location: HttpAuthLocation) -> Self {
            self.location = location;
            self
        }

        /// Sets the the HTTP auth name.
        pub fn name(mut self, name: impl Into<String>) -> Self {
            self.name = name.into();
            self
        }

        /// Sets the HTTP auth scheme.
        pub fn scheme(mut self, scheme: impl Into<Option<String>>) -> Self {
            let scheme: Option<String> = scheme.into();
            self.scheme = scheme;
            self
        }

        /// Constructs a [`HttpAuthDefinition`] from the builder.
        pub fn build(self) -> Result<HttpAuthDefinition, AuthError> {
            if self.scheme.is_some() && !self.name.eq_ignore_ascii_case("authorization") {
                return Err(AuthError::from(AuthErrorKind::SchemeNotAllowed));
            }
            Ok(HttpAuthDefinition {
                location: self.location,
                name: self.name,
                scheme: self.scheme,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::auth::AuthApiKey;
    use crate::auth::AuthErrorKind;
    use crate::auth::HttpAuthDefinition;
    use crate::auth::HttpAuthLocation;

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
    fn auth_location_fails_when_invalid() {
        let result = HttpAuthLocation::try_from("invalid").map_err(|e| e.kind);
        let expected = Err(AuthErrorKind::InvalidLocation);
        assert_eq!(expected, result);
    }

    #[test]
    fn auth_definition_for_header_without_scheme() {
        let definition = HttpAuthDefinition::header("Header", None).unwrap();
        assert_eq!(definition.location, HttpAuthLocation::Header);
        assert_eq!(definition.name, "Header");
        assert_eq!(definition.scheme, None);
    }

    #[test]
    fn auth_definition_for_authorization_header_with_scheme() {
        let definition =
            HttpAuthDefinition::header("authorization", "Scheme".to_owned()).unwrap();
        assert_eq!(definition.location(), HttpAuthLocation::Header);
        assert_eq!(definition.name(), "authorization");
        assert_eq!(definition.scheme(), Some("Scheme"));
    }

    #[test]
    fn auth_definition_fails_with_scheme_not_allowed() {
        let result = HttpAuthDefinition::header("Invalid".to_owned(), "Scheme".to_owned())
            .map_err(|e| e.kind);
        let expected = Err(AuthErrorKind::SchemeNotAllowed);
        assert_eq!(expected, result);
    }

    #[test]
    fn auth_definition_for_basic() {
        let definition = HttpAuthDefinition::basic_auth().unwrap();
        assert_eq!(
            definition,
            HttpAuthDefinition {
                location: HttpAuthLocation::Header,
                name: "Authorization".to_owned(),
                scheme: Some("Basic".to_owned()),
            }
        );
    }

    #[test]
    fn auth_definition_for_digest() {
        let definition = HttpAuthDefinition::digest_auth().unwrap();
        assert_eq!(definition.location(), HttpAuthLocation::Header);
        assert_eq!(definition.name(), "Authorization");
        assert_eq!(definition.scheme(), Some("Digest"));
    }

    #[test]
    fn auth_definition_for_bearer_token() {
        let definition = HttpAuthDefinition::bearer_auth().unwrap();
        assert_eq!(definition.location(), HttpAuthLocation::Header);
        assert_eq!(definition.name(), "Authorization");
        assert_eq!(definition.scheme(), Some("Bearer"));
    }

    #[test]
    fn auth_definition_for_query() {
        let definition = HttpAuthDefinition::query("query_key").unwrap();
        assert_eq!(definition.location(), HttpAuthLocation::Query);
        assert_eq!(definition.name(), "query_key");
        assert_eq!(definition.scheme(), None);
    }
}
