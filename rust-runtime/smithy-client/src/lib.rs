/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
//! A Hyper-based Smithy service client.
#![warn(
    missing_debug_implementations,
    missing_docs,
    rustdoc::all,
    rust_2018_idioms
)]

pub mod retry;

#[cfg(feature = "test-util")]
pub mod test_connection;

#[cfg(feature = "hyper")]
mod hyper_impls;

/// Type aliases for standard connection types.
#[cfg(feature = "hyper")]
#[allow(missing_docs)]
pub mod conns {
    #[cfg(feature = "rustls")]
    pub type Https = crate::hyper_impls::HyperAdapter<
        hyper_rustls::HttpsConnector<hyper::client::HttpConnector>,
    >;

    #[cfg(feature = "native-tls")]
    pub type NativeTls =
        crate::hyper_impls::HyperAdapter<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>;

    #[cfg(feature = "rustls")]
    pub type Rustls = crate::hyper_impls::HyperAdapter<
        hyper_rustls::HttpsConnector<hyper::client::HttpConnector>,
    >;
}

use smithy_http::body::SdkBody;
use smithy_http::operation::Operation;
use smithy_http::response::ParseHttpResponse;
pub use smithy_http::result::{SdkError, SdkSuccess};
use smithy_http::retry::ClassifyResponse;
use smithy_http_tower::dispatch::DispatchLayer;
use smithy_http_tower::parse_response::ParseResponseLayer;
use smithy_types::retry::ProvideErrorKind;
use std::error::Error;
use std::fmt;
use tower::{Layer, Service, ServiceBuilder, ServiceExt};

type BoxError = Box<dyn Error + Send + Sync>;

/// Smithy service client.
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
///
/// With the `hyper` feature enabled, you can construct a `Client` directly from a
/// [`hyper::Client`] using [`Builder::hyper`]. You can also enable the `rustls` or `native-tls`
/// features to construct a Client against a standard HTTPS endpoint using [`Builder::rustls`] and
/// [`Builder::native_tls`] respectively.
#[derive(Debug)]
pub struct Client<
    Connector = DynConnector,
    Middleware = DynMiddleware<Connector>,
    RetryPolicy = retry::Standard,
