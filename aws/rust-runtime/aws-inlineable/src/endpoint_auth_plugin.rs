/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;

use aws_smithy_runtime_api::client::{
    auth::{AuthSchemeId, AuthSchemeOption},
    endpoint::ResolveEndpoint,
    runtime_components::RuntimeComponentsBuilder,
    runtime_plugin::{Order, RuntimePlugin},
};

// A runtime plugin that registers `EndpointBasedAuthSchemeOptionResolver` with `RuntimeComponents`.
#[derive(Debug)]
pub(crate) struct EndpointBasedAuthOptionsPlugin {
    runtime_components: RuntimeComponentsBuilder,
}

impl EndpointBasedAuthOptionsPlugin {
    pub(crate) fn new(auth_schemes: Vec<AuthSchemeId>) -> Self {
        let runtime_components = RuntimeComponentsBuilder::new("endpoint_based_auth_options")
            .with_auth_scheme_option_resolver(Some(EndpointBasedAuthSchemeOptionResolver::new(
                auth_schemes,
            )));
        Self { runtime_components }
    }
}

impl RuntimePlugin for EndpointBasedAuthOptionsPlugin {
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

// An `AuthSchemeOptionResolver` that prioritizes auth scheme options from an internally resolved endpoint.
//
// The code generator provides `modeled_auth_scheme_ids`, but during auth scheme option resolution, their priority is
// overridden by the auth scheme options specified in the endpoint. `resolve_auth_scheme_options_v2` places the endpoint's
// options at the beginning of the resulting list. Furthermore, if the endpoint includes an unmodeled auth scheme option, this resolver
// will dynamically generate the option and add it to the resulting list.
#[derive(Debug)]
pub(crate) struct EndpointBasedAuthSchemeOptionResolver {
    modeled_auth_scheme_ids: Vec<AuthSchemeId>,
}

impl EndpointBasedAuthSchemeOptionResolver {
    /// Creates a new instance of `EndpointBasedAuthSchemeOptionResolver`.
    pub(crate) fn new(modeled_auth_scheme_ids: Vec<AuthSchemeId>) -> Self {
        Self {
            modeled_auth_scheme_ids,
        }
    }
}

impl aws_smithy_runtime_api::client::auth::ResolveAuthSchemeOptions
    for EndpointBasedAuthSchemeOptionResolver
{
    fn resolve_auth_scheme_options_v2<'a>(
        &'a self,
        _params: &'a aws_smithy_runtime_api::client::auth::AuthSchemeOptionResolverParams,
        cfg: &'a aws_smithy_types::config_bag::ConfigBag,
        runtime_components: &'a aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
    ) -> aws_smithy_runtime_api::client::auth::AuthSchemeOptionsFuture<'a> {
        aws_smithy_runtime_api::client::auth::AuthSchemeOptionsFuture::new(async move {
            let endpoint_params = cfg
                .load::<aws_smithy_runtime_api::client::endpoint::EndpointResolverParams>()
                .expect("endpoint resolver params must be set");

            tracing::debug!(endpoint_params = ?endpoint_params, "resolving endpoint for auth scheme selection");

            let endpoint = runtime_components
                .endpoint_resolver()
                .resolve_endpoint(endpoint_params)
                .await?;

            let mut endpoint_auth_scheme_ids = Vec::new();

            if let Some(aws_smithy_types::Document::Array(endpoint_auth_schemes)) =
                endpoint.properties().get("authSchemes")
            {
                for endpoint_auth_scheme in endpoint_auth_schemes {
                    let scheme_id_str = endpoint_auth_scheme
                        .as_object()
                        .and_then(|object| object.get("name"))
                        .and_then(aws_smithy_types::Document::as_string);
                    if let Some(scheme_id_str) = scheme_id_str {
                        endpoint_auth_scheme_ids
                            .push(AuthSchemeId::from(Cow::Owned(scheme_id_str.to_owned())));
                    }
                }
            }

            let result =
                merge_auth_scheme_ids(&self.modeled_auth_scheme_ids, endpoint_auth_scheme_ids);

            // TODO(AccountIdBasedRouting): Before merging the final PR to main, experiment pupulating the `properties`
            // field of `AuthSchemeOption` to avoid the orchestrator relying upon `AuthSchemeEndpointConfig`.
            Ok(result
                .into_iter()
                .map(|auth_scheme_id| {
                    AuthSchemeOption::builder()
                        .scheme_id(auth_scheme_id)
                        .build()
                        .expect("required fields set")
                })
                .collect::<Vec<_>>())
        })
    }
}

// Merge a list of `AuthSchemeId`s both in `modeled_auth_scheme_ids` and in `endpoint_auth_scheme_ids`,
// but with those in `endpoint_auth_scheme_ids` placed at the front of the resulting list.
fn merge_auth_scheme_ids(
    modeled_auth_scheme_ids: &[AuthSchemeId],
    mut endpoint_auth_scheme_ids: Vec<AuthSchemeId>,
) -> Vec<AuthSchemeId> {
    let (_, model_only_auth_scheme_ids): (Vec<_>, Vec<_>) = modeled_auth_scheme_ids
        .iter()
        .partition(|auth_scheme_id| endpoint_auth_scheme_ids.contains(auth_scheme_id));

    endpoint_auth_scheme_ids.extend(model_only_auth_scheme_ids.into_iter().cloned());

    endpoint_auth_scheme_ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn into_auth_scheme_ids<const N: usize>(strs: [&'static str; N]) -> Vec<AuthSchemeId> {
        strs.into_iter().map(AuthSchemeId::from).collect::<Vec<_>>()
    }

    #[test]
    fn merge_auth_scheme_ids_basic() {
        let modeled_auth_scheme_ids =
            into_auth_scheme_ids(["schemeA", "schemeX", "schemeB", "schemeY"]);
        let endpoint_auth_scheme_ids = into_auth_scheme_ids(["schemeY", "schemeX"]);
        let expected = into_auth_scheme_ids(["schemeY", "schemeX", "schemeA", "schemeB"]);
        let actual = merge_auth_scheme_ids(&modeled_auth_scheme_ids, endpoint_auth_scheme_ids);
        assert_eq!(expected, actual);
    }

    #[test]
    fn merge_auth_scheme_ids_with_empty_endpoint_auth_scheme_ids() {
        let modeled_auth_scheme_ids =
            into_auth_scheme_ids(["schemeA", "schemeX", "schemeB", "schemeY"]);
        let endpoint_auth_scheme_ids = Vec::new();
        let actual = merge_auth_scheme_ids(&modeled_auth_scheme_ids, endpoint_auth_scheme_ids);
        assert_eq!(modeled_auth_scheme_ids, actual);
    }

    #[test]
    fn merge_auth_scheme_ids_should_also_include_those_only_in_endpoint_auth_scheme_ids() {
        let modeled_auth_scheme_ids =
            into_auth_scheme_ids(["schemeA", "schemeX", "schemeB", "schemeY"]);
        let endpoint_auth_scheme_ids = into_auth_scheme_ids(["schemeY", "schemeZ"]);
        let expected =
            into_auth_scheme_ids(["schemeY", "schemeZ", "schemeA", "schemeX", "schemeB"]);
        let actual = merge_auth_scheme_ids(&modeled_auth_scheme_ids, endpoint_auth_scheme_ids);
        assert_eq!(expected, actual);
    }
}
