/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{borrow::Cow, collections::HashMap};

use aws_smithy_runtime_api::client::{
    auth::{AuthSchemeId, AuthSchemeOption},
    endpoint::ResolveEndpoint,
    runtime_components::RuntimeComponentsBuilder,
    runtime_plugin::{Order, RuntimePlugin},
};
use aws_smithy_types::{config_bag::Layer, Document};
use aws_types::{
    region::{Region, SigningRegion},
    SigningName,
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
    modeled_auth_scheme_options: Vec<AuthSchemeOption>,
}

impl EndpointBasedAuthSchemeOptionResolver {
    /// Creates a new instance of `EndpointBasedAuthSchemeOptionResolver`.
    pub(crate) fn new(modeled_auth_scheme_ids: Vec<AuthSchemeId>) -> Self {
        Self {
            modeled_auth_scheme_options: modeled_auth_scheme_ids
                .into_iter()
                .map(|scheme_id| {
                    AuthSchemeOption::builder()
                        .scheme_id(scheme_id.clone())
                        .build()
                        .expect("required field set")
                })
                .collect::<Vec<_>>(),
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

            let endpoint_auth_scheme_ids = endpoint
                .properties()
                .get("authSchemes")
                .and_then(|prop| prop.as_array())
                .map(|array| {
                    array
                        .into_iter()
                        .flat_map(|auth_schemes_property| {
                            auth_scheme_option_from_auth_schemes_property(auth_schemes_property)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            Ok(merge_auth_scheme_options(
                &self.modeled_auth_scheme_options,
                endpoint_auth_scheme_ids,
            ))
        })
    }
}

// Merge a list of `AuthSchemeId`s both in `modeled_auth_scheme_ids` and in `endpoint_auth_scheme_ids`,
// but with those in `endpoint_auth_scheme_ids` placed at the front of the resulting list.
fn merge_auth_scheme_options(
    modeled_auth_scheme_options: &[AuthSchemeOption],
    mut endpoint_auth_scheme_options: Vec<AuthSchemeOption>,
) -> Vec<AuthSchemeOption> {
    let (_, model_only_auth_scheme_options): (Vec<_>, Vec<_>) = modeled_auth_scheme_options
        .iter()
        .partition(|auth_scheme_option| {
            endpoint_auth_scheme_options
                .iter()
                .any(|opt| opt.scheme_id() == auth_scheme_option.scheme_id())
        });

    // There is no need to merge properties from `modeled_auth_scheme_options` because they have already been added to the config bag
    // within client's config `Builder`:
    // https://github.com/awslabs/aws-sdk-rust/blob/cb818835c3846d030a81ec1dcda5f91d56326f71/sdk/s3/src/config.rs#L1226-L1230
    // Merging them again within endpoint `AuthSchemeOption`'s `FrozenLayer` means we would end up adding modeled `AuthSchemeOption`'s properties
    // to the config bag, again.

    endpoint_auth_scheme_options.extend(model_only_auth_scheme_options.into_iter().cloned());

    endpoint_auth_scheme_options
}

fn auth_scheme_option_from_auth_schemes_property<'a>(
    auth_schemes_property: &Document,
) -> Option<AuthSchemeOption> {
    fn get<'a>(field_name: &'static str, object: &'a HashMap<String, Document>) -> Option<&'a str> {
        object
            .get(field_name)
            .and_then(aws_smithy_types::Document::as_string)
    }

    if let Some(object) = auth_schemes_property.as_object() {
        if let Some(auch_scheme_id) = get("name", object) {
            let mut layer = Layer::new("Test");
            get("signingRegion", object)
                .map(|s| layer.store_put(SigningRegion::from(Region::new(s.to_owned()))));
            get("signingName", object).map(|s| layer.store_put(SigningName::from(s.to_owned())));
            return Some(
                AuthSchemeOption::builder()
                    .scheme_id(AuthSchemeId::from(Cow::Owned(auch_scheme_id.to_owned())))
                    .properties(layer.freeze())
                    .build()
                    .expect("required fields set"),
            );
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn into_auth_scheme_options<const N: usize>(strs: [&'static str; N]) -> Vec<AuthSchemeOption> {
        strs.into_iter()
            .map(|s| {
                AuthSchemeOption::builder()
                    .scheme_id(AuthSchemeId::from(s))
                    .build()
                    .unwrap()
            })
            .collect::<Vec<_>>()
    }

    #[test]
    fn merge_auth_scheme_options_basic() {
        let modeled_auth_scheme_options =
            into_auth_scheme_options(["schemeA", "schemeX", "schemeB", "schemeY"]);
        let endpoint_auth_scheme_options = into_auth_scheme_options(["schemeY", "schemeX"]);
        let expected = ["schemeY", "schemeX", "schemeA", "schemeB"];
        let actual =
            merge_auth_scheme_options(&modeled_auth_scheme_options, endpoint_auth_scheme_options);
        assert_eq!(
            expected.to_vec(),
            actual
                .iter()
                .map(|opt| opt.scheme_id().inner())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn merge_auth_scheme_options_with_empty_endpoint_auth_scheme_options() {
        let expected = ["schemeA", "schemeX", "schemeB", "schemeY"];
        let modeled_auth_scheme_options = into_auth_scheme_options(expected);
        let endpoint_auth_scheme_options = Vec::new();
        let actual =
            merge_auth_scheme_options(&modeled_auth_scheme_options, endpoint_auth_scheme_options);
        assert_eq!(
            expected.to_vec(),
            actual
                .iter()
                .map(|opt| opt.scheme_id().inner())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn merge_auth_scheme_options_should_also_include_those_only_in_endpoint_auth_scheme_options() {
        let modeled_auth_scheme_ids =
            into_auth_scheme_options(["schemeA", "schemeX", "schemeB", "schemeY"]);
        let endpoint_auth_scheme_ids = into_auth_scheme_options(["schemeY", "schemeZ"]);
        let expected = ["schemeY", "schemeZ", "schemeA", "schemeX", "schemeB"];
        let actual = merge_auth_scheme_options(&modeled_auth_scheme_ids, endpoint_auth_scheme_ids);
        assert_eq!(
            expected.to_vec(),
            actual
                .iter()
                .map(|opt| opt.scheme_id().inner())
                .collect::<Vec<_>>()
        );
    }
}
