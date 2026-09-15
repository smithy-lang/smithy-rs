/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime construction and dispatch for one selected schema protocol.

use super::Route;
use crate::{
    body::{Body, BodyLimitExceeded, BoxBody},
    error::{BoxError, Error},
    schema::{
        ProtocolRegistration, ProtocolRegistry, RequestBodyCollectionConfig, RequestBodyCollectionError,
        SelectedProtocolOperation, ServiceRequestBodyConfig, SharedServerProtocol,
    },
};
use aws_smithy_schema::{OperationSchema, ServiceSchema};
use bytes::{Bytes, BytesMut};
use http::{Request, Response};

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::time::Instant;
use tower::{util::Either, Service};

/// A canonical operation schema and its position in the handler array.
#[derive(Clone, Copy, Debug)]
pub struct OperationIndex {
    index: usize,
    operation: &'static OperationSchema<'static>,
}
impl OperationIndex {
    /// Returns the assigned handler position.
    pub fn index(self) -> usize {
        self.index
    }
    /// Returns the operation's canonical schema.
    pub fn operation(self) -> &'static OperationSchema<'static> {
        self.operation
    }
}

/// A protocol-independent operation and its HTTP handler.
pub struct OperationHandlerBinding<B = Body> {
    operation: &'static OperationSchema<'static>,
    route: Route<B>,
}
impl<B> OperationHandlerBinding<B> {
    /// Binds an operation to a handler, without assigning any protocol-specific routing rule.
    pub fn new(operation: &'static OperationSchema<'static>, route: Route<B>) -> Self {
        Self { operation, route }
    }
}
impl<B> Clone for OperationHandlerBinding<B> {
    fn clone(&self) -> Self {
        Self::new(self.operation, self.route.clone())
    }
}
impl<B> fmt::Debug for OperationHandlerBinding<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationHandlerBinding")
            .field("operation", &self.operation.shape_id())
            .finish()
    }
}

/// Construction options, independent of any generated router implementation.
#[derive(Clone, Debug, Default)]
pub struct SchemaRoutingOptions {
    /// Global and per-operation body-read allowances. Operation entries replace the whole record.
    pub request_body: ServiceRequestBodyConfig,
    /// Preserve the existing RPC v2 CBOR capitalized operation aliases.
    pub rpc_v2_cbor_add_capitalized_route: bool,
    /// Compatibility names emitted by codegen, keyed by absolute operation shape ID.
    /// Protocols that use modeled names can ignore these overrides.
    pub operation_names: HashMap<String, String>,
}

/// Invalid schema, bindings, or protocol-specific routing configuration.
#[derive(Debug, thiserror::Error)]
pub enum RouterBuildError {
    #[error("schema routing requires exactly one protocol, found {0}")]
    ProtocolCount(usize),
    #[error("no protocol registration recognizes the service schema")]
    UnknownProtocol,
    #[error("invalid operation binding: {0}")]
    Binding(String),
    #[error("invalid routing configuration: {0}")]
    Configuration(String),
    #[error("protocol {protocol} requires body routing, which cannot support streaming operation {operation}")]
    StreamingBodyRouting { protocol: String, operation: String },
    #[error("protocol could not build its router: {0}")]
    Protocol(#[source] BoxError),
}

/// Selects an operation within an already-selected protocol. All rejections are terminal.
pub trait ProtocolRouter: Send + Sync + fmt::Debug {
    /// Whether operation selection requires the complete request body.
    fn requires_body_for_routing(&self) -> bool;

