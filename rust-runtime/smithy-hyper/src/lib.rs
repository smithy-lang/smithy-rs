/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
//! A Hyper-based Smithy service client.
#![warn(missing_debug_implementations, missing_docs, rustdoc::all)]

pub mod retry;

#[cfg(feature = "test-util")]
pub mod test_connection;

mod hyper_impls;
use hyper_impls::HyperAdapter;

use smithy_http::body::SdkBody;
use smithy_http::operation::Operation;
use smithy_http::response::ParseHttpResponse;
pub use smithy_http::result::{SdkError, SdkSuccess};
use smithy_http::retry::ClassifyResponse;
use smithy_http_tower::dispatch::DispatchLayer;
use smithy_http_tower::parse_response::ParseResponseLayer;
use smithy_types::retry::ProvideErrorKind;
use std::error::Error;
use std::fmt::Debug;
use tower::{Layer, Service, ServiceBuilder, ServiceExt};

type BoxError = Box<dyn Error + Send + Sync>;

/// Hyper-based Smithy Service Client.
///
/// The service client is customizeable in a number of ways (see [`Builder`]), but most customers
/// can stick with the standard constructor provided by [`Client::new`]. It takes only a single
/// argument, which is the middleware that fills out the [`http::Request`] for each higher-level
/// operation so that it can ultimately be sent to the remote host. The middleware is responsible
/// for filling in any request parameters that aren't specified by the Smithy protocol definition,
/// such as those used for routing (like the URL), authentication, and authorization.
///
/// The middleware takes the form of a [`tower::Layer`] that wraps the actual connection for each
/// request. The [`tower::Service`] that the middleware produces must accept requests of the type
/// [`smithy_http::operation::Request`] and return responses of the type
/// [`http::Response<SdkBody>`], most likely by modifying the provided request in place, passing it
/// to the inner service, and then ultimately returning the inner service's response.
#[derive(Debug)]
pub struct Client<Connector, Middleware, RetryPolicy = retry::Standard> {
    connector: Connector,
    middleware: Middleware,
    retry_policy: RetryPolicy,
}

impl<M> Client<HyperAdapter<hyper_rustls::HttpsConnector<hyper::client::HttpConnector>>, M> {
    /// Create a Smithy client that uses HTTPS and the [standard retry policy](retry::Standard).
    pub fn new(middleware: M) -> Self {
        Builder::new().https().middleware(middleware).build()
    }
}

impl<C, M> Client<C, M> {
    /// Set the standard retry policy's configuration.
    pub fn set_retry_config(&mut self, config: retry::Config) {
        self.retry_policy.with_config(config);
    }
}

/// A builder that provides more customization options when constructing a [`Client`].
///
/// To start, call [`Builder::new`]. Then, chain the method calls to configure the `Builder`.
/// When configured to your liking, call [`Builder::build`]. The individual methods have additional
/// documentation.
#[derive(Clone, Debug)]
pub struct Builder<C, M, R = retry::Standard> {
    connector: C,
    middleware: M,
    retry_policy: R,
}

impl Default for Builder<(), ()> {
    fn default() -> Self {
        Builder {
            connector: (),
            middleware: (),
            retry_policy: retry::Standard::default(),
        }
    }
}

impl Builder<(), ()> {
    /// Construct a new, unconfigured builder.
    ///
    /// This builder cannot yet be used, as it does not specify a [connector](Builder::connector)
    /// or [middleware](Builder::middleware). It uses the [standard retry
    /// mechanism](retry::Standard).
    pub fn new() -> Self {
        Self::default()
    }
}

impl<C, M, R> Builder<C, M, R> {
    /// Specify the connector for the eventual client to use.
    ///
    /// The connector dictates how requests are turned into responses. Normally, this would entail
    /// sending the request to some kind of remote server, but in certain settings it's useful to
    /// be able to use a custom connector instead, such as to mock the network for tests.
    ///
    /// If you want to use a custom hyper connector, use [`Builder::hyper`].
    ///
    /// If you just want to specify a function from request to response instead, use
    /// [`Builder::map_connector`].
    pub fn connector<C2>(self, connector: C2) -> Builder<C2, M, R> {
        Builder {
            connector,
            retry_policy: self.retry_policy,
            middleware: self.middleware,
        }
    }

