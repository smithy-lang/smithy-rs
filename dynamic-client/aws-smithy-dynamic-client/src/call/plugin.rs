/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::serde::{DocumentDeserializer, DocumentSerializer};
use crate::DynamicClient;
use aws_smithy_lang::ShapeId;
use aws_smithy_runtime::client::auth::no_auth::{NoAuthScheme, NO_AUTH_SCHEME_ID};
use aws_smithy_runtime::client::identity::no_auth::NoAuthIdentityResolver;
use aws_smithy_runtime_api::client::auth::static_resolver::StaticAuthSchemeOptionResolver;
use aws_smithy_runtime_api::client::auth::{
    AuthSchemeOptionResolverParams, SharedAuthScheme, SharedAuthSchemeOptionResolver,
};
use aws_smithy_runtime_api::client::endpoint::EndpointResolverParams;
use aws_smithy_runtime_api::client::identity::SharedIdentityResolver;
use aws_smithy_runtime_api::client::orchestrator::Metadata;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::client::runtime_plugin::StaticRuntimePlugin;
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