> {
    connector: Connector,
    middleware: Middleware,
    retry_policy: RetryPolicy,
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
    /// use smithy_client::Builder;
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

impl<C, M, R> Builder<C, M, R>
where
    C: bounds::SmithyConnector,
    M: bounds::SmithyMiddleware<DynConnector> + Send + 'static,
    R: retry::NewRequestPolicy,
{
    /// Build a type-erased Smithy service [`Client`].
    ///
    /// Note that if you're using the standard retry mechanism, [`retry::Standard`], `DynClient<R>`
    /// is equivalent to [`Client`] with no type arguments.
    ///
    /// ```rust
    /// # #[cfg(feature = "https")]
    /// # fn not_main() {
    /// use smithy_client::{Builder, Client};
    /// struct MyClient {
    ///     client: smithy_client::Client,
    /// }
    ///
    /// let client = Builder::new()
    ///     .https()
    ///     .middleware(tower::layer::util::Identity::new())
    ///     .build_dyn();
    /// let client = MyClient { client };
    /// # client.client.check();
    /// # }
    pub fn build_dyn(self) -> DynClient<R> {
        self.build().simplify()
    }
}

impl<C, M, R> Client<C, M, R>
where
    C: bounds::SmithyConnector,
    M: bounds::SmithyMiddleware<C>,
    R: retry::NewRequestPolicy,
{
    /// Dispatch this request to the network
    ///
    /// For ergonomics, this does not include the raw response for successful responses. To
    /// access the raw response use `call_raw`.
    pub async fn call<O, T, E, Retry>(&self, input: Operation<O, Retry>) -> Result<T, SdkError<E>>
    where
        R::Policy: bounds::SmithyRetryPolicy<O, T, E, Retry>,
        bounds::Parsed<<M as bounds::SmithyMiddleware<C>>::Service, O, Retry>:
            Service<Operation<O, Retry>, Response = SdkSuccess<T>, Error = SdkError<E>> + Clone,
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
        R::Policy: bounds::SmithyRetryPolicy<O, T, E, Retry>,
        // This bound is not _technically_ inferred by all the previous bounds, but in practice it
        // is because _we_ know that there is only implementation of Service for Parsed
        // (ParsedResponseService), and it will apply as long as the bounds on C, M, and R hold,
        // and will produce (as expected) Response = SdkSuccess<T>, Error = SdkError<E>. But Rust
        // doesn't know that -- there _could_ theoretically be other implementations of Service for
        // Parsed that don't return those same types. So, we must give the bound.
        bounds::Parsed<<M as bounds::SmithyMiddleware<C>>::Service, O, Retry>:
            Service<Operation<O, Retry>, Response = SdkSuccess<T>, Error = SdkError<E>> + Clone,
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

impl<C, M, R> Client<C, M, R>
where
    C: bounds::SmithyConnector,
    M: bounds::SmithyMiddleware<C> + Send + 'static,
    R: retry::NewRequestPolicy,
{
    /// Erase the middleware type from the client type signature.
    ///
    /// This makes the final client type easier to name, at the cost of a marginal increase in
    /// runtime performance. See [`DynMiddleware`] for details.
    ///
    /// In practice, you'll use this method once you've constructed a client to your liking:
    ///
    /// ```rust
    /// # #[cfg(feature = "https")]
    /// # fn not_main() {
    /// use smithy_client::{Builder, Client};
    /// struct MyClient {
    ///     client: Client<smithy_client::conns::Https>,
    /// }
    ///
    /// let client = Builder::new()
    ///     .https()
    ///     .middleware(tower::layer::util::Identity::new())
    ///     .build();
    /// let client = MyClient { client: client.erase_middleware() };
    /// # client.client.check();
    /// # }
    pub fn erase_middleware(self) -> Client<C, DynMiddleware<C>, R> {
        Client {
            connector: self.connector,
            middleware: DynMiddleware::new(self.middleware),
            retry_policy: self.retry_policy,
        }
    }
}

impl<C, M, R> Client<C, M, R>
where
    C: bounds::SmithyConnector,
    M: bounds::SmithyMiddleware<DynConnector> + Send + 'static,
    R: retry::NewRequestPolicy,
{
    /// Erase the connector type from the client type signature.
    ///
    /// This makes the final client type easier to name, at the cost of a marginal increase in
    /// runtime performance. See [`DynConnector`] for details.
    ///
    /// In practice, you'll use this method once you've constructed a client to your liking:
    ///
    /// ```rust
    /// # #[cfg(feature = "https")]
    /// # fn not_main() {
    /// # type MyMiddleware = smithy_client::DynMiddleware<smithy_client::DynConnector>;
    /// use smithy_client::{Builder, Client};
    /// struct MyClient {
    ///     client: Client<smithy_client::DynConnector, MyMiddleware>,
    /// }
    ///
    /// let client = Builder::new()
    ///     .https()
    ///     .middleware(tower::layer::util::Identity::new())
    ///     .build();
    /// let client = MyClient { client: client.erase_connector() };
    /// # client.client.check();
    /// # }
    pub fn erase_connector(self) -> Client<DynConnector, M, R> {
        Client {
            connector: DynConnector::new(self.connector),
            middleware: self.middleware,
            retry_policy: self.retry_policy,
        }
    }

    /// Erase the connector and middleware types from the client type signature.
    ///
    /// This makes the final client type easier to name, at the cost of a marginal increase in
    /// runtime performance. See [`DynConnector`] and [`DynMiddleware`] for details.
    ///
    /// Note that if you're using the standard retry mechanism, [`retry::Standard`], `DynClient<R>`
    /// is equivalent to `Client` with no type arguments.
    ///
    /// In practice, you'll use this method once you've constructed a client to your liking:
    ///
    /// ```rust
    /// # #[cfg(feature = "https")]
    /// # fn not_main() {
    /// use smithy_client::{Builder, Client};
    /// struct MyClient {
    ///     client: smithy_client::Client,
    /// }
    ///
    /// let client = Builder::new()
    ///     .https()
    ///     .middleware(tower::layer::util::Identity::new())
    ///     .build();
    /// let client = MyClient { client: client.simplify() };
    /// # client.client.check();
    /// # }
    pub fn simplify(self) -> DynClient<R> {
        self.erase_connector().erase_middleware()
    }
}

// These types are technically public in that they're reachable from the public trait impls on
// DynMiddleware, but no-one should ever look at them or use them.
#[doc(hidden)]
pub mod erase;

/// A [`Client`] whose connector and middleware types have been erased.
///
/// Mainly useful if you need to name `R` in a type-erased client. If you do not, you can instead
/// just use `Client` with no type parameters, which ends up being the same type.
pub type DynClient<R = retry::Standard> = Client<DynConnector, DynMiddleware<DynConnector>, R>;

/// A Smithy connector that uses dynamic dispatch.
///
/// This type allows you to pay a small runtime cost to avoid having to name the exact connector
/// you're using anywhere you want to hold a [`Client`]. Specifically, this will use `Box` to
/// enable dynamic dispatch for every request that goes through the connector, which increases
/// memory pressure and suffers an additional vtable indirection for each request, but is unlikely
/// to matter in all but the highest-performance settings.
#[non_exhaustive]
#[derive(Clone)]
pub struct DynConnector(
    erase::BoxCloneService<http::Request<SdkBody>, http::Response<SdkBody>, BoxError>,
);

impl fmt::Debug for DynConnector {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("DynConnector").finish()
    }
}

impl DynConnector {
    /// Construct a new dynamically-dispatched Smithy middleware.
    pub fn new<E, C>(connector: C) -> Self
    where
        C: bounds::SmithyConnector<Error = E> + Send + 'static,
        E: Into<BoxError>,
    {
        Self(erase::BoxCloneService::new(connector.map_err(|e| e.into())))
    }
}

impl Service<http::Request<SdkBody>> for DynConnector {
    type Response = http::Response<SdkBody>;
    type Error = BoxError;
    type Future = erase::BoxFuture<Self::Response, Self::Error>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<SdkBody>) -> Self::Future {
        self.0.call(req)
    }
}