    /// Specify the middleware for the eventual client ot use.
    ///
    /// The middleware adjusts requests before they are dispatched to the connector. It is
    /// responsible for filling in any request parameters that aren't specified by the Smithy
    /// protocol definition, such as those used for routing (like the URL), authentication, and
    /// authorization.
    ///
    /// The middleware takes the form of a [`tower::Layer`] that wraps the actual connection for
    /// each request. The [`tower::Service`] that the middleware produces must accept requests of
    /// the type [`smithy_http::operation::Request`] and return responses of the type
    /// [`http::Response<SdkBody>`], most likely by modifying the provided request in place,
    /// passing it to the inner service, and then ultimately returning the inner service's
    /// response.
    ///
    /// If your requests are already ready to be sent and need no adjustment, you can use
    /// [`tower::layer::util::Identity`] as your middleware.
    pub fn middleware<M2>(self, middleware: M2) -> Builder<C, M2, R> {
        Builder {
            connector: self.connector,
            retry_policy: self.retry_policy,
            middleware,
        }
    }

    /// Specify the retry policy for the eventual client to use.
    ///
    /// By default, the Smithy client uses a standard retry policy that works well in most
    /// settings. You can use this method to override that policy with a custom one. A new policy
    /// instance will be instantiated for each request using [`retry::NewRequestPolicy`]. Each
    /// policy instance must implement [`tower::retry::Policy`].
    ///
    /// If you just want to modify the policy _configuration_ for the standard retry policy, use
    /// [`Builder::set_retry_config`].
    pub fn retry_policy<R2>(self, retry_policy: R2) -> Builder<C, M, R2> {
        Builder {
            connector: self.connector,
            retry_policy,
            middleware: self.middleware,
        }
    }
}

impl<C, M> Builder<C, M> {
    /// Set the standard retry policy's configuration.
    pub fn set_retry_config(&mut self, config: retry::Config) {
        self.retry_policy.with_config(config);
    }
}

impl<C, M, R> Builder<C, M, R> {
    /// Use a connector that directly maps each request to a response.
    ///
    /// ```rust
    /// use smithy_hyper::Builder;
    /// use smithy_http::body::SdkBody;
    /// let client = Builder::new()
    /// # /*
    ///   .middleware(..)
    /// # */
    /// # .middleware(tower::layer::util::Identity::new())
    ///   .map_connector(|req: http::Request<SdkBody>| {
    ///     async move {
    ///       Ok(http::Response::new(SdkBody::empty()))
    ///     }
    ///   })
    ///   .build();
    /// # client.check();
    /// ```
    pub fn map_connector<F, FF>(self, map: F) -> Builder<tower::util::ServiceFn<F>, M, R>
    where
        F: Fn(http::Request<SdkBody>) -> FF + Send,
        FF: std::future::Future<Output = Result<http::Response<SdkBody>, BoxError>>,
    {
        self.connector(tower::service_fn(map))
    }
}

impl<C, M, R> Builder<C, M, R> {
    /// Build a Smithy service [`Client`].
    pub fn build(self) -> Client<C, M, R> {
        Client {
            connector: self.connector,
            retry_policy: self.retry_policy,
            middleware: self.middleware,
        }
    }
}

