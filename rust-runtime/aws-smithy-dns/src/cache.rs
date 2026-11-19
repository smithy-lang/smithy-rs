/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::dns::*;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// All configuration options for a caching DNS resolver
#[derive(Clone, Debug, Default)]
pub struct ResolverConfig {
    internal_resolver: Option<SharedDnsResolver>,
    runtime: Option<tokio::runtime::Handle>,
}

impl ResolverConfig {
    /// Creates a new caching resolver configuration builder instance
    pub fn builder() -> ResolverConfigBuilder {
        ResolverConfigBuilder {
            config: ResolverConfig::default(),
        }
    }
}

/// Builder type for caching resolver configuration instances
#[derive(Debug)]
pub struct ResolverConfigBuilder {
    config: ResolverConfig,
}

impl ResolverConfigBuilder {
    /// Configures what will perform our internal name resolution when the situation requires
    pub fn with_internal_resolver(&mut self, resolver: SharedDnsResolver) -> &Self {
        self.config.internal_resolver = Some(resolver);
        self
    }

    /// Configures the runtime that will execute our recurrent async processing task as well as
    /// name resolution lookups.  We may need to split into two if we want to use anything other
    /// than tokio.  The recurrent async processing task requires tokio (mpsc, one-shot, select!),
    /// while the name resolution lookups could use any kind of future-handling runtime.
    pub fn with_runtime(&mut self, runtime: tokio::runtime::Handle) -> &Self {
        self.config.runtime = Some(runtime);
        self
    }

    /// Creates a new configuration instance for caching resolvers
    pub fn build(&self) -> ResolverConfig {
        self.config.clone()
    }
}

/// Type signature for a resolution request's completion callback
pub type ResolutionCompletionFn = Box<dyn FnOnce(ResolveDnsResult) + Send + Sync>;

/// Internal resolve request type.  Needs to be public so that caching resolver variants can
/// be implemented externally.
pub struct ResolveRequestInternal {
    pub(crate) request: ResolveRequest,

    pub(crate) completion_callback: ResolutionCompletionFn,
}

impl ResolveRequestInternal {
    /// The name resolution request
    pub fn request(&self) -> &ResolveRequest {
        &self.request
    }

    /// Function that will complete the resolution request's user-facing future
    pub fn completion_callback(self) -> ResolutionCompletionFn {
        self.completion_callback
    }
}

impl std::fmt::Debug for ResolveRequestInternal {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.request.fmt(f)
    }
}

/// The result of a name resolution lookup attempt
#[derive(Debug)]
pub struct ResolveResult {
    name: String,
    result: ResolveDnsResult,
}

impl ResolveResult {
    /// What host name this is a resolve result for
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The result of looking up the host name
    pub fn result(&self) -> &ResolveDnsResult {
        &self.result
    }

    /// Creates a new builder for a ResolveResult instance
    pub fn builder(name: String, result: ResolveDnsResult) -> ResolveResultBuilder {
        ResolveResultBuilder::new(name, result)
    }
}

/// Builder type for ResolveResult values
pub struct ResolveResultBuilder {
    result: ResolveResult,
}

impl ResolveResultBuilder {
    fn new(name: String, result: ResolveDnsResult) -> ResolveResultBuilder {
        ResolveResultBuilder {
            result: ResolveResult { name, result },
        }
    }

    /// Create a new ResolveResult value
    pub fn build(self) -> ResolveResult {
        self.result
    }
}

impl Default for ResolveResultBuilder {
    fn default() -> Self {
        todo!()
    }
}

/// The fundamental operation type that a caching resolver works with.
#[non_exhaustive]
#[derive(Debug)]
pub enum ResolverOperation {
    /// An operation that requests an IP address for a host name
    ResolveRequest(ResolveRequestInternal),

    /// An operation holding the result of looking up IP addresses for a host name
    ResolveResult(ResolveResult),

    /// An operation detailing an event that may cause the resolver to change how it
    /// distributes IP addresses to resolve requests
    HostEvent(HostEvent),
}

/// Type signature for the function that performs background task name lookup resolution
pub type ResolutionFunction = Box<dyn Fn(String) + Send + Sync>;