/// A Smithy middleware that uses dynamic dispatch.
///
/// This type allows you to pay a small runtime cost to avoid having to name the exact middleware
/// you're using anywhere you want to hold a [`Client`]. Specifically, this will use `Box` to
/// enable dynamic dispatch for every request that goes through the middleware, which increases
/// memory pressure and suffers an additional vtable indirection for each request, but is unlikely
/// to matter in all but the highest-performance settings.
#[non_exhaustive]
pub struct DynMiddleware<C>(
    erase::BoxCloneLayer<
        smithy_http_tower::dispatch::DispatchService<C>,
        smithy_http::operation::Request,
        http::Response<SdkBody>,
        smithy_http_tower::SendOperationError,
    >,
);

impl<C> fmt::Debug for DynMiddleware<C> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("DynMiddleware").finish()
    }
}

impl<C> DynMiddleware<C> {
    /// Construct a new dynamically-dispatched Smithy middleware.
    pub fn new<M: bounds::SmithyMiddleware<C> + Send + 'static>(middleware: M) -> Self {
        Self(erase::BoxCloneLayer::new(middleware))
    }
}

impl<C> Layer<smithy_http_tower::dispatch::DispatchService<C>> for DynMiddleware<C> {
    type Service = erase::BoxCloneService<
        smithy_http::operation::Request,
        http::Response<SdkBody>,
        smithy_http_tower::SendOperationError,
    >;

    fn layer(&self, inner: smithy_http_tower::dispatch::DispatchService<C>) -> Self::Service {
        self.0.layer(inner)
    }
}

/// This module holds convenient short-hands for the otherwise fairly extensive trait bounds
/// required for `call` and friends.
///
/// The short-hands will one day be true [trait aliases], but for now they are traits with blanket
/// implementations. Also, due to [compiler limitations], the bounds repeat a nubmer of associated
/// types with bounds so that those bounds [do not need to be repeated] at the call site. It's a
/// bit of a mess to define, but _should_ be invisible to callers.
///
/// [trait aliases]: https://rust-lang.github.io/rfcs/1733-trait-alias.html
/// [compiler limitations]: https://github.com/rust-lang/rust/issues/20671
/// [do not need to be repeated]: https://github.com/rust-lang/rust/issues/20671#issuecomment-529752828
pub mod bounds {
    use super::*;

    /// A service that has parsed a raw Smithy response.
    pub type Parsed<S, O, Retry> =
        smithy_http_tower::parse_response::ParseResponseService<S, O, Retry>;

    /// A low-level Smithy connector that maps from [`http::Request`] to [`http::Response`].
    ///
    /// This trait has a blanket implementation for all compatible types, and should never need to
    /// be implemented.
    pub trait SmithyConnector:
        Service<
            http::Request<SdkBody>,
            Response = http::Response<SdkBody>,
            Error = <Self as SmithyConnector>::Error,
            Future = <Self as SmithyConnector>::Future,
        > + Send
        + Clone
        + 'static
    {
        /// Forwarding type to `<Self as Service>::Error` for bound inference.
        ///
        /// See module-level docs for details.
        type Error: Into<BoxError> + Send + Sync + 'static;

        /// Forwarding type to `<Self as Service>::Future` for bound inference.
        ///
        /// See module-level docs for details.
        type Future: Send + 'static;
    }

