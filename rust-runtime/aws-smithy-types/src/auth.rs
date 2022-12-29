/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
//! Smithy HTTP Auth Types

use std::fmt::Debug;

/// Authentication configuration to connect to an Smithy Service
#[derive(Clone, Debug)]
pub struct AuthApiKey {
    api_key: String,
}

impl AuthApiKey {
    /// Constructs a new API key.
    pub fn new(api_key: String) -> AuthApiKey {
        AuthApiKey {
            api_key,
        }
    }

    /// Returns the underlying api key.
    pub fn api_key(self) -> String {
        self.api_key
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
#[allow(dead_code)]
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