    /// Selects from request metadata and, for body-first protocols, the collected content.
    #[allow(clippy::result_large_err)] // Keep immediate protocol responses allocation-free.
    fn route(&self, request: &Request<Body>, body: &Bytes) -> Result<OperationIndex, Response<BoxBody>>;
}

/// Shared, erased operation router built by a server protocol.
#[derive(Clone, Debug)]
pub struct SharedProtocolRouter(Arc<dyn ProtocolRouter>);

impl SharedProtocolRouter {
    /// Wrap a concrete protocol router.
    pub fn new(router: impl ProtocolRouter + 'static) -> Self {
        Self(Arc::new(router))
    }
}

impl std::ops::Deref for SharedProtocolRouter {
    type Target = dyn ProtocolRouter;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

#[derive(Clone, Debug)]
struct BoundHandler {
    operation: &'static OperationSchema<'static>,
    route: Route<Body>,
    config: RequestBodyCollectionConfig,
}

#[derive(Clone, Debug)]
struct Dispatch {
    router: SharedProtocolRouter,
    protocol: SharedServerProtocol,
    bindings: Vec<BoundHandler>,
}

/// A service routing normalized requests using one protocol and an owned handler collection.
#[derive(Clone, Debug)]
pub struct SchemaRoutingService {
    inner: Either<BodyPreparation<Dispatch>, Dispatch>,
}

#[derive(Clone, Debug)]
struct BodyPreparation<S> {
    inner: S,
    config: RequestBodyCollectionConfig,
}

pin_project_lite::pin_project! {
    struct CollectionFuture {
        body: Body,
        config: RequestBodyCollectionConfig,
        bytes: BytesMut,
        trailers: Option<http::HeaderMap>,
        started: Instant,
        #[pin]
        timeout: Option<tokio::time::Sleep>,
    }
}
impl CollectionFuture {
    fn new(body: Body, config: RequestBodyCollectionConfig) -> Self {
        Self {
            body,
            config,
            bytes: BytesMut::new(),
            trailers: None,
            started: Instant::now(),
            timeout: config.read_timeout.map(tokio::time::sleep),
        }
    }
}
impl Future for CollectionFuture {
    type Output = Result<(Bytes, Option<http::HeaderMap>, Duration), RequestBodyCollectionError<Error>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use http_body::Body as _;
        let mut this = self.project();
        loop {
            if let Some(timeout) = this.timeout.as_mut().as_pin_mut() {
                if timeout.poll(cx).is_ready() {
                    return Poll::Ready(Err(RequestBodyCollectionError::Timeout {
                        timeout: this.config.read_timeout.expect("configured timeout"),
                    }));
                }
            }
            match std::task::ready!(Pin::new(&mut *this.body).poll_frame(cx)) {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => {
                        if let Some(limit) = this.config.max_bytes {
                            if data.len() > limit.get().saturating_sub(this.bytes.len()) {
                                return Poll::Ready(Err(RequestBodyCollectionError::TooLarge(BodyLimitExceeded {
                                    limit: limit.get(),
                                })));
                            }
                        }
                        this.bytes.extend_from_slice(&data);
                    }
                    Err(frame) => {
                        if let Ok(trailers) = frame.into_trailers() {
                            this.trailers.get_or_insert_with(http::HeaderMap::new).extend(trailers);
                        }
                    }
                },
                Some(Err(error)) => return Poll::Ready(Err(RequestBodyCollectionError::Body(error))),
                None => {
                    return Poll::Ready(Ok((
                        this.bytes.split().freeze(),
                        this.trailers.take(),
                        this.started.elapsed(),
                    )))
                }
            }
        }
    }
}

pin_project_lite::pin_project! {
    struct DispatchFuture {
        #[pin]
        inner: futures_util::future::Either<super::route::RouteFuture<Body>, std::future::Ready<Result<Response<BoxBody>, Infallible>>>,
    }
}
impl DispatchFuture {
    fn rejection(response: Response<BoxBody>) -> Self {
        Self {
            inner: futures_util::future::Either::Right(std::future::ready(Ok(response))),
        }
    }
}
impl Future for DispatchFuture {
    type Output = Result<Response<BoxBody>, Infallible>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}
