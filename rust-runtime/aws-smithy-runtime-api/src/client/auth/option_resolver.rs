/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::auth::{AuthOptionResolver, AuthOptionResolverParams, AuthSchemeId};
use crate::client::orchestrator::BoxError;
use std::borrow::Cow;

/// New-type around a `Vec<HttpAuthOption>` that implements `AuthOptionResolver`.
///
/// This is useful for clients that don't require `AuthOptionResolverParams` to resolve auth options.
#[derive(Debug)]
pub struct AuthOptionListResolver {
    auth_options: Vec<AuthSchemeId>,
}

impl AuthOptionListResolver {
    /// Creates a new instance of `AuthOptionListResolver`.
    pub fn new(auth_options: Vec<AuthSchemeId>) -> Self {
        Self { auth_options }
    }
}

impl AuthOptionResolver for AuthOptionListResolver {
    fn resolve_auth_options<'a>(
        &'a self,
        _params: &AuthOptionResolverParams,
    ) -> Result<Cow<'a, [AuthSchemeId]>, BoxError> {
        Ok(Cow::Borrowed(&self.auth_options))
    }
}

/// Empty params to be used with [`AuthOptionListResolver`].
#[derive(Debug)]
pub struct AuthOptionListResolverParams;

impl AuthOptionListResolverParams {
    /// Creates new `AuthOptionListResolverParams`.
    pub fn new() -> Self {
        Self
    }
}
