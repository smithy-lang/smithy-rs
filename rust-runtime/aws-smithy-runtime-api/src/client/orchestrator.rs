/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use self::builders::IdentityResolverConfigBuilder;
use super::identity::Identity;
use super::interceptors::context::{Input, OutputOrError};
use super::interceptors::InterceptorContext;
use crate::config_bag::ConfigBag;
use crate::type_erasure::TypeErasedBox;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::property_bag::PropertyBag;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type HttpRequest = ::http::Request<SdkBody>;
pub type HttpResponse = ::http::Response<SdkBody>;
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type BoxFallibleFut<T> = Pin<Box<dyn Future<Output = Result<T, BoxError>>>>;

pub trait TraceProbe: Send + Sync + Debug {
    fn dispatch_events(&self, cfg: &ConfigBag) -> BoxFallibleFut<()>;
}

pub trait RequestSerializer: Send + Sync + Debug {
    fn serialize_input(&self, input: &Input, cfg: &ConfigBag) -> Result<HttpRequest, BoxError>;
}

pub trait ResponseDeserializer: Send + Sync + Debug {
    fn deserialize_streaming(&self, response: &mut HttpResponse) -> Option<OutputOrError> {
        let _ = response;
        None
    }

    fn deserialize_nonstreaming(&self, response: &HttpResponse) -> OutputOrError;
}

pub trait Connection: Send + Sync + Debug {
    fn call(&self, request: &mut HttpRequest, cfg: &ConfigBag) -> BoxFallibleFut<HttpResponse>;
}

pub trait RetryStrategy: Send + Sync + Debug {
    fn should_attempt_initial_request(&self, cfg: &ConfigBag) -> Result<(), BoxError>;

    fn should_attempt_retry(
        &self,
        context: &InterceptorContext<HttpRequest, HttpResponse>,
        cfg: &ConfigBag,
    ) -> Result<bool, BoxError>;
}

pub type AuthOptionResolverParams = TypeErasedBox;

pub trait AuthOptionResolver: Send + Sync + Debug {
    fn resolve_auth_options(
        &self,
        params: &AuthOptionResolverParams,
    ) -> Result<Vec<HttpAuthOption>, BoxError>;
}

#[derive(Clone, Debug)]
pub struct HttpAuthOption {
    scheme_id: &'static str,
    identity_properties: Arc<PropertyBag>,
    signer_properties: Arc<PropertyBag>,
}

impl HttpAuthOption {
    pub fn new(
        scheme_id: &'static str,
        identity_properties: Arc<PropertyBag>,
        signer_properties: Arc<PropertyBag>,
    ) -> Self {
        Self {
            scheme_id,
            identity_properties,
            signer_properties,
        }
    }

    pub fn scheme_id(&self) -> &'static str {
        self.scheme_id
    }

    pub fn identity_properties(&self) -> &PropertyBag {
        &*self.identity_properties
    }

    pub fn signer_properties(&self) -> &PropertyBag {
        &*self.signer_properties
    }
}

pub trait IdentityResolver: Send + Sync + Debug {
    fn resolve_identity(&self, cfg: &ConfigBag) -> Result<Identity, BoxError>;
}

pub struct IdentityResolverConfig {
    identity_resolvers: Vec<(&'static str, Box<dyn IdentityResolver>)>,
}

impl IdentityResolverConfig {
    pub fn builder() -> IdentityResolverConfigBuilder {
        IdentityResolverConfigBuilder::new()
    }

    pub fn get_identity_resolver(
        &self,
        identity_type: &'static str,
    ) -> Option<&dyn IdentityResolver> {
        self.identity_resolvers
            .iter()
            .find(|resolver| resolver.0 == identity_type)
            .map(|resolver| &*resolver.1)
    }
}

pub trait HttpAuthScheme: Send + Sync + Debug {
    fn scheme_id(&self) -> &'static str;

    fn identity_resolver(
        &self,
        identity_resolver_config: &IdentityResolverConfig,
    ) -> &dyn IdentityResolver;

    fn request_signer(&self) -> &dyn HttpRequestSigner;
}

pub trait HttpRequestSigner: Send + Sync + Debug {
    /// Return a signed version of the given request using the given identity.
    ///
    /// If the provided identity is incompatible with this signer, an error must be returned.
    fn sign_request(
        &self,
        request: &HttpRequest,
        identity: &Identity,
        cfg: &ConfigBag,
    ) -> Result<HttpRequest, BoxError>;
}

pub trait EndpointResolver: Send + Sync + Debug {
    fn resolve_and_apply_endpoint(
        &self,
        request: &mut HttpRequest,
        cfg: &ConfigBag,
    ) -> Result<(), BoxError>;