impl Dispatch {
    fn dispatch(&mut self, mut request: Request<Body>, bytes: &Bytes, elapsed: Option<Duration>) -> DispatchFuture {
        let selected = match self.router.route(&request, bytes) {
            Ok(selected) => selected,
            Err(response) => return DispatchFuture::rejection(response),
        };
        let binding = &mut self.bindings[selected.index];
        debug_assert!(
            std::ptr::eq(binding.operation, selected.operation),
            "router index belongs to a different operation"
        );
        if let Some(elapsed) = elapsed {
            let failure = if let Some(limit) = binding.config.max_bytes.filter(|limit| bytes.len() > limit.get()) {
                Some(RequestBodyCollectionError::TooLarge(BodyLimitExceeded {
                    limit: limit.get(),
                }))
            } else {
                binding
                    .config
                    .read_timeout
                    .filter(|timeout| elapsed > *timeout)
                    .map(|timeout| RequestBodyCollectionError::Timeout { timeout })
            };
            if let Some(failure) = failure {
                return DispatchFuture::rejection(crate::schema::body_collection_rejection(&*self.protocol, failure));
            }
        }
        request
            .extensions_mut()
            .insert(SelectedProtocolOperation::new(self.protocol.clone(), binding.operation));
        DispatchFuture {
            inner: futures_util::future::Either::Left(binding.route.call(request)),
        }
    }
}
impl Service<Request<Body>> for Dispatch {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = DispatchFuture;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: Request<Body>) -> Self::Future {
        self.dispatch(request, &Bytes::new(), None)
    }
}

pin_project_lite::pin_project! {
    struct PreparationFuture {
        dispatch: Dispatch,
        parts: Option<http::request::Parts>,
        #[pin]
        collection: CollectionFuture,
        #[pin]
        response: Option<DispatchFuture>,
    }
}
impl Future for PreparationFuture {
    type Output = Result<Response<BoxBody>, Infallible>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        if this.response.is_none() {
            let response = match std::task::ready!(this.collection.poll(cx)) {
                Ok((bytes, trailers, elapsed)) => {
                    let request = Request::from_parts(
                        this.parts.take().expect("request dispatched once"),
                        Body::buffered(bytes.clone(), trailers),
                    );
                    this.dispatch.dispatch(request, &bytes, Some(elapsed))
                }
                Err(error) => {
                    DispatchFuture::rejection(crate::schema::body_collection_rejection(&*this.dispatch.protocol, error))
                }
            };
            this.response.set(Some(response));
        }
        this.response.as_pin_mut().expect("response initialized").poll(cx)
    }
}
impl Service<Request<Body>> for BodyPreparation<Dispatch> {
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = PreparationFuture;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let replacement = self.inner.clone();
        let dispatch = std::mem::replace(&mut self.inner, replacement);
        let (parts, body) = request.into_parts();
        PreparationFuture {
            dispatch,
            parts: Some(parts),
            collection: CollectionFuture::new(body, self.config),
            response: None,
        }
    }
}

pin_project_lite::pin_project! {
    /// Response future for schema routing, including optional body preparation.
    pub struct SchemaRoutingFuture {
        #[pin]
        inner: tower::util::future::EitherResponseFuture<PreparationFuture, DispatchFuture>,
    }
}
impl Future for SchemaRoutingFuture {
    type Output = Result<Response<BoxBody>, Infallible>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}
impl SchemaRoutingService {
    pub fn from_operation_handler_bindings(
        service: &'static ServiceSchema<'static>,
        registrations: impl IntoIterator<Item = ProtocolRegistration>,
        bindings: impl IntoIterator<Item = OperationHandlerBinding>,
    ) -> Result<Self, RouterBuildError> {
        Self::from_operation_handler_bindings_with_options(
            service,
            registrations,
            bindings,
            SchemaRoutingOptions::default(),
        )
    }