impl<C, M, R> Client<C, M, R>
where
    C: Service<http::Request<SdkBody>, Response = http::Response<SdkBody>> + Send + Clone + 'static,
    C::Error: Into<BoxError> + Send + Sync + 'static,
    C::Future: Send + 'static,
    R: retry::NewRequestPolicy,
    M: Layer<smithy_http_tower::dispatch::DispatchService<C>>,
    M::Service: Service<
            smithy_http::operation::Request,
            Response = http::Response<SdkBody>,
            Error = smithy_http_tower::SendOperationError,
        > + Send
        + Clone
        + 'static,
    <M::Service as Service<smithy_http::operation::Request>>::Future: Send + 'static,
{
    /// Dispatch this request to the network
    ///
    /// For ergonomics, this does not include the raw response for successful responses. To
    /// access the raw response use `call_raw`.
    pub async fn call<O, T, E, Retry>(&self, input: Operation<O, Retry>) -> Result<T, SdkError<E>>
    where
        O: ParseHttpResponse<SdkBody, Output = Result<T, E>> + Send + Sync + Clone + 'static,
        E: Error + ProvideErrorKind,
        R::Policy: tower::retry::Policy<Operation<O, Retry>, SdkSuccess<T>, SdkError<E>> + Clone,
        Retry: ClassifyResponse<SdkSuccess<T>, SdkError<E>>,
    {
        self.call_raw(input).await.map(|res| res.parsed)
    }

    /// Dispatch this request to the network
    ///
    /// The returned result contains the raw HTTP response which can be useful for debugging or
    /// implementing unsupported features.
    pub async fn call_raw<O, T, E, Retry>(
        &self,
        input: Operation<O, Retry>,
    ) -> Result<SdkSuccess<T>, SdkError<E>>
    where
        O: ParseHttpResponse<SdkBody, Output = Result<T, E>> + Send + Sync + Clone + 'static,
        E: Error + ProvideErrorKind,
        R::Policy: tower::retry::Policy<Operation<O, Retry>, SdkSuccess<T>, SdkError<E>> + Clone,
        Retry: ClassifyResponse<SdkSuccess<T>, SdkError<E>>,
    {
        let connector = self.connector.clone();
        let mut svc = ServiceBuilder::new()
            // Create a new request-scoped policy
            .retry(self.retry_policy.new_request_policy())
            .layer(ParseResponseLayer::<O, Retry>::new())
            // These layers can be considered as occuring in order. That is, first invoke the
            // customer-provided middleware, then dispatch dispatch over the wire.
            .layer(&self.middleware)
            .layer(DispatchLayer::new())
            .check_service()
            .service(connector);
        svc.ready().await?.call(input).await
    }

    /// Statically check the validity of a `Client` without a request to send.
    ///
    /// This will make sure that all the bounds hold that would be required by `call` and
    /// `call_raw` (modulo those that relate to the specific `Operation` type). Comes in handy to
    /// ensure (statically) that all the various constructors actually produce "useful" types.
    #[doc(hidden)]
    pub fn check(&self)
    where
        R::Policy: tower::retry::Policy<
                static_tests::ValidTestOperation,
                SdkSuccess<()>,
                SdkError<static_tests::TestOperationError>,
            > + Clone,
    {
        let _ = |o: static_tests::ValidTestOperation| {
            let _ = self.call_raw(o);
        };
    }
}

/// This module provides types useful for static tests.
///
/// These are only used to write the bounds in [`Client::check`]. Customers will not need them.
/// But the module and its types must be public so that we can call `check` from doc-tests.
#[doc(hidden)]
#[allow(missing_docs, missing_debug_implementations)]
pub mod static_tests {
    use super::*;

    #[derive(Debug)]
    #[non_exhaustive]
    pub struct TestOperationError;
    impl std::fmt::Display for TestOperationError {
        fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            unreachable!("only used for static tests")
        }
    }
    impl Error for TestOperationError {}
    impl ProvideErrorKind for TestOperationError {
        fn retryable_error_kind(&self) -> Option<smithy_types::retry::ErrorKind> {
            unreachable!("only used for static tests")
        }

        fn code(&self) -> Option<&str> {
            unreachable!("only used for static tests")
        }
    }
    #[derive(Clone)]
    #[non_exhaustive]
    pub struct TestOperation;
    impl ParseHttpResponse<SdkBody> for TestOperation {
        type Output = Result<(), TestOperationError>;

        fn parse_unloaded(&self, _: &mut http::Response<SdkBody>) -> Option<Self::Output> {
            unreachable!("only used for static tests")
        }

        fn parse_loaded(&self, _response: &http::Response<bytes::Bytes>) -> Self::Output {
            unreachable!("only used for static tests")
        }
    }
    pub type ValidTestOperation = Operation<TestOperation, ()>;

    // Statically check that a standard retry can actually be used to build a Client.
    #[allow(dead_code)]
    #[cfg(test)]
    fn sanity_retry() {
        Builder::new()
            .middleware(tower::layer::util::Identity::new())
            .map_connector(|_| async { unreachable!() })
            .build()
            .check();
    }

    // Statically check that a hyper client can actually be used to build a Client.
    #[allow(dead_code)]
    #[cfg(test)]
    fn sanity_hyper<C>(hc: hyper::Client<C, SdkBody>)
    where
        C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
    {
        Builder::new()
            .middleware(tower::layer::util::Identity::new())
            .hyper(hc)
            .build()
            .check();
    }
}