    fn resolve_auth_schemes(&self) -> Result<Vec<String>, BoxError>;
}

pub trait ConfigBagAccessors {
    fn retry_strategy(&self) -> &dyn RetryStrategy;
    fn set_retry_strategy(&mut self, retry_strategy: impl RetryStrategy + 'static);

    fn endpoint_resolver(&self) -> &dyn EndpointResolver;
    fn set_endpoint_resolver(&mut self, endpoint_resolver: impl EndpointResolver + 'static);

    fn connection(&self) -> &dyn Connection;
    fn set_connection(&mut self, connection: impl Connection + 'static);

    fn request_serializer(&self) -> &dyn RequestSerializer;
    fn set_request_serializer(&mut self, request_serializer: impl RequestSerializer + 'static);

    fn response_deserializer(&self) -> &dyn ResponseDeserializer;
    fn set_response_deserializer(
        &mut self,
        response_serializer: impl ResponseDeserializer + 'static,
    );

    fn trace_probe(&self) -> &dyn TraceProbe;
    fn set_trace_probe(&mut self, trace_probe: impl TraceProbe + 'static);
}

impl ConfigBagAccessors for ConfigBag {
    fn retry_strategy(&self) -> &dyn RetryStrategy {
        &**self
            .get::<Box<dyn RetryStrategy>>()
            .expect("a retry strategy must be set")
    }

    fn set_retry_strategy(&mut self, retry_strategy: impl RetryStrategy + 'static) {
        self.put::<Box<dyn RetryStrategy>>(Box::new(retry_strategy));
    }

    fn endpoint_resolver(&self) -> &dyn EndpointResolver {
        &**self
            .get::<Box<dyn EndpointResolver>>()
            .expect("an endpoint resolver must be set")
    }

    fn set_endpoint_resolver(&mut self, endpoint_resolver: impl EndpointResolver + 'static) {
        self.put::<Box<dyn EndpointResolver>>(Box::new(endpoint_resolver));
    }

    fn connection(&self) -> &dyn Connection {
        &**self
            .get::<Box<dyn Connection>>()
            .expect("missing connector")
    }

    fn set_connection(&mut self, connection: impl Connection + 'static) {
        self.put::<Box<dyn Connection>>(Box::new(connection));
    }

    fn request_serializer(&self) -> &dyn RequestSerializer {
        &**self
            .get::<Box<dyn RequestSerializer>>()
            .expect("missing request serializer")
    }

    fn set_request_serializer(&mut self, request_serializer: impl RequestSerializer + 'static) {
        self.put::<Box<dyn RequestSerializer>>(Box::new(request_serializer));
    }

    fn response_deserializer(&self) -> &dyn ResponseDeserializer {
        &**self
            .get::<Box<dyn ResponseDeserializer>>()
            .expect("missing response deserializer")
    }

    fn set_response_deserializer(
        &mut self,
        response_deserializer: impl ResponseDeserializer + 'static,
    ) {
        self.put::<Box<dyn ResponseDeserializer>>(Box::new(response_deserializer));
    }

    fn trace_probe(&self) -> &dyn TraceProbe {
        &**self
            .get::<Box<dyn TraceProbe>>()
            .expect("missing trace probe")
    }

    fn set_trace_probe(&mut self, trace_probe: impl TraceProbe + 'static) {
        self.put::<Box<dyn TraceProbe>>(Box::new(trace_probe));
    }
}

pub mod builders {
    use super::*;

    #[derive(Debug, Default)]
    pub struct IdentityResolverConfigBuilder {
        identity_resolvers: Vec<(&'static str, Box<dyn IdentityResolver>)>,
    }

    impl IdentityResolverConfigBuilder {
        pub fn new() -> Self {
            Default::default()
        }

        pub fn identity_resolver(
            mut self,
            name: &'static str,
            resolver: impl IdentityResolver + 'static,
        ) -> Self {
            self.identity_resolvers
                .push((name, Box::new(resolver) as _));
            self
        }

        pub fn build(self) -> IdentityResolverConfig {
            IdentityResolverConfig {
                identity_resolvers: self.identity_resolvers,
            }
        }
    }
}
