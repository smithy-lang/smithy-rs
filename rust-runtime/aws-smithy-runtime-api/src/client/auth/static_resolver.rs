/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::box_error::BoxError;
use crate::client::auth::{AuthSchemeId, AuthSchemeOptionResolverParams, ResolveAuthSchemeOptions};
use std::borrow::Cow;

use super::AuthSchemeOptionsFuture;

/// New-type around a `Vec<AuthSchemeId>` that implements `ResolveAuthSchemeOptions`.
#[derive(Debug)]
pub struct StaticAuthSchemeOptionResolver {
    auth_scheme_options: Vec<AuthSchemeId>,
}

impl StaticAuthSchemeOptionResolver {
    /// Creates a new instance of `StaticAuthSchemeOptionResolver`.
    pub fn new(auth_scheme_options: Vec<AuthSchemeId>) -> Self {
        Self {
            auth_scheme_options,
        }
    }
}

impl ResolveAuthSchemeOptions for StaticAuthSchemeOptionResolver {
    fn resolve_auth_scheme_options(
        &self,
        _params: &AuthSchemeOptionResolverParams,
    ) -> Result<Cow<'_, [AuthSchemeId]>, BoxError> {
        Ok(Cow::Borrowed(&self.auth_scheme_options))
    }

    fn resolve_auth_scheme_options_v2<'a>(
        &'a self,
        params: &'a AuthSchemeOptionResolverParams,
        _cfg: &'a aws_smithy_types::config_bag::ConfigBag,
        _runtime_components: &'a crate::client::runtime_components::RuntimeComponents,
    ) -> super::AuthSchemeOptionsFuture<'a> {
        let supported_effective_auth_schemes = params.get::<Vec<AuthSchemeId>>();
        let opts = if let Some(auth_schemes) = supported_effective_auth_schemes {
            auth_schemes
                .iter()
                .cloned()
                .map(|scheme_id| {
                    super::AuthSchemeOption::builder()
                        .scheme_id(scheme_id)
                        .build()
                        .expect("required fields set")
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        AuthSchemeOptionsFuture::ready(Ok(opts))
    }
}

/// Empty params to be used with [`StaticAuthSchemeOptionResolver`].
#[derive(Debug)]
pub struct StaticAuthSchemeOptionResolverParams;

impl StaticAuthSchemeOptionResolverParams {
    /// Creates a new `StaticAuthSchemeOptionResolverParams`.
    pub fn new() -> Self {
        Self
    }
}

impl From<StaticAuthSchemeOptionResolverParams> for AuthSchemeOptionResolverParams {
    fn from(params: StaticAuthSchemeOptionResolverParams) -> Self {
        AuthSchemeOptionResolverParams::new(params)
    }
}
