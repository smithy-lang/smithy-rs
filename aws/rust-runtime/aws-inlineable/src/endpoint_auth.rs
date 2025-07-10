/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;

use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::{
        auth::{AuthSchemeId, AuthSchemeOption},
        endpoint::{EndpointResolverParams, ResolveEndpoint},
        runtime_components::RuntimeComponents,
    },
};
use aws_smithy_types::config_bag::ConfigBag;

pub(crate) async fn resolve_endpoint_based_auth_scheme_options<'a>(
    modeled_auth_scheme_options: &'a [AuthSchemeOption],
    cfg: &'a ConfigBag,
    runtime_components: &'a RuntimeComponents,
) -> Result<Vec<AuthSchemeOption>, BoxError> {
    let endpoint_params = cfg
        .load::<EndpointResolverParams>()
        .expect("endpoint resolver params must be set");

    tracing::debug!(endpoint_params = ?endpoint_params, "resolving endpoint for auth scheme selection");

    let endpoint = runtime_components
        .endpoint_resolver()
        .resolve_endpoint(endpoint_params)
        .await?;

    let mut endpoint_auth_scheme_ids = Vec::new();

    // Note that we're not constructing the `properties` for `endpoint_auth_schemes` here—only collecting
    // auth scheme IDs but not properties. This is because, at this stage, we're only determining which auth schemes will be candidates.
    // Any `authSchemes` list properties that influence the signing context will be extracted later
    // in `AuthSchemeEndpointConfig`, and passed by the orchestrator to the signer's `sign_http_request` method.
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

    Ok(merge_auth_scheme_options(
        modeled_auth_scheme_options,
        endpoint_auth_scheme_ids,
    ))
}

// Returns a list of merged auth scheme options from `modeled_auth_scheme_options` and `endpoint_auth_scheme_ids`,
// copying properties from the modeled auth scheme options into the endpoint auth scheme options as they are built.
//
// Note: We only extract properties from the modeled auth schemes. Pulling properties from the endpoint auth schemes
// would result in duplication; they would be added here and again in the `extract_operation_config` function during signing.
fn merge_auth_scheme_options(
    modeled_auth_scheme_options: &[AuthSchemeOption],
    endpoint_auth_scheme_ids: Vec<AuthSchemeId>,
) -> Vec<AuthSchemeOption> {
    let (common_auth_scheme_options, model_only_auth_scheme_options): (Vec<_>, Vec<_>) =
        modeled_auth_scheme_options
            .iter()
            .partition(|auth_scheme_option| {
                endpoint_auth_scheme_ids.contains(auth_scheme_option.scheme_id())
            });

    let mut endpoint_auth_scheme_options = endpoint_auth_scheme_ids
        .into_iter()
        .map(|id| {
            let modelded = common_auth_scheme_options
                .iter()
                .find(|opt| opt.scheme_id() == &id)
                .cloned();
            let mut builder = AuthSchemeOption::builder().scheme_id(id);
            builder.set_properties(modelded.and_then(|m| m.properties()));
            builder.build().unwrap()
        })
        .collect::<Vec<_>>();

    endpoint_auth_scheme_options.extend(model_only_auth_scheme_options.into_iter().cloned());

    endpoint_auth_scheme_options
}

#[cfg(test)]
mod tests {
    use aws_runtime::auth::PayloadSigningOverride;
    use aws_smithy_types::config_bag::Layer;

    use super::*;

    fn into_auth_scheme_ids<const N: usize>(strs: [&'static str; N]) -> Vec<AuthSchemeId> {
        strs.into_iter().map(AuthSchemeId::from).collect::<Vec<_>>()
    }

    fn into_auth_scheme_options<const N: usize>(strs: [&'static str; N]) -> Vec<AuthSchemeOption> {
        strs.into_iter()
            .map(|s| AuthSchemeOption::from(AuthSchemeId::from(s)))
            .collect::<Vec<_>>()
    }

    #[test]
    fn merge_auth_scheme_options_basic() {
        let modeled_auth_scheme_options =
            into_auth_scheme_options(["schemeA", "schemeX", "schemeB", "schemeY"]);
        let endpoint_auth_scheme_ids = into_auth_scheme_ids(["schemeY", "schemeX"]);
        let expected = ["schemeY", "schemeX", "schemeA", "schemeB"];
        let actual =
            merge_auth_scheme_options(&modeled_auth_scheme_options, endpoint_auth_scheme_ids);
        assert_eq!(
            expected.to_vec(),
            actual
                .iter()
                .map(|opt| opt.scheme_id().inner())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn merge_auth_scheme_options_preserving_modeled_auth_properties() {
        let mut modeled_auth_scheme_options =
            into_auth_scheme_options(["schemeA", "schemeX", "schemeB"]);
        modeled_auth_scheme_options.push(
            AuthSchemeOption::builder()
                .scheme_id(AuthSchemeId::new("schemeY"))
                .properties({
                    let mut layer = Layer::new("TestAuthSchemeProperties");
                    layer.store_put(PayloadSigningOverride::unsigned_payload());
                    layer.freeze()
                })
                .build()
                .unwrap(),
        );
        let endpoint_auth_scheme_ids = into_auth_scheme_ids(["schemeY", "schemeX"]);
        let expected = ["schemeY", "schemeX", "schemeA", "schemeB"];
        let actual =
            merge_auth_scheme_options(&modeled_auth_scheme_options, endpoint_auth_scheme_ids);
        assert_eq!(
            expected.to_vec(),
            actual
                .iter()
                .map(|opt| opt.scheme_id().inner())
                .collect::<Vec<_>>()
        );
        let prop = actual.first().unwrap().properties().unwrap();
        assert!(matches!(
            prop.load::<PayloadSigningOverride>().unwrap(),
            PayloadSigningOverride::UnsignedPayload
        ));
    }

    #[test]
    fn merge_auth_scheme_options_with_empty_endpoint_auth_scheme_options() {
        let expected = ["schemeA", "schemeX", "schemeB", "schemeY"];
        let modeled_auth_scheme_options = into_auth_scheme_options(expected);
        let endpoint_auth_scheme_ids = Vec::new();
        let actual =
            merge_auth_scheme_options(&modeled_auth_scheme_options, endpoint_auth_scheme_ids);
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
        let endpoint_auth_scheme_ids = into_auth_scheme_ids(["schemeY", "schemeZ"]);
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
