/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pub use aws_smithy_client::retry::Config as RetryConfig;

use aws_endpoint::AwsEndpointStage;
use aws_http::auth::CredentialsStage;
use aws_http::user_agent::UserAgentStage;
use aws_sig_auth::middleware::SigV4SigningStage;
use aws_sig_auth::signer::SigV4Signer;
use aws_smithy_http_tower::map_request::{AsyncMapRequestLayer, MapRequestLayer};
use std::fmt::Debug;
use tower::layer::util::Stack;
use tower::ServiceBuilder;

type AwsMiddlewareStack = Stack<
    MapRequestLayer<SigV4SigningStage>,
    Stack<
        AsyncMapRequestLayer<CredentialsStage>,
        Stack<MapRequestLayer<UserAgentStage>, MapRequestLayer<AwsEndpointStage>>,
    >,
>;

/// AWS Middleware Stack
///
/// This implements the default middleware stack used with AWS Services
/// # Examples
/// **Construct a Smithy Client with HTTPS and the AWS Middleware stack**:
/// ```no_run
/// use aws_hyper::AwsMiddleware;
/// use aws_smithy_client::erase::DynConnector;
/// let client = aws_smithy_client::Builder::<DynConnector, AwsMiddleware>::dyn_https()
///     .default_async_sleep()
///     .build();
/// ```
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct AwsMiddleware;

impl AwsMiddleware {
    pub fn new() -> Self {
        AwsMiddleware::default()
    }
}
impl<S> tower::Layer<S> for AwsMiddleware {
    type Service = <AwsMiddlewareStack as tower::Layer<S>>::Service;

    fn layer(&self, inner: S) -> Self::Service {
        let credential_provider = AsyncMapRequestLayer::for_mapper(CredentialsStage::new());
        let signer = MapRequestLayer::for_mapper(SigV4SigningStage::new(SigV4Signer::new()));
        let endpoint_resolver = MapRequestLayer::for_mapper(AwsEndpointStage);
        let user_agent = MapRequestLayer::for_mapper(UserAgentStage::new());
        // These layers can be considered as occurring in order, that is:
        // 1. Resolve an endpoint
        // 2. Add a user agent
        // 3. Acquire credentials
        // 4. Sign with credentials
        // (5. Dispatch over the wire)
        ServiceBuilder::new()
            .layer(endpoint_resolver)
            .layer(user_agent)
            .layer(credential_provider)
            .layer(signer)
            .service(inner)
    }
}
