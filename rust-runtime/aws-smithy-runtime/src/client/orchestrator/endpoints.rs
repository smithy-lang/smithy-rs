/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::endpoint::{
    error::ResolveEndpointError, EndpointFuture, EndpointResolverParams, ResolveEndpoint,
};
use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::{box_error::BoxError, client::endpoint::EndpointPrefix};
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::endpoint::Endpoint;
use http::header::HeaderName;
use http::uri::PathAndQuery;
use http::{HeaderValue, Uri};
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
    ctx: &mut InterceptorContext,
    runtime_components: &RuntimeComponents,
    cfg: &mut ConfigBag,
) -> Result<(), BoxError> {
    resolve_endpoint(runtime_components, cfg).await?;
    apply_endpoint(ctx, cfg)
}

pub(super) async fn resolve_endpoint(
    runtime_components: &RuntimeComponents,
    cfg: &mut ConfigBag,
) -> Result<(), BoxError> {
    trace!("resolving endpoint");

    let params = cfg
        .load::<EndpointResolverParams>()
        .expect("endpoint resolver params must be set");

    let endpoint = runtime_components
        .endpoint_resolver()
        .resolve_endpoint(params)
        .await?;

    // Make the endpoint config available to interceptors
    cfg.interceptor_state().store_put(endpoint);
    Ok(())
}

fn apply_endpoint(ctx: &mut InterceptorContext, cfg: &mut ConfigBag) -> Result<(), BoxError> {
    trace!("applying endpoint");

    let endpoint = cfg
        .load::<Endpoint>()
        .ok_or("`Endpoint` should have been resolved")?;
    tracing::debug!("will use endpoint {:?}", endpoint);

    let endpoint_prefix = cfg.load::<EndpointPrefix>();
    let request = ctx.request_mut().expect("set during serialization");

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
                "failed to apply endpoint `{}` to request `{:?}`",
                endpoint_url, request,
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
    use super::*;
    use aws_smithy_runtime_api::client::interceptors::context::Input;
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;

    #[test]
    fn test_apply_endpoint() {
        let mut req = HttpRequest::empty();
        req.set_uri("/foo?bar=1").unwrap();

        let mut ctx = InterceptorContext::new(Input::doesnt_matter());
        ctx.enter_serialization_phase();
        ctx.set_request(req);

        let endpoint = Endpoint::builder().url("https://s3.amazon.com").build();
        let prefix = EndpointPrefix::new("prefix.subdomain.").unwrap();
        let mut layer = Layer::new("test");
        layer.store_put(endpoint);
        layer.store_put(prefix);
        let mut cfg = ConfigBag::of_layers(vec![layer]);

        super::apply_endpoint(&mut ctx, &mut cfg).expect("should succeed");
        assert_eq!(
            ctx.request().unwrap().uri(),
            "https://prefix.subdomain.s3.amazon.com/foo?bar=1"
        );
    }
}
