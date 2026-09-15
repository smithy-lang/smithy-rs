/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime construction and dispatch for one selected schema protocol.

use super::Route;
use crate::{
    body::BoxBody,
    error::BoxError,
    schema::{
        ProtocolRegistration, ProtocolRegistry, SelectedProtocolOperation, ServiceRequestBodyConfig,
        SharedServerProtocol,
    },
};
use aws_smithy_schema::{OperationSchema, ServiceSchema};
use bytes::Bytes;
use http::{Request, Response};

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service;

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

/// A protocol-independent operation and its HTTP handler, generic over the transport body `B`.
pub struct OperationHandlerBinding<B = hyper::body::Incoming> {
    operation: &'static OperationSchema<'static>,
    route: Route<crate::body::SchemaBody<B>>,
}
impl<B> OperationHandlerBinding<B> {
    /// Binds an operation to a handler, without assigning any protocol-specific routing rule.
    pub fn new(operation: &'static OperationSchema<'static>, route: Route<crate::body::SchemaBody<B>>) -> Self {
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

/// Selects an operation within an already-selected protocol from request metadata alone.
/// All rejections are terminal.
///
/// The request carries no body: metadata routing never reads one, and keeping the trait
/// body-free keeps it usable behind `dyn` for every transport body type.
pub trait ProtocolRouter: Send + Sync + fmt::Debug {
    /// Selects from the request URI, method and headers.
    #[allow(clippy::result_large_err)] // Keep immediate protocol responses allocation-free.
    fn route(&self, request: &Request<()>) -> Result<OperationIndex, Response<BoxBody>>;
}

/// The body a body-first router collected while routing.
///
/// The routing service rebuilds the dispatched request around this content, so the selected
/// handler replays exactly the bytes routing read.
#[derive(Debug)]
pub struct CollectedBody {
    /// The collected content.
    pub bytes: Bytes,
    /// Trailers read with the content, when the transport delivered any.
    pub trailers: Option<http::HeaderMap>,
}

/// The future returned by [`AsyncProtocolRouter::route`].
pub type ProtocolRouteFuture =
    Pin<Box<dyn Future<Output = Result<(OperationIndex, Request<CollectedBody>), Response<BoxBody>>> + Send>>;

/// Selects an operation for protocols that read the request body to route.
///
/// The router owns the request while routing: it collects the erased body under the allowance
/// it was built with (see [`collect_for_routing`](crate::schema::collect_for_routing)) and
/// returns the request with the [`CollectedBody`]. A body-first protocol always buffers before
/// selecting, so its output is the buffered content, never the transport body — which is what
/// keeps this trait `dyn`-safe and free of the transport body type. All rejections are terminal
/// and the protocol frames them itself.
pub trait AsyncProtocolRouter: Send + Sync + fmt::Debug {
    /// Selects an operation, returning the request for dispatch to its handler.
    fn route(self: Arc<Self>, request: Request<BoxBody>) -> ProtocolRouteFuture;
}

/// Shared, erased operation router built by a server protocol.
#[derive(Clone, Debug)]
pub struct SharedProtocolRouter(RouterKind);

#[derive(Clone, Debug)]
enum RouterKind {
    Metadata(Arc<dyn ProtocolRouter>),
    Body(Arc<dyn AsyncProtocolRouter>),
}

impl SharedProtocolRouter {
    /// Wraps a router that selects from request metadata alone.
    pub fn new(router: impl ProtocolRouter + 'static) -> Self {
        Self(RouterKind::Metadata(Arc::new(router)))
    }

    /// Wraps a router that reads the request body to select an operation.
    pub fn new_async(router: impl AsyncProtocolRouter + 'static) -> Self {
        Self(RouterKind::Body(Arc::new(router)))
    }

    /// Whether operation selection reads the request body.
    pub fn routes_on_body(&self) -> bool {
        matches!(self.0, RouterKind::Body(_))
    }
}

struct BoundHandler<B> {
    operation: &'static OperationSchema<'static>,
    route: Route<crate::body::SchemaBody<B>>,
}
impl<B> Clone for BoundHandler<B> {
    fn clone(&self) -> Self {
        Self {
            operation: self.operation,
            route: self.route.clone(),
        }
    }
}
impl<B> fmt::Debug for BoundHandler<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundHandler")
            .field("operation", &self.operation.shape_id())
            .finish()
    }
}