    pub fn from_operation_handler_bindings_with_options(
        service: &'static ServiceSchema<'static>,
        registrations: impl IntoIterator<Item = ProtocolRegistration>,
        bindings: impl IntoIterator<Item = OperationHandlerBinding>,
        options: SchemaRoutingOptions,
    ) -> Result<Self, RouterBuildError> {
        if service.protocols().len() != 1 {
            return Err(RouterBuildError::ProtocolCount(service.protocols().len()));
        }
        let mut registry = ProtocolRegistry::builtin();
        for registration in registrations {
            registry = registry.register(registration);
        }
        let protocol = registry.resolve(service).ok_or(RouterBuildError::UnknownProtocol)?;
        let bindings: Vec<_> = bindings.into_iter().collect();
        let mut seen = HashSet::new();
        for binding in &bindings {
            let id = binding.operation.shape_id().as_str();
            if !seen.insert(id)
                || !service
                    .operations()
                    .iter()
                    .any(|op| std::ptr::eq(*op, binding.operation))
            {
                return Err(RouterBuildError::Binding(id.to_owned()));
            }
        }
        for operation in service.operations() {
            if !seen.contains(operation.shape_id().as_str()) {
                return Err(RouterBuildError::Binding(format!("missing {}", operation.shape_id())));
            }
        }
        if seen.len() != service.operations().len() {
            return Err(RouterBuildError::Binding("duplicate service operation schemas".into()));
        }
        for id in options
            .request_body
            .per_operation
            .keys()
            .chain(options.operation_names.keys())
        {
            if !seen.contains(id.as_str()) {
                return Err(RouterBuildError::Configuration(format!("unknown operation {id}")));
            }
        }
        let targets: Vec<_> = bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| OperationIndex {
                index,
                operation: binding.operation,
            })
            .collect();
        let routing_config = options.request_body.for_routing();
        let router = protocol.build_router(service, &targets, &options)?;
        let requires_body = router.requires_body_for_routing();
        if requires_body {
            for operation in service.operations() {
                if [operation.input(), operation.output()]
                    .iter()
                    .any(|schema| schema.members().iter().any(|member| member.streaming()))
                {
                    return Err(RouterBuildError::StreamingBodyRouting {
                        protocol: protocol.protocol_id().to_string(),
                        operation: operation.shape_id().to_string(),
                    });
                }
            }
        }
        let bindings = bindings
            .into_iter()
            .map(|binding| BoundHandler {
                operation: binding.operation,
                route: binding.route,
                config: options.request_body.for_operation(binding.operation.shape_id()),
            })
            .collect();
        let dispatch = Dispatch {
            router,
            protocol,
            bindings,
        };
        Ok(Self {
            inner: if requires_body {
                Either::Left(BodyPreparation {
                    inner: dispatch,
                    config: routing_config,
                })
            } else {
                Either::Right(dispatch)
            },
        })
    }

    /// Applies middleware after routing, uniformly to all bound handlers.
    pub fn layer<L>(mut self, layer: &L) -> Self
    where
        L: tower::Layer<Route<Body>>,
        L::Service: Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible> + Clone + Send + 'static,
        <L::Service as Service<Request<Body>>>::Future: Send + 'static,
    {
        let dispatch = match &mut self.inner {
            Either::Left(preparation) => &mut preparation.inner,
            Either::Right(dispatch) => dispatch,
        };
        dispatch.bindings = std::mem::take(&mut dispatch.bindings)
            .into_iter()
            .map(|binding| BoundHandler {
                operation: binding.operation,
                config: binding.config,
                route: Route::new(layer.layer(binding.route)),
            })
            .collect();
        self
    }
}
impl<B> Service<Request<B>> for SchemaRoutingService
where
    B: http_body::Body<Data = Bytes> + Send + Sync + 'static,
    B::Error: Into<BoxError>,
{
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = SchemaRoutingFuture;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: Request<B>) -> Self::Future {
        SchemaRoutingFuture {
            inner: self.inner.call(request.map(Body::new)),
        }
    }
}

/// Adapts an existing metadata-only router to schema routing.
#[derive(Debug)]
struct ExistingRouter<R, P> {
    router: R,
    protocol: std::marker::PhantomData<fn() -> P>,
}
impl<R, P> ProtocolRouter for ExistingRouter<R, P>
where
    R: super::Router<Body, Service = OperationIndex> + Send + Sync + fmt::Debug,
    R::Error: crate::response::IntoResponse<P>,
    P: fmt::Debug,
{
    fn requires_body_for_routing(&self) -> bool {
        false
    }
    fn route(&self, request: &Request<Body>, _: &Bytes) -> Result<OperationIndex, Response<BoxBody>> {
        use crate::response::IntoResponse;
        self.router.match_route(request).map_err(|error| error.into_response())
    }
}

