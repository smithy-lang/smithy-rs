/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;

use aws_smithy_runtime_api::client::auth::static_resolver::StaticAuthSchemeOptionResolver;
use aws_smithy_runtime_api::client::auth::{
    AuthSchemeId, AuthSchemeOptionResolverParams, AuthSchemeOptionsFuture, ResolveAuthSchemeOptions,
};
use aws_smithy_runtime_api::client::endpoint::{EndpointResolverParams, ResolveEndpoint};
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponents, RuntimeComponentsBuilder,
};
use aws_smithy_runtime_api::client::runtime_plugin::{Order, RuntimePlugin};
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::Document;

#[derive(Debug)]
pub(crate) struct DefaultAuthOptionsPlugin {
    runtime_components: RuntimeComponentsBuilder,
}

impl DefaultAuthOptionsPlugin {
    #[allow(dead_code)]
    pub(crate) fn new(auth_schemes: Vec<AuthSchemeId>) -> Self {
        let runtime_components = RuntimeComponentsBuilder::new("default_auth_options")
            .with_auth_scheme_option_resolver(Some(StaticAuthSchemeOptionResolver::new(
                auth_schemes,
            )));
        Self { runtime_components }
    }

    #[allow(dead_code)]
    pub(crate) fn new_endpoint_based(auth_schemes: Vec<AuthSchemeId>) -> Self {
        let runtime_components = RuntimeComponentsBuilder::new("default_auth_options")
            .with_auth_scheme_option_resolver(Some(EndpointBasedAuthSchemeOptionResolver::new(
                auth_schemes,
            )));
        Self { runtime_components }
    }
}

impl RuntimePlugin for DefaultAuthOptionsPlugin {
    fn order(&self) -> Order {
        Order::Defaults
    }

    fn runtime_components(
        &self,
        _current_components: &RuntimeComponentsBuilder,
    ) -> Cow<'_, RuntimeComponentsBuilder> {
        Cow::Borrowed(&self.runtime_components)
    }
}

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
        _params: &'a AuthSchemeOptionResolverParams,
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
                    if config_scheme_id == Some(auth_scheme.as_str()) {
                        return Ok(Cow::Owned(vec![auth_scheme.clone()]));
                    }
                }
            }

            Ok(Cow::Owned(vec![]))
        })
    }
}