struct Dispatch<B> {
    router: SharedProtocolRouter,
    protocol: SharedServerProtocol,
    bindings: Vec<BoundHandler<B>>,
}
impl<B> Clone for Dispatch<B> {
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
            protocol: self.protocol.clone(),
            bindings: self.bindings.clone(),
        }
    }
}
impl<B> fmt::Debug for Dispatch<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dispatch")
            .field("router", &self.router)
            .field("protocol", &self.protocol)
            .field("bindings", &self.bindings)
            .finish()
    }
}

/// A service routing normalized requests using one protocol and an owned handler collection.
///
/// Generic over the transport body `B`: requests entering with the transport's own body flow to
/// handlers unerased. The default is hyper's body; anything else — tests, upgrade layers, other
/// transports — enters through [`SchemaBody::new`](crate::body::SchemaBody::new)-built bodies.
pub struct SchemaRoutingService<B = hyper::body::Incoming> {
    inner: Dispatch<B>,
}
impl<B> Clone for SchemaRoutingService<B> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
impl<B> fmt::Debug for SchemaRoutingService<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchemaRoutingService").field("inner", &self.inner).finish()
    }
}

impl<B> Dispatch<B>
where
    B: http_body::Body<Data = Bytes> + Send + Sync + Unpin + 'static,
    B::Error: Into<BoxError>,
{
    /// Hands the routed request to its handler, recording the selection for downstream consumers.
    fn handle(
        &mut self,
        selected: OperationIndex,
        mut request: Request<crate::body::SchemaBody<B>>,
    ) -> super::route::RouteFuture<crate::body::SchemaBody<B>> {
        let binding = &mut self.bindings[selected.index];
        debug_assert!(
            std::ptr::eq(binding.operation, selected.operation),
            "router index belongs to a different operation"
        );
        request
            .extensions_mut()
            .insert(SelectedProtocolOperation::new(self.protocol.clone(), binding.operation));
        binding.route.call(request)
    }

    fn call(&mut self, request: Request<crate::body::SchemaBody<B>>) -> SchemaRoutingFuture<B> {
        let state = match &self.router.0 {
            RouterKind::Metadata(router) => {
                // Probe with the head only: the parts move over and back, nothing is cloned,
                // and the router stays free of the transport body type.
                let (parts, body) = request.into_parts();
                let probe = Request::from_parts(parts, ());
                match router.route(&probe) {
                    Ok(selected) => {
                        let (parts, ()) = probe.into_parts();
                        State::Handling {
                            future: self.handle(selected, Request::from_parts(parts, body)),
                        }
                    }
                    Err(response) => State::Rejected {
                        response: Some(response),
                    },
                }
            }
            // A body-first protocol buffers everything before selecting, so erasing the body
            // here costs one box on a path that allocates the full content anyway.
            RouterKind::Body(router) => State::Routing {
                future: router.clone().route(request.map(crate::body::boxed)),
                dispatch: Some(self.clone()),
            },
        };
        SchemaRoutingFuture { inner: state }
    }
}

pin_project_lite::pin_project! {
    #[project = StateProj]
    enum State<B> {
        // The routing future owns the request; the dispatch clone owns the handlers. Nothing is
        // borrowed across the await, so `Route` needs `Clone + Send` but never `Sync`.
        Routing {
            future: ProtocolRouteFuture,
            dispatch: Option<Dispatch<B>>,
        },
        Handling {
            #[pin]
            future: super::route::RouteFuture<crate::body::SchemaBody<B>>,
        },
        Rejected {
            response: Option<Response<BoxBody>>,
        },
    }
}