pub(crate) fn rest_router<P: fmt::Debug + 'static>(
    targets: &[OperationIndex],
) -> Result<SharedProtocolRouter, RouterBuildError>
where
    crate::protocol::rest::router::Error: crate::response::IntoResponse<P>,
{
    use super::request_spec::{PathSegment, QuerySegment, RequestSpec};
    let entries = targets
        .iter()
        .map(|target| {
            let http = target.operation.schema().http().ok_or_else(|| {
                RouterBuildError::Configuration(format!("missing HTTP trait on {}", target.operation.shape_id()))
            })?;
            let method = http
                .method()
                .parse()
                .map_err(|err| RouterBuildError::Protocol(Box::new(err)))?;
            let (path, query) = http.uri().split_once('?').unwrap_or((http.uri(), ""));
            let segments = path
                .trim_start_matches('/')
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|segment| {
                    if segment.starts_with('{') && segment.ends_with("+}") {
                        PathSegment::Greedy
                    } else if segment.starts_with('{') && segment.ends_with('}') {
                        PathSegment::Label
                    } else {
                        PathSegment::Literal(segment.to_owned())
                    }
                })
                .collect();
            let query = form_urlencoded::parse(query.as_bytes())
                .map(|(key, value)| {
                    if value.is_empty() {
                        QuerySegment::Key(key.into_owned())
                    } else {
                        QuerySegment::KeyValue(key.into_owned(), value.into_owned())
                    }
                })
                .collect();
            Ok((
                RequestSpec::new(
                    method,
                    super::request_spec::UriSpec::new(super::request_spec::PathAndQuerySpec::new(
                        super::request_spec::PathSpec::from_vector_unchecked(segments),
                        super::request_spec::QuerySpec::from_vector_unchecked(query),
                    )),
                ),
                *target,
            ))
        })
        .collect::<Result<Vec<_>, RouterBuildError>>()?;
    Ok(SharedProtocolRouter::new(ExistingRouter::<_, P> {
        router: crate::protocol::rest::router::RestRouter::from_iter(entries),
        protocol: std::marker::PhantomData,
    }))
}

pub(crate) fn aws_json_router<P: fmt::Debug + 'static>(
    service: &'static ServiceSchema<'static>,
    targets: &[OperationIndex],
    options: &SchemaRoutingOptions,
) -> Result<SharedProtocolRouter, RouterBuildError>
where
    crate::protocol::aws_json::router::Error: crate::response::IntoResponse<P>,
{
    let entries = targets.iter().map(|target| {
        let name = options
            .operation_names
            .get(target.operation.shape_id().as_str())
            .map(String::as_str)
            .unwrap_or(target.operation.shape_id().shape_name());
        (format!("{}.{}", service.shape_id().shape_name(), name), *target)
    });
    Ok(SharedProtocolRouter::new(ExistingRouter::<_, P> {
        router: crate::protocol::aws_json::router::AwsJsonRouter::from_owned(entries),
        protocol: std::marker::PhantomData,
    }))
}

pub(crate) fn rpc_v2_cbor_router(
    service: &'static ServiceSchema<'static>,
    targets: &[OperationIndex],
    options: &SchemaRoutingOptions,
) -> Result<SharedProtocolRouter, RouterBuildError> {
    let entries = targets.iter().flat_map(|target| {
        let name = target.operation.shape_id().shape_name();
        let mut names = vec![name.to_owned()];
        if options.rpc_v2_cbor_add_capitalized_route {
            let mut chars = name.chars();
            if let Some(first) = chars.next() {
                let alias = format!("{}{}", first.to_uppercase(), chars.as_str());
                if alias != name {
                    names.push(alias);
                }
            }
        }
        names
            .into_iter()
            .map(move |name| (format!("{}.{}", service.shape_id().shape_name(), name), *target))
    });
    Ok(SharedProtocolRouter::new(ExistingRouter::<_, crate::protocol::rpc_v2_cbor::RpcV2Cbor> {
        router: crate::protocol::rpc_v2_cbor::router::RpcV2CborRouter::from_owned(entries),
        protocol: std::marker::PhantomData,
    }))
}

#[cfg(test)]
mod tests;
