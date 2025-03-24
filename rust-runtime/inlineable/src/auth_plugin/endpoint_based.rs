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

/// An `AuthSchemeOptionResolver` that prioritizes auth schemes from an internally resolved endpoint.
#[derive(Debug)]
pub(crate) struct EndpointBasedAuthSchemeOptionResolver {
    auth_scheme_ids: Vec<AuthSchemeId>,
}

impl EndpointBasedAuthSchemeOptionResolver {
    /// Creates a new instance of `StaticAuthSchemeOptionResolver`.
    pub(crate) fn new(auth_scheme_ids: Vec<AuthSchemeId>) -> Self {
        Self { auth_scheme_ids }
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

            let mut endpoint_auth_scheme_id_strs = Vec::new();

            if let Some(aws_smithy_types::Document::Array(endpoint_auth_schemes)) =
                endpoint.properties().get("authSchemes")
            {
                for endpoint_auth_scheme in endpoint_auth_schemes {
                    let scheme_id_str = endpoint_auth_scheme
                        .as_object()
                        .and_then(|object| object.get("name"))
                        .and_then(aws_smithy_types::Document::as_string);
                    if let Some(scheme_id_str) = scheme_id_str {
                        endpoint_auth_scheme_id_strs.push(scheme_id_str);
                    }
                }
            }

            let result = move_endpoint_auth_scheme_ids_to_front(
                &self.auth_scheme_ids,
                &endpoint_auth_scheme_id_strs,
            );

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

// Returns newly allocated `Vec<AuthSchemeId>` with the same elements from `model_auth_scheme_options`
// but with those appear in `endpoint_auth_scheme_strs` moved to the front of the list.
fn move_endpoint_auth_scheme_ids_to_front(
    model_auth_scheme_options: &[AuthSchemeId],
    endpoint_auth_scheme_strs: &[&str],
) -> Vec<AuthSchemeId> {
    // Right after `partition`, `result` only contains the intersection of `AuthSchemeId`s that appear both in the endpoint and in the model.
    let (mut result, model_only_auth_scheme_ids): (Vec<_>, Vec<_>) = model_auth_scheme_options
        .iter()
        .partition(|auth_scheme_id| endpoint_auth_scheme_strs.contains(&auth_scheme_id.as_str()));

    // Sort result according to the order of elements in endpoint_auth_scheme_strs.
    result.sort_by(|a: &AuthSchemeId, b: &AuthSchemeId| {
        let index_a = endpoint_auth_scheme_strs
            .iter()
            .position(|&x| x == a.as_str())
            .unwrap();
        let index_b = endpoint_auth_scheme_strs
            .iter()
            .position(|&x| x == b.as_str())
            .unwrap();
        index_a.cmp(&index_b)
    });

    // Extend `result` with `AuthSchemeId`s that only appear in the model.
    // As a result, the intersection of `AuthSchemeId`s are brought to the front of the vec,
    // while placing those that only exist in the model towards the end of the vec.
    result.extend(model_only_auth_scheme_ids.iter());

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_endpoint_auth_scheme_ids_to_front_basic() {
        let model_auth_scheme_ids = ["schemeA", "schemeX", "schemeB", "schemeY"]
            .into_iter()
            .map(AuthSchemeId::from)
            .collect::<Vec<_>>();
        let endpoint_auth_scheme_id_strs = vec!["schemeY", "schemeX"];
        let expected = ["schemeY", "schemeX", "schemeA", "schemeB"]
            .into_iter()
            .map(AuthSchemeId::from)
            .collect::<Vec<_>>();
        let actual = move_endpoint_auth_scheme_ids_to_front(
            &model_auth_scheme_ids,
            &endpoint_auth_scheme_id_strs,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn move_endpoint_auth_scheme_ids_to_front_with_empty_endpoint_auth_scheme_ids() {
        let model_auth_scheme_ids = ["schemeA", "schemeX", "schemeB", "schemeY"]
            .into_iter()
            .map(AuthSchemeId::from)
            .collect::<Vec<_>>();
        let endpoint_auth_scheme_id_strs = vec![""];
        let actual = move_endpoint_auth_scheme_ids_to_front(
            &model_auth_scheme_ids,
            &endpoint_auth_scheme_id_strs,
        );
        assert_eq!(model_auth_scheme_ids, actual);
    }

    #[test]
    fn move_endpoint_auth_scheme_ids_to_front_with_foreign_endpoint_auth_scheme_ids() {
        let model_auth_scheme_ids = ["schemeA", "schemeX", "schemeB", "schemeY"]
            .into_iter()
            .map(AuthSchemeId::from)
            .collect::<Vec<_>>();
        let endpoint_auth_scheme_id_strs = vec!["schemeY", "schemeZ"];
        let expected = ["schemeY", "schemeA", "schemeX", "schemeB"]
            .into_iter()
            .map(AuthSchemeId::from)
            .collect::<Vec<_>>();
        let actual = move_endpoint_auth_scheme_ids_to_front(
            &model_auth_scheme_ids,
            &endpoint_auth_scheme_id_strs,
        );
        assert_eq!(expected, actual);
    }
}