pin_project_lite::pin_project! {
    /// Response future for schema routing.
    pub struct SchemaRoutingFuture<B = hyper::body::Incoming> {
        #[pin]
        inner: State<B>,
    }
}
impl<B> Future for SchemaRoutingFuture<B>
where
    B: http_body::Body<Data = Bytes> + Send + Sync + Unpin + 'static,
    B::Error: Into<BoxError>,
{
    type Output = Result<Response<BoxBody>, Infallible>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.inner.as_mut().project() {
                StateProj::Routing { future, dispatch } => match future.as_mut().poll(cx) {
                    Poll::Ready(Ok((selected, request))) => {
                        let mut dispatch = dispatch.take().expect("routing resolves once");
                        let request =
                            request.map(|collected| crate::body::SchemaBody::buffered(collected.bytes, collected.trailers));
                        let future = dispatch.handle(selected, request);
                        this.inner.set(State::Handling { future });
                    }
                    Poll::Ready(Err(response)) => return Poll::Ready(Ok(response)),
                    Poll::Pending => return Poll::Pending,
                },
                StateProj::Handling { future } => return future.poll(cx),
                StateProj::Rejected { response } => {
                    return Poll::Ready(Ok(response.take().expect("polled after completion")))
                }
            }
        }
    }
}
impl<B> SchemaRoutingService<B> {
    pub fn from_operation_handler_bindings(
        service: &'static ServiceSchema<'static>,
        registrations: impl IntoIterator<Item = ProtocolRegistration>,
        bindings: impl IntoIterator<Item = OperationHandlerBinding<B>>,
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
        bindings: impl IntoIterator<Item = OperationHandlerBinding<B>>,
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
        let router = protocol.build_router(service, &targets, &options)?;
        if router.routes_on_body() {
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
            })
            .collect();
        Ok(Self {
            inner: Dispatch {
                router,
                protocol,
                bindings,
            },
        })
    }

    /// Applies middleware after routing, uniformly to all bound handlers.
    pub fn layer<L>(mut self, layer: &L) -> Self
    where
        L: tower::Layer<Route<crate::body::SchemaBody<B>>>,
        L::Service: Service<Request<crate::body::SchemaBody<B>>, Response = Response<BoxBody>, Error = Infallible>
            + Clone
            + Send
            + 'static,
        <L::Service as Service<Request<crate::body::SchemaBody<B>>>>::Future: Send + 'static,
    {
        let dispatch = &mut self.inner;
        dispatch.bindings = std::mem::take(&mut dispatch.bindings)
            .into_iter()
            .map(|binding| BoundHandler {
                operation: binding.operation,
                route: Route::new(layer.layer(binding.route)),
            })
            .collect();
        self
    }
}
/// The transport door: the transport's own body enters unerased.
impl<B> Service<Request<B>> for SchemaRoutingService<B>
where
    B: http_body::Body<Data = Bytes> + Send + Sync + Unpin + 'static,
    B::Error: Into<BoxError>,
{
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = SchemaRoutingFuture<B>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: Request<B>) -> Self::Future {
        self.inner.call(request.map(crate::body::SchemaBody::passthrough))
    }
}

/// The pipeline door: an already-normalized body — buffered, erased, or rebuilt — enters as-is.
/// Coherent with the transport door because `B` can never equal `SchemaBody<B>`.
impl<B> Service<Request<crate::body::SchemaBody<B>>> for SchemaRoutingService<B>
where
    B: http_body::Body<Data = Bytes> + Send + Sync + Unpin + 'static,
    B::Error: Into<BoxError>,
{
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = SchemaRoutingFuture<B>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: Request<crate::body::SchemaBody<B>>) -> Self::Future {
        self.inner.call(request)
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
    R: super::Router<(), Service = OperationIndex> + Send + Sync + fmt::Debug,
    R::Error: crate::response::IntoResponse<P>,
    P: fmt::Debug,
{
    fn route(&self, request: &Request<()>) -> Result<OperationIndex, Response<BoxBody>> {
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
