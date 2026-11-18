/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::warn_unused_keys;
use crate::config::{self, puts_plugin, ConfigParts};
use crate::serde::{DocumentDeserializer, DocumentSerializer};
use crate::DynamicClient;
use aws_smithy_lang::ShapeId;
use aws_smithy_runtime::client::auth::no_auth::{NoAuthScheme, NO_AUTH_SCHEME_ID};
use aws_smithy_runtime::client::identity::no_auth::NoAuthIdentityResolver;
use aws_smithy_runtime::client::identity::IdentityCache;
use aws_smithy_runtime_api::client::auth::static_resolver::StaticAuthSchemeOptionResolver;
use aws_smithy_runtime_api::client::auth::{
    AuthSchemeOptionResolverParams, SharedAuthScheme, SharedAuthSchemeOptionResolver,
};
use aws_smithy_runtime_api::client::endpoint::EndpointResolverParams;
use aws_smithy_runtime_api::client::identity::SharedIdentityResolver;
use aws_smithy_runtime_api::client::orchestrator::Metadata;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::client::runtime_plugin::{SharedRuntimePlugin, StaticRuntimePlugin};
use aws_smithy_runtime_api::client::ser_de::{SharedRequestSerializer, SharedResponseDeserializer};
use aws_smithy_types::config_bag::Layer;

// Returns the operation plugin of one call. The plugin stores the same config as a generated
// operation plugin.
pub(super) fn operation_plugin(client: &DynamicClient, operation: &ShapeId) -> StaticRuntimePlugin {
    let mut layer = Layer::new("DynamicOperation");
    let schemas = client.schemas.clone();
    let operation_id = operation.clone();
    layer.store_put(SharedRequestSerializer::new(DocumentSerializer::new(
        schemas.clone(),
        operation_id.clone(),
    )));
    layer.store_put(SharedResponseDeserializer::new(DocumentDeserializer::new(
        schemas,
        operation_id,
    )));
    layer.store_put(Metadata::new(
        operation.name().to_owned(),
        client.sdk_id.to_string(),
    ));
    // TODO(dynamic-client): Resolve the operation's auth options from the model.
    layer.store_put(AuthSchemeOptionResolverParams::new(()));
    // TODO(dynamic-client): Resolve the endpoint from the model's endpoint rules.
    layer.store_put(EndpointResolverParams::new(()));
    let components = RuntimeComponentsBuilder::new("DynamicOperation")
        .with_auth_scheme_option_resolver(Some(SharedAuthSchemeOptionResolver::new(
            StaticAuthSchemeOptionResolver::new(vec![NO_AUTH_SCHEME_ID]),
        )))
        .with_auth_scheme(SharedAuthScheme::new(NoAuthScheme::new()))
        .with_identity_resolver(
            NO_AUTH_SCHEME_ID,
            SharedIdentityResolver::new(NoAuthIdentityResolver::new()),
        );
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
    warn_unused_keys(&client.service_view(), &puts);
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
