/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::auth::{
    AuthOptionResolverParams, AuthSchemeId, HttpAuthScheme, HttpAuthSchemes, HttpRequestSigner,
};
use aws_smithy_runtime_api::client::identity::{
    AnonymousIdentityResolver, Identity, IdentityResolver, IdentityResolvers,
};
use aws_smithy_runtime_api::client::orchestrator::{BoxError, HttpRequest};
use aws_smithy_runtime_api::config_bag::ConfigBag;

pub const ANONYMOUS_AUTH_SCHEME_ID: AuthSchemeId = AuthSchemeId::new("anonymous");

#[derive(Debug, Default)]
pub struct EmptyAuthOptionResolverParams {}

impl EmptyAuthOptionResolverParams {
    pub fn new() -> Self {
        Self::default()
    }
}

impl From<EmptyAuthOptionResolverParams> for AuthOptionResolverParams {
    fn from(params: EmptyAuthOptionResolverParams) -> Self {
        AuthOptionResolverParams::new(params)
    }
}

pub fn identity_resolvers_for_testing() -> IdentityResolvers {
    IdentityResolvers::builder()
        .identity_resolver(ANONYMOUS_AUTH_SCHEME_ID, AnonymousIdentityResolver::new())
        .build()
}

#[derive(Debug, Default)]
pub struct AnonymousAuthScheme {
    signer: AnonymousSigner,
}

impl AnonymousAuthScheme {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Default)]
pub struct AnonymousSigner {}

impl AnonymousSigner {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HttpRequestSigner for AnonymousSigner {
    fn sign_request(
        &self,
        _request: &mut HttpRequest,
        _identity: &Identity,
        _config_bag: &ConfigBag,
    ) -> Result<(), BoxError> {
        Ok(())
    }
}

impl HttpAuthScheme for AnonymousAuthScheme {
    fn scheme_id(&self) -> AuthSchemeId {
        ANONYMOUS_AUTH_SCHEME_ID
    }

    fn identity_resolver<'a>(
        &self,
        identity_resolvers: &'a IdentityResolvers,
    ) -> Option<&'a dyn IdentityResolver> {
        identity_resolvers.identity_resolver(ANONYMOUS_AUTH_SCHEME_ID)
    }

    fn request_signer(&self) -> &dyn HttpRequestSigner {
        &self.signer
    }
}

pub fn http_auth_schemes_for_testing() -> HttpAuthSchemes {
    HttpAuthSchemes::builder()
        .auth_scheme(ANONYMOUS_AUTH_SCHEME_ID, AnonymousAuthScheme::new())
        .build()
}
