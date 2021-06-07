/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::{AwsEndpoint, BoxError, CredentialScope, ResolveAwsEndpoint};
use aws_types::region::Region;
use smithy_http::endpoint::Endpoint;

pub type CompleteEndpoint = Definition;

/// Endpoint metadata
///
/// T & P exist to support optional vs. non optional values for Protocol and URI template during
/// merging
#[derive(Debug)]
pub struct Definition {
    /// URI for the endpoint.
    ///
    /// May contain `{region}` which will replaced with the region during endpoint construction
    pub uri_template: &'static str,

    /// Protocol to use for this endpoint
    pub protocol: Protocol,

    /// Credential scope to set for requests to this endpoint
    pub credential_scope: CredentialScope,

    /// Signature versions supported by this endpoint.
    ///
    /// Currently unused since the SDK only supports SigV4
    pub signature_versions: SignatureVersion,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum Protocol {
    Http,
    Https,
}

impl Protocol {
    fn as_str(&self) -> &'static str {
        match self {
            Protocol::Http => "http",
            Protocol::Https => "https",
        }
    }
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum SignatureVersion {
    V4,
}

impl ResolveAwsEndpoint for CompleteEndpoint {
    fn resolve_endpoint(&self, region: &Region) -> Result<AwsEndpoint, BoxError> {
        let uri = self.uri_template.replace("{region}", region.as_ref());
        let uri = format!("{}://{}", self.protocol.as_str(), uri);
        let endpoint = Endpoint::mutable(uri.parse()?);
        let ep = AwsEndpoint {
            endpoint,
            credential_scope: CredentialScope {
                service: self.credential_scope.service.clone(),
                region: self
                    .credential_scope
                    .region
                    .clone()
                    .or_else(|| Some(region.clone().into())),
            },
        };
        Ok(ep)
    }
}
