/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime::client::auth;
use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::{
        auth::{
            AuthSchemeId, AuthSchemeOptionResolverParams, AuthSchemeOptionsFuture,
            ResolveAuthSchemeOptions,
        },
        endpoint::{EndpointResolverParams, ResolveEndpoint},
        runtime_components::RuntimeComponents,
    },
};
use aws_smithy_types::{config_bag::ConfigBag, Document};
use std::borrow::Cow;

/// New-type around a `Vec<AuthSchemeId>` that implements `ResolveAuthSchemeOptions`.
#[derive(Debug)]
pub(crate) struct EndpointBasedAuthSchemeOptionResolver {
    auth_scheme_options: Vec<AuthSchemeId>,
}

impl EndpointBasedAuthSchemeOptionResolver {
    /// Creates a new instance of `StaticAuthSchemeOptionResolver`.
    pub(crate) fn new(auth_scheme_options: Vec<AuthSchemeId>) -> Self {
        Self {
            auth_scheme_options,
        }
    }
}

impl ResolveAuthSchemeOptions for EndpointBasedAuthSchemeOptionResolver {
    fn resolve_auth_scheme_options_v2<'a>(
        &'a self,
        params: &'a AuthSchemeOptionResolverParams,
        cfg: &'a ConfigBag,
        runtime_components: &'a RuntimeComponents,
    ) -> AuthSchemeOptionsFuture<'a> {
        AuthSchemeOptionsFuture::new(async move {
            let params = cfg
                .load::<EndpointResolverParams>()
                .expect("endpoint resolver params must be set");

            let endpoint = runtime_components
                .endpoint_resolver()
                .resolve_endpoint(params)
                .await
                .unwrap();

            let auth_schemes = match endpoint.properties().get("authSchemes") {
                Some(Document::Array(schemes)) => schemes,
                _ => return Ok(Cow::Owned(vec![])),
            };
            for auth_scheme in &self.auth_scheme_options {
                for other in auth_schemes {
                    let config_scheme_id = other
                        .as_object()
                        .and_then(|object| object.get("name"))
                        .and_then(Document::as_string);
                    if (config_scheme_id == Some(auth_scheme.as_str())) {
                        return Ok(Cow::Owned(vec![auth_scheme.clone()]));
                    }
                }
            }

            Ok(Cow::Owned(vec![]))
        })
    }
}
