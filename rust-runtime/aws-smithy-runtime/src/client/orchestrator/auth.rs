/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::auth::no_auth::NO_AUTH_SCHEME_ID;
use crate::client::identity::IdentityCache;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::auth::{
    AuthScheme, AuthSchemeEndpointConfig, AuthSchemeId, AuthSchemeOptionResolverParams,
    ResolveAuthSchemeOptions,
};
use aws_smithy_runtime_api::client::endpoint::{EndpointResolverParams, ResolveEndpoint};
use aws_smithy_runtime_api::client::identity::Identity;
use aws_smithy_runtime_api::client::identity::ResolveIdentity;
use aws_smithy_runtime_api::client::identity::{IdentityCacheLocation, ResolveCachedIdentity};
use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::{ConfigBag, Layer, Storable, StoreReplace};
use aws_smithy_types::endpoint::Endpoint;
use aws_smithy_types::Document;
use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;
use tracing::trace;

#[derive(Debug)]
enum AuthOrchestrationError {
    MissingEndpointConfig,
    BadAuthSchemeEndpointConfig(Cow<'static, str>),
}

impl fmt::Display for AuthOrchestrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // This error is never bubbled up
            Self::MissingEndpointConfig => f.write_str("missing endpoint config"),
            Self::BadAuthSchemeEndpointConfig(message) => f.write_str(message),
        }
    }
}

impl StdError for AuthOrchestrationError {}

#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct NoEndpointRequiredForAuthSchemeResolution;

impl Storable for NoEndpointRequiredForAuthSchemeResolution {
    type Storer = StoreReplace<Self>;
}

pub(super) async fn resolve_auth_scheme(
    _ctx: &mut InterceptorContext,
    runtime_components: &RuntimeComponents,
    cfg: &mut ConfigBag,
) -> Result<AuthSchemeId, BoxError> {
    let params = cfg
        .load::<AuthSchemeOptionResolverParams>()
        .expect("auth scheme option resolver params must be set");
    let option_resolver = runtime_components.auth_scheme_option_resolver();
    let options = option_resolver
        .resolve_auth_scheme_options_v2(params, cfg, runtime_components)
        .await?;

    for &scheme_id in options.as_ref() {
        if let Some(auth_scheme) = runtime_components.auth_scheme(scheme_id) {
            if let Some(identity_resolver) = auth_scheme.identity_resolver(runtime_components) {
                if endpoint_auth_scheme_matches_configured_scheme_id(
                    scheme_id,
                    runtime_components,
                    cfg,
                )
                .await
                {
                    let identity_cache = if identity_resolver.cache_location()
                        == IdentityCacheLocation::RuntimeComponents
                    {
                        runtime_components.identity_cache()
                    } else {
                        IdentityCache::no_cache()
                    };
                    let identity = identity_cache
                        .resolve_cached_identity(identity_resolver, runtime_components, cfg)
                        .await?;
                    trace!(identity = ?identity, "resolved identity");
                    let mut layer = Layer::new("resolve_identity");
                    layer.store_put(identity);
                    cfg.push_layer(layer);
                    return Ok(scheme_id);
                }
            }
        }
    }

    Err("No matching auth scheme option".into())
}

async fn endpoint_auth_scheme_matches_configured_scheme_id(
    scheme_id: AuthSchemeId,
    runtime_components: &RuntimeComponents,
    cfg: &ConfigBag,
) -> bool {
    if cfg
        .load::<NoEndpointRequiredForAuthSchemeResolution>()
        .is_some()
    {
        // Make it noop for services that don't require this function for auth scheme resolution
        return true;
    }

    let params = if let Some(params) = cfg.load::<EndpointResolverParams>() {
        params
    } else {
        return false;
    };

    let endpoint = runtime_components
        .endpoint_resolver()
        .resolve_endpoint(params)
        .await
        .unwrap();

    extract_endpoint_auth_scheme_config(&endpoint, scheme_id).is_ok()
}

pub(super) fn sign_request(
    scheme_id: AuthSchemeId,
    ctx: &mut InterceptorContext,
    runtime_components: &RuntimeComponents,
    cfg: &ConfigBag,
) -> Result<(), BoxError> {
    trace!("signing request");
    let request = ctx.request_mut().expect("set during serialization");
    let identity = cfg
        .load::<Identity>()
        .expect("identity should be set by `resolve_identity`");
    let endpoint = cfg
        .load::<Endpoint>()
        .expect("endpoint added to config bag by endpoint orchestrator");
    let auth_scheme = runtime_components
        .auth_scheme(scheme_id)
        .ok_or("should be configured")?;
    let signer = auth_scheme.signer();
    let auth_scheme_endpoint_config = extract_endpoint_auth_scheme_config(&endpoint, scheme_id)?;
    trace!(
        signer = ?signer,
        "signing implementation"
    );
    signer.sign_http_request(
        request,
        &identity,
        auth_scheme_endpoint_config,
        runtime_components,
        cfg,
    )?;
    return Ok(());
}

fn extract_endpoint_auth_scheme_config(
    endpoint: &Endpoint,
    scheme_id: AuthSchemeId,
) -> Result<AuthSchemeEndpointConfig<'_>, AuthOrchestrationError> {
    // TODO(P96049742): Endpoint config doesn't currently have a concept of optional auth or "no auth", so
    // we are short-circuiting lookup of endpoint auth scheme config if that is the selected scheme.
    if scheme_id == NO_AUTH_SCHEME_ID {
        return Ok(AuthSchemeEndpointConfig::empty());
    }
    let auth_schemes = match endpoint.properties().get("authSchemes") {
        Some(Document::Array(schemes)) => schemes,
        // no auth schemes:
        None => return Ok(AuthSchemeEndpointConfig::empty()),
        _other => {
            return Err(AuthOrchestrationError::BadAuthSchemeEndpointConfig(
                "expected an array for `authSchemes` in endpoint config".into(),
            ))
        }
    };
    let auth_scheme_config = auth_schemes
        .iter()
        .find(|doc| {
            let config_scheme_id = doc
                .as_object()
                .and_then(|object| object.get("name"))
                .and_then(Document::as_string);
            config_scheme_id == Some(scheme_id.as_str())
        })
        .ok_or(AuthOrchestrationError::MissingEndpointConfig)?;
    Ok(AuthSchemeEndpointConfig::from(Some(auth_scheme_config)))
}