    impl<T> SmithyConnector for T
    where
        T: Service<http::Request<SdkBody>, Response = http::Response<SdkBody>>
            + Send
            + Clone
            + 'static,
        T::Error: Into<BoxError> + Send + Sync + 'static,
        T::Future: Send + 'static,
    {
        type Error = T::Error;
        type Future = T::Future;
    }

    /// A Smithy middleware service that adjusts [`smithy_http::operation::Request`]s.
    ///
    /// This trait has a blanket implementation for all compatible types, and should never need to
    /// be implemented.
    pub trait SmithyMiddlewareService:
        Service<
        smithy_http::operation::Request,
        Response = http::Response<SdkBody>,
        Error = smithy_http_tower::SendOperationError,
        Future = <Self as SmithyMiddlewareService>::Future,
    >
    {
        /// Forwarding type to `<Self as Service>::Future` for bound inference.
        ///
        /// See module-level docs for details.
        type Future: Send + 'static;
    }

    impl<T> SmithyMiddlewareService for T
    where
        T: Service<
            smithy_http::operation::Request,
            Response = http::Response<SdkBody>,
            Error = smithy_http_tower::SendOperationError,
        >,
        T::Future: Send + 'static,
    {
        type Future = T::Future;
    }

    /// A Smithy middleware layer (i.e., factory).
    ///
    /// This trait has a blanket implementation for all compatible types, and should never need to
    /// be implemented.
    pub trait SmithyMiddleware<C>:
        Layer<
        smithy_http_tower::dispatch::DispatchService<C>,
        Service = <Self as SmithyMiddleware<C>>::Service,
    >
    {
        /// Forwarding type to `<Self as Layer>::Service` for bound inference.
        ///
        /// See module-level docs for details.
        type Service: SmithyMiddlewareService + Send + Clone + 'static;
    }

    impl<T, C> SmithyMiddleware<C> for T
    where
        T: Layer<smithy_http_tower::dispatch::DispatchService<C>>,
        T::Service: SmithyMiddlewareService + Send + Clone + 'static,
    {
        type Service = T::Service;
    }

    /// A Smithy retry policy.
    ///
    /// This trait has a blanket implementation for all compatible types, and should never need to
    /// be implemented.
    pub trait SmithyRetryPolicy<O, T, E, Retry>:
        tower::retry::Policy<Operation<O, Retry>, SdkSuccess<T>, SdkError<E>> + Clone
    {
        /// Forwarding type to `O` for bound inference.
        ///
        /// See module-level docs for details.
        type O: ParseHttpResponse<SdkBody, Output = Result<T, Self::E>>
            + Send
            + Sync
            + Clone
            + 'static;
        /// Forwarding type to `E` for bound inference.
        ///
        /// See module-level docs for details.
        type E: Error + ProvideErrorKind;

        /// Forwarding type to `Retry` for bound inference.
        ///
        /// See module-level docs for details.
        type Retry: ClassifyResponse<SdkSuccess<T>, SdkError<Self::E>>;
    }

    impl<R, O, T, E, Retry> SmithyRetryPolicy<O, T, E, Retry> for R
    where
        R: tower::retry::Policy<Operation<O, Retry>, SdkSuccess<T>, SdkError<E>> + Clone,
        O: ParseHttpResponse<SdkBody, Output = Result<T, E>> + Send + Sync + Clone + 'static,
        E: Error + ProvideErrorKind,
        Retry: ClassifyResponse<SdkSuccess<T>, SdkError<E>>,
    {
        type O = O;
        type E = E;
        type Retry = Retry;
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
    #[cfg(all(test, feature = "hyper"))]
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

    // Statically check that a type-erased middleware client is actually a valid Client.
    #[allow(dead_code)]
    fn sanity_erase_middleware() {
        Builder::new()
            .middleware(tower::layer::util::Identity::new())
            .map_connector(|_| async { unreachable!() })
            .build()
            .erase_middleware()
            .check();
    }

    // Statically check that a type-erased connector client is actually a valid Client.
    #[allow(dead_code)]
    fn sanity_erase_connector() {
        Builder::new()
            .middleware(tower::layer::util::Identity::new())
            .map_connector(|_| async { unreachable!() })
            .build()
            .erase_connector()
            .check();
    }
}
