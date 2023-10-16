/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

impl<Params> ResolveEndpoint for DefaultEndpointResolver<Params>
where
    Params: Debug + Send + Sync + 'static,
{
    fn resolve_endpoint<'a>(&'a self, params: &'a EndpointResolverParams) -> EndpointFuture<'a> {
        use aws_smithy_http::endpoint::ResolveEndpoint as _;
        let ep = match params.get::<Params>() {
            Some(params) => self.inner.resolve_endpoint(params).map_err(Box::new),
            None => Err(Box::new(ResolveEndpointError::message(
                "params of expected type was not present",
            ))),
        }
        .map_err(|e| e as _);
        EndpointFuture::ready(ep)
    }
}
