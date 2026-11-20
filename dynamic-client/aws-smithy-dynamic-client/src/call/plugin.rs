/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::auth;
use crate::client::warn_unused_keys;
use crate::config::{self, puts_plugin, ConfigParts};
use crate::error::DynamicOperationError;
use crate::serde::{DocumentDeserializer, DocumentSerializer};
use crate::DynamicClient;
use aws_smithy_lang::ShapeId;
use aws_smithy_runtime::client::auth::no_auth::{NoAuthScheme, NO_AUTH_SCHEME_ID};
use aws_smithy_runtime::client::identity::no_auth::NoAuthIdentityResolver;
use aws_smithy_runtime::client::identity::IdentityCache;
use aws_smithy_runtime::client::retries::classifiers::{
    ModeledAsRetryableClassifier, TransientErrorClassifier,
};
use aws_smithy_runtime_api::client::auth::{AuthSchemeOptionResolverParams, SharedAuthScheme};
use aws_smithy_runtime_api::client::endpoint::EndpointResolverParams;
use aws_smithy_runtime_api::client::identity::SharedIdentityResolver;
use aws_smithy_runtime_api::client::orchestrator::Metadata;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::client::runtime_plugin::{SharedRuntimePlugin, StaticRuntimePlugin};
use aws_smithy_runtime_api::client::ser_de::{SharedRequestSerializer, SharedResponseDeserializer};
use aws_smithy_types::config_bag::Layer;

// Returns the operation plugin of one call. The plugin stores the same config as a generated
// operation plugin.
pub(crate) fn operation_plugin(client: &DynamicClient, operation: &ShapeId) -> StaticRuntimePlugin {
    let mut layer = Layer::new("DynamicOperation");
    layer.store_put(SharedRequestSerializer::new(DocumentSerializer));
    layer.store_put(SharedResponseDeserializer::new(DocumentDeserializer::new(
        client.schemas.clone(),
        operation.clone(),
    )));
    layer.store_put(Metadata::new(
        operation.name().to_owned(),
        client.sdk_id.to_string(),
    ));
    // TODO(aws-smithy-lang): Store `SensitiveOutput` for an operation whose output reaches a
    // `@sensitive` shape, as generated clients do, once a `SensitiveIndex` exists. Until then, the
    // orchestrator logs the whole response body at trace level, with any sensitive values in it.
    let model = client.schemas.model();
    let operation_view = model
        .get_shape(operation)
        .and_then(|shape| shape.as_shape())
        .and_then(|shape| shape.as_operation())
        .expect("the client's operations are operation shapes of the model");
    layer.store_put(AuthSchemeOptionResolverParams::new(auth::params(
        model,
        &client.service_view(),
        &operation_view,
    )));
    // TODO(dynamic-client): Resolve the endpoint from the model's endpoint rules.
    layer.store_put(EndpointResolverParams::new(()));
    let components = RuntimeComponentsBuilder::new("DynamicOperation")
        .with_retry_classifier(TransientErrorClassifier::<DynamicOperationError>::new())
        .with_retry_classifier(ModeledAsRetryableClassifier::<DynamicOperationError>::new())
        .with_auth_scheme(SharedAuthScheme::new(NoAuthScheme::new()))
        .with_identity_resolver(
            NO_AUTH_SCHEME_ID,
            SharedIdentityResolver::new(NoAuthIdentityResolver::new()),
        );
    // An AWS service marks throttling and transient errors by error code, such as
    // `ThrottlingException`. An AWS SDK client retries those codes, and so does this client.
    #[cfg(feature = "aws")]
    let components =
        components
            .with_retry_classifier(aws_runtime::retries::classifiers::AwsErrorCodeClassifier::<
                DynamicOperationError,
            >::new());
    StaticRuntimePlugin::new()
        .with_config(layer.freeze())
        .with_runtime_components(components)
}

// Returns the plugins of a per-call config override, in the order generated clients apply them.
// The override's runtime plugins come first, then a plugin for the override's config. The
// values set by `put` come last.
pub(super) fn per_call_plugins(
    client: &DynamicClient,
    mut config_override: config::Builder,
) -> Vec<SharedRuntimePlugin> {
    let puts = std::mem::take(&mut config_override.puts);
    warn_unused_keys(&client.service_view(), &puts, &client.unused_keys);
    let ConfigParts {
        layer,
        mut components,
        runtime_plugins: mut plugins,
    } = config_override.into_parts("DynamicConfigOverride");
    // An override with its own identity resolver gets a short-lived identity cache, so the
    // client's cache gains no partition for it. A plain lazy cache is enough, even for AWS. A
    // per-call identity lives only as long as the call, so it needs no static stability cache.
    if components.has_identity_resolvers() && components.identity_cache().is_none() {
        components.set_identity_cache(Some(IdentityCache::lazy().max_partitions(1).build()));
    }
    plugins.push(SharedRuntimePlugin::new(
        StaticRuntimePlugin::new()
            .with_config(layer.freeze())
            .with_runtime_components(components),
    ));
    plugins.extend(puts_plugin(&puts).map(SharedRuntimePlugin::new));
    plugins
}
