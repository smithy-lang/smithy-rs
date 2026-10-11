/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::endpoint::{
    error::ResolveEndpointError, EndpointFuture, EndpointResolverParams, ResolveEndpoint,
};
use aws_smithy_runtime_api::client::identity::Identity;
use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::protocol::ConfiguredProtocol;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::{box_error::BoxError, client::endpoint::EndpointPrefix};
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::endpoint::Endpoint;
use http_1x::header::HeaderName;
use http_1x::uri::PathAndQuery;
use http_1x::{HeaderValue, Uri};
use std::borrow::Cow;
use std::fmt::Debug;
use std::str::FromStr;
use tracing::trace;

/// An endpoint resolver that uses a static URI.
#[derive(Clone, Debug)]
pub struct StaticUriEndpointResolver {
    endpoint: String,
}

impl StaticUriEndpointResolver {
    /// Create a resolver that resolves to `http://localhost:{port}`.
    pub fn http_localhost(port: u16) -> Self {
        Self {
            endpoint: format!("http://localhost:{port}"),
        }
    }

    /// Create a resolver that resolves to the given URI.
    pub fn uri(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }
}

impl ResolveEndpoint for StaticUriEndpointResolver {
    fn resolve_endpoint<'a>(&'a self, _params: &'a EndpointResolverParams) -> EndpointFuture<'a> {
        EndpointFuture::ready(Ok(Endpoint::builder()
            .url(self.endpoint.to_string())
            .build()))
    }
}

/// Empty params to be used with [`StaticUriEndpointResolver`].
#[derive(Debug, Default)]
pub struct StaticUriEndpointResolverParams;

impl StaticUriEndpointResolverParams {
    /// Creates a new `StaticUriEndpointResolverParams`.
    pub fn new() -> Self {
        Self
    }
}

impl From<StaticUriEndpointResolverParams> for EndpointResolverParams {
    fn from(params: StaticUriEndpointResolverParams) -> Self {
        EndpointResolverParams::new(params)
    }
}

pub(super) async fn orchestrate_endpoint(
    identity: Identity,
    ctx: &mut InterceptorContext,
    runtime_components: &RuntimeComponents,
    cfg: &mut ConfigBag,
) -> Result<(), BoxError> {
    trace!("orchestrating endpoint resolution");

    let params = cfg
        .get_mut_from_interceptor_state::<EndpointResolverParams>()
        .expect("endpoint resolver params must be set in the interceptor state");
    params.set_property(identity);

    let endpoint_resolver = runtime_components.endpoint_resolver();

    endpoint_resolver.finalize_params(params)?;

    tracing::debug!(endpoint_params = ?params, "resolving endpoint");
    let endpoint = endpoint_resolver.resolve_endpoint(params).await?;

    apply_endpoint(&endpoint, ctx, cfg)?;

    // Make the endpoint config available to interceptors
    cfg.interceptor_state().store_put(endpoint);
    Ok(())
}

pub(super) fn apply_endpoint(
    endpoint: &Endpoint,
    ctx: &mut InterceptorContext,
    cfg: &ConfigBag,
) -> Result<(), BoxError> {
    let endpoint_prefix = cfg.load::<EndpointPrefix>();
    tracing::debug!(endpoint_prefix = ?endpoint_prefix, "will apply endpoint {:?}", endpoint);
    let request = ctx.request_mut().expect("set during serialization");

    // If a schema-driven protocol is in use, delegate endpoint application to it. The bag entry is
    // the version-stable `ConfiguredProtocol`, so this works without depending on the crate (or
    // the major version of the crate) that defines the protocol.
    if let Some(protocol) = cfg.load::<ConfiguredProtocol>() {
        protocol.update_endpoint(request, endpoint, cfg)?;
    } else {
        apply_endpoint_to_request(request, endpoint, endpoint_prefix)?;
    }

    Ok(())
}