/// Configuration for the async recurrent task that handles all incoming ResolverOperation
/// instances.  This context is passed into the operation handler function a caching resolver
/// was created with, allowing it to do name lookups
pub struct OperationHandlerContext {
    pub(crate) resolution_function: ResolutionFunction,
}

impl Debug for OperationHandlerContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut formatter = f.debug_struct("OperationHandlerContext");
        formatter.finish()
    }
}

impl OperationHandlerContext {
    /// function object that will actually perform lookups.  Lookups are performed on the runtime
    /// associated with the caching resolver
    pub fn resolution_function(&self) -> &ResolutionFunction {
        &self.resolution_function
    }
}

/// Type signature for a function (pointer) that handles resolver operations together with
/// an associated bookkeeping context (of type T)
pub type ResolverOperationHandlerFn<T> =
    fn(context: &OperationHandlerContext, &mut T, ResolverOperation) -> ();

/// Represents the centralized, recurrent task that handles ResolverOperation instances
struct ResolverProcess<T> {
    /// Operation handling context passed to the resolver's operation handler function on each
    /// incoming operation.
    context: OperationHandlerContext,

    /// input queue receiver, all communication from the outside world (including resolution task
    /// results) comes through here
    receiver: mpsc::UnboundedReceiver<ResolverOperation>,

    /// the (mutable) bookkeeping state (cache) the caching resolver uses to make resolution decisions
    state: T,
}

impl<T> ResolverProcess<T> {
    fn new(
        state: T,
        internal_resolver: SharedDnsResolver,
        sender: Arc<mpsc::UnboundedSender<ResolverOperation>>,
        receiver: mpsc::UnboundedReceiver<ResolverOperation>,
    ) -> Self {
        // build the resolver function that runs resolution as a background task on the tokio
        // runtime and then takes the result and sends it back to the resolver process via
        // the operation input channel
        let resolver = internal_resolver.clone();
        let operation_channel = sender.downgrade();
        let resolution_function: ResolutionFunction = Box::new(move |name| {
            let handle = tokio::runtime::Handle::current();
            let internal_name = name.to_string();
            let internal_resolver = resolver.clone();
            let internal_channel = operation_channel.clone();
            handle.spawn(async move {
                let result = internal_resolver.resolve_dns(&internal_name).await;
                let caching_result = ResolverOperation::ResolveResult(ResolveResult {
                    name: internal_name,
                    result,
                });
                let strong_channel_option = internal_channel.upgrade();
                if let Some(strong_channel) = strong_channel_option {
                    let _ = strong_channel.send(caching_result);
                }
            });
        });

        ResolverProcess {
            context: OperationHandlerContext {
                resolution_function,
            },
            receiver,
            state,
        }
    }

    // is it a problem to run this function on the same runtime that the internal resolves are being
    // spawned on?  The worry is that long-running timeouts on blocking-style resolutions (like
    // getaddrinfo) could choke this task's ability to distribute IPs to requests for other names.
    // that aren't having a problem.  Name lookups can take very different amounts of time depending
    // on whether they hit a local system cache or must go out on the network.
    //
    // If we find we need to split them, then we'll need two runtimes.  This function must run on
    // tokio (due to mpsc and select!), while the resolution lookups could run on anything that can
    // handle futures.

