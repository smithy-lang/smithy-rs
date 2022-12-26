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
