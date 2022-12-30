/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
//! Smithy HTTP Auth Types

use std::fmt::Debug;
use std::cmp::PartialEq;

/// Authentication configuration to connect to an Smithy Service
#[derive(Clone, Debug)]
pub struct AuthApiKey {
    api_key: String,
}

impl AuthApiKey {
    /// Constructs a new API key.
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
        }
    }

    /// Returns the underlying api key.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Sets the value for the api key
    pub fn set_api_key(
        mut self,
        api_key: impl Into<String>,
    ) -> Self {
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
    pub fn new(location: String, name: String, scheme: Option<String>) -> Self {
        Self {
            location,
            name,
            scheme,
        }
    }

    /// Returns the HTTP auth location.
    pub fn location(&self) -> &str {
        &self.location
    }

    /// Sets the value for HTTP auth location.
    pub fn set_location(
        mut self,
        location: impl Into<String> + std::cmp::PartialEq<String> + Debug,
    ) -> Self {
        if location != "header".to_string() && location != "query".to_string() {
            panic!("Location not allowed: Got: {:?}. Expected: `header` or `query`", location);
        }
        self.location = location.into();
        self
    }

    /// Returns the HTTP auth name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the value for HTTP auth name.
    pub fn set_name(
        mut self,
        name: impl Into<String>,
    ) -> Self {
        self.name = name.into();
        self
    }

    /// Returns the HTTP auth scheme.
    pub fn scheme(&self) -> &Option<String> {
        &self.scheme
    }

    /// Sets the value for HTTP auth scheme.
    pub fn set_scheme(
        mut self,
        scheme: impl Into<String>,
    ) -> Self {
        if self.location.eq("header") && self.name.eq_ignore_ascii_case("authorization") {
            self.scheme = Some(scheme.into());
            println!("Scheme can only be set when it is set into the `Authorization header`.");
        } else {
            self.scheme = None
        }
        self
    }
}