fn apply_endpoint_to_request(
    request: &mut HttpRequest,
    endpoint: &Endpoint,
    endpoint_prefix: Option<&EndpointPrefix>,
) -> Result<(), BoxError> {
    let endpoint_url = match endpoint_prefix {
        None => Cow::Borrowed(endpoint.url()),
        Some(prefix) => {
            let parsed = endpoint.url().parse::<Uri>()?;
            let scheme = parsed.scheme_str().unwrap_or_default();
            let prefix = prefix.as_str();
            let authority = parsed
                .authority()
                .map(|auth| auth.as_str())
                .unwrap_or_default();
            let path_and_query = parsed
                .path_and_query()
                .map(PathAndQuery::as_str)
                .unwrap_or_default();
            Cow::Owned(format!("{scheme}://{prefix}{authority}{path_and_query}"))
        }
    };

    request
        .uri_mut()
        .set_endpoint(&endpoint_url)
        .map_err(|err| {
            ResolveEndpointError::message(format!(
                "failed to apply endpoint `{endpoint_url}` to request `{request:?}`",
            ))
            .with_source(Some(err.into()))
        })?;

    for (header_name, header_values) in endpoint.headers() {
        request.headers_mut().remove(header_name);
        for value in header_values {
            request.headers_mut().append(
                HeaderName::from_str(header_name).map_err(|err| {
                    ResolveEndpointError::message("invalid header name")
                        .with_source(Some(err.into()))
                })?,
                HeaderValue::from_str(value).map_err(|err| {
                    ResolveEndpointError::message("invalid header value")
                        .with_source(Some(err.into()))
                })?,
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use aws_smithy_runtime_api::client::endpoint::EndpointPrefix;
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use aws_smithy_types::endpoint::Endpoint;

    #[test]
    fn test_apply_endpoint() {
        let mut req = HttpRequest::empty();
        req.set_uri("/foo?bar=1").unwrap();
        let endpoint = Endpoint::builder().url("https://s3.amazon.com").build();
        let prefix = EndpointPrefix::new("prefix.subdomain.").unwrap();
        super::apply_endpoint_to_request(&mut req, &endpoint, Some(&prefix))
            .expect("should succeed");
        assert_eq!(
            req.uri(),
            "https://prefix.subdomain.s3.amazon.com/foo?bar=1"
        );
    }

    #[derive(Debug)]
    struct HeaderOnlyProtocol;

    impl
        aws_smithy_runtime_api::client::versioned_config::ConfigPayloadFor<
            aws_smithy_runtime_api::client::protocol::ClientProtocolSlot,
        > for HeaderOnlyProtocol
    {
        const REPRESENTATION: aws_smithy_runtime_api::client::versioned_config::RepresentationId =
            aws_smithy_runtime_api::client::versioned_config::RepresentationId::new(
                "test-protocols",
                "1",
                1,
            );
    }

    impl aws_smithy_runtime_api::client::protocol::ProtocolHandle for HeaderOnlyProtocol {
        fn update_endpoint(
            &self,
            request: &mut HttpRequest,
            endpoint: &Endpoint,
            _cfg: &aws_smithy_types::config_bag::ConfigBag,
        ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
            request
                .headers_mut()
                .insert("x-endpoint", endpoint.url().to_owned());
            Ok(())
        }
    }

    #[test]
    fn configured_protocol_owns_endpoint_application() {
        use aws_smithy_runtime_api::client::interceptors::context::{Input, InterceptorContext};
        use aws_smithy_runtime_api::client::protocol::ConfiguredProtocol;
        use aws_smithy_types::config_bag::{ConfigBag, Layer};

        let mut layer = Layer::new("test");
        layer.store_put(ConfiguredProtocol::new(HeaderOnlyProtocol));
        let cfg = ConfigBag::of_layers(vec![layer]);

        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.enter_serialization_phase();
        let _ = ctx.take_input();
        let mut req = HttpRequest::empty();
        req.set_uri("/foo").unwrap();
        ctx.set_request(req);
        ctx.enter_before_transmit_phase();

        let endpoint = Endpoint::builder().url("https://example.com").build();
        super::apply_endpoint(&endpoint, &mut ctx, &cfg).expect("should succeed");

        let req = ctx.request().unwrap();
        // The protocol, not the default implementation, applied the endpoint.
        assert_eq!(req.uri(), "/foo");
        assert_eq!(req.headers().get("x-endpoint"), Some("https://example.com"));
    }
}