    /// Core recurrent loop that handles incoming operations
    async fn run(&mut self, operation_handler: ResolverOperationHandlerFn<T>) {
        let receiver = &mut self.receiver;
        let context = &mut self.context;
        let state = &mut self.state;

        // runs until recv returns None implying all input queue Sender clones are gone
        loop {
            tokio::select! {
                operation_result = receiver.recv() => {
                    match operation_result {
                        Some(operation) => {
                            (operation_handler)(context, state, operation);
                        }
                        None => {
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// Internal caching resolver implementation type that serves as the entry point for
/// resolution requests
#[derive(Debug)]
pub struct ResolverImpl {
    operation_channel: Arc<mpsc::UnboundedSender<ResolverOperation>>,
}

fn spawn_resolution_runtime<T>(
    mut resolver_process: ResolverProcess<T>,
    operation_handler: ResolverOperationHandlerFn<T>,
) where
    T: Send + 'static,
{
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("caching-resolver-fallback-rt")
            .build()
            .unwrap();

        runtime.block_on(async move {
            resolver_process.run(operation_handler).await;
        });
    });
}

impl ResolverImpl {
    /// Creates a new resolver implementation using the default constructor for the bookkeeping
    /// state
    pub fn new_default<T>(
        config: ResolverConfig,
        operation_handler: ResolverOperationHandlerFn<T>,
    ) -> Self
    where
        T: Default + Send + 'static,
    {
        ResolverImpl::new(config, T::default(), operation_handler)
    }

    /// Creates a new resolver implementation
    pub fn new<T>(
        config: ResolverConfig,
        state: T,
        operation_handler: ResolverOperationHandlerFn<T>,
    ) -> Self
    where
        T: Send + 'static,
    {
        let (tx, rx): (
            mpsc::UnboundedSender<ResolverOperation>,
            mpsc::UnboundedReceiver<ResolverOperation>,
        ) = mpsc::unbounded_channel();

        let atx = Arc::new(tx);

        let caching_resolver = ResolverImpl {
            operation_channel: atx.clone(),
        };

        // Config resolver || basic getaddrinfo resolver
        let internal_resolver = config.internal_resolver.unwrap_or(SharedDnsResolver::new(
            aws_smithy_runtime::client::dns::TokioDnsResolver::new(),
        ));
        let mut resolver_process = ResolverProcess::<T>::new(state, internal_resolver, atx, rx);

        // Config runtime || create a new runtime
        if let Some(config_runtime) = config.runtime {
            config_runtime.spawn(async move {
                resolver_process.run(operation_handler).await;
            });
        } else {
            spawn_resolution_runtime(resolver_process, operation_handler);
        }

        caching_resolver
    }
}

impl ResolveDns for ResolverImpl {
    fn resolve_dns<'a>(&'a self, name: &'a str) -> DnsFuture<'a> {
        self.resolve(ResolveRequest::builder(name).build())
    }

    fn resolve<'a>(&'a self, request: ResolveRequest) -> DnsFuture<'a> {
        let (tx, rx) = oneshot::channel::<ResolveDnsResult>();
        let internal_request = ResolveRequestInternal {
            request,
            completion_callback: Box::new(move |result| {
                // if this fails, there's nothing we can do; it's likely the waiter is gone
                let _ = tx.send(result);
            }),
        };

        // kick off the resolve request
        let send_result = self
            .operation_channel
            .send(ResolverOperation::ResolveRequest(internal_request));

        DnsFuture::new(async move {
            if let Err(send_result_err) = send_result {
                return Err(ResolveDnsError::new(std::io::Error::other(send_result_err)));
            }

            let result = rx.await;

            result.unwrap_or_else(|err| Err(ResolveDnsError::new(std::io::Error::other(err))))
        })
    }

    fn notify(&self, event: HostEvent) -> ResolverResult {
        self.operation_channel
            .send(ResolverOperation::HostEvent(event))
            .map_err(|err| err.into())
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::future::Future;
    use std::net::{IpAddr, Ipv4Addr};

    #[derive(Default, Debug)]
    pub(crate) struct EmptyResolverState {}

    fn handle_operation_mock_lookup_success(
        _: &OperationHandlerContext,
        _: &mut EmptyResolverState,
        operation: ResolverOperation,
    ) {
        match operation {
            ResolverOperation::ResolveRequest(request) => {
                let address: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
                let addresses = vec![address];
                let success_result: ResolveDnsResult = Ok(addresses);
                (request.completion_callback)(success_result);
            }

            ResolverOperation::ResolveResult(_) => {}

            ResolverOperation::HostEvent(_) => {}
        }
    }

    fn new_caching_resolver_mock_lookup_success(config: ResolverConfig) -> SharedDnsResolver {
        SharedDnsResolver::new(ResolverImpl::new_default(
            config,
            handle_operation_mock_lookup_success,
        ))
    }

    #[test]
    fn caching_resolver_lookup_success() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let handle = runtime.handle().clone();
        let resolver = new_caching_resolver_mock_lookup_success(
            ResolverConfig::builder()
                .with_runtime(handle.clone())
                .build(),
        );

        let lookup_future = resolver.resolve(ResolveRequest::builder("hello.world.com").build());
        let lookup_result = handle.block_on(lookup_future);

        assert!(lookup_result.is_ok());

        let addresses = lookup_result.unwrap();
        assert_eq!(1, addresses.len());

        let address = &addresses[0];
        assert_eq!("127.0.0.1", address.to_string());
    }

    fn handle_operation_mock_lookup_failure(
        _: &OperationHandlerContext,
        _: &mut EmptyResolverState,
        operation: ResolverOperation,
    ) {
        match operation {
            ResolverOperation::ResolveRequest(request) => {
                let failure_result: ResolveDnsResult =
                    Err(ResolveDnsError::new("Catastrophic Failure"));
                (request.completion_callback)(failure_result);
            }

            ResolverOperation::ResolveResult(_) => {}

            ResolverOperation::HostEvent(_) => {}
        }
    }

    fn new_caching_resolver_mock_lookup_failure(config: ResolverConfig) -> SharedDnsResolver {
        SharedDnsResolver::new(ResolverImpl::new_default(
            config,
            handle_operation_mock_lookup_failure,
        ))
    }

    #[test]
    fn caching_resolver_lookup_fail() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let handle = runtime.handle().clone();
        let resolver = new_caching_resolver_mock_lookup_failure(
            ResolverConfig::builder()
                .with_runtime(handle.clone())
                .build(),
        );

        let lookup_future = resolver.resolve(ResolveRequest::builder("hello.world.com").build());
        let lookup_result = handle.block_on(lookup_future);

        assert!(lookup_result.is_err());
    }

    #[derive(Default)]
    pub(crate) struct MockHostEventCachingResolverState {
        on_host_event_fn: Option<Box<dyn FnOnce(HostEvent) + Send + Sync>>,
    }

    fn handle_operation_mock_host_event(
        _: &OperationHandlerContext,
        state: &mut MockHostEventCachingResolverState,
        operation: ResolverOperation,
    ) {
        match operation {
            ResolverOperation::ResolveRequest(_) => {}

            ResolverOperation::ResolveResult(_) => {}

            ResolverOperation::HostEvent(event) => {
                let completion_callback = state.on_host_event_fn.take();
                if let Some(callback) = completion_callback {
                    (callback)(event);
                }
            }
        }
    }

    fn new_caching_resolver_mock_host_event(
        config: ResolverConfig,
    ) -> (
        SharedDnsResolver,
        std::pin::Pin<Box<dyn Future<Output = HostEvent>>>,
    ) {
        let (tx, rx) = oneshot::channel::<HostEvent>();

        let resolver_state = MockHostEventCachingResolverState {
            on_host_event_fn: Some(Box::new(move |event: HostEvent| {
                let _ = tx.send(event);
            })),
        };

        let resolver = SharedDnsResolver::new(ResolverImpl::new(
            config,
            resolver_state,
            handle_operation_mock_host_event,
        ));

        let event_future = async move || rx.await.unwrap();
        (resolver, Box::pin(event_future()))
    }

    #[test]
    fn caching_resolver_connection_error() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let handle = runtime.handle().clone();
        let (resolver, event_future) = new_caching_resolver_mock_host_event(
            ResolverConfig::builder()
                .with_runtime(handle.clone())
                .build(),
        );

        let address_penalty =
            HostEventAddressPenalty::builder(AddressPenaltyCategory::Fatal).build();
        let host_event = HostEvent::builder(
            "localhost",
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            HostEventInformation::AddressPenalty(address_penalty),
        )
        .build();

        let notify_result = resolver.notify(host_event);
        assert!(notify_result.is_ok());

        let received_event = runtime.block_on(event_future);

        assert_eq!(IpAddr::V4(Ipv4Addr::LOCALHOST), received_event.address());
        assert_eq!("localhost", received_event.name());

        match received_event.event_data() {
            HostEventInformation::AddressPenalty(penalty_event) => {
                assert_eq!(AddressPenaltyCategory::Fatal, *penalty_event.category());
                assert!(penalty_event.source().is_none());
            }
            _ => {
                panic!("Unexpected event data");
            }
        }
    }
}
