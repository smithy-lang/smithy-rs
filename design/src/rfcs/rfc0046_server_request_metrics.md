<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Server request metrics
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC
>
> Applies to: server

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC defines the spec for metrique metrics integration for smithy generated servers using middleware. It will enable users to have an out-of-the-box experience with a set of default request and response metrics, as well as exposing the ability to define and integrate their own additional metrics using metrique in a turnkey fashion. The design ensures to maintain support for folding metrics from all layers of middleware, from custom outer-level middleware to model-level middleware and operation handlers. It also ensures that configuration options are available and extensible.

<!-- The "Terminology" section is optional but is really useful for defining the technical terms you're using in the RFC -->
Terminology
-----------

- **metrique**: https://crates.io/crates/metrique

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

### In the current version of the SDK

See [Appendix A](#appendix-a-code-snippet-for-what-users-need-to-do-without-this-change) for a code snippet.

Users need to define their own metrics middleware. This involves creating a metrics HTTP plugin to support operation-level metrics, as well as a metrics layer if there is a need to fold outer and route level metrics.

To avoid bloating this RFC, see below for an example of the code the user would need to write to integrate metrics into their smithy-rs service with compatibility with all middleware layers. This is with a barebones example with no imports, a few placeholder metrics, and no further logic for injection of customizations such as initialization settings.

### Once this RFC is implemented

Users will get default metrics in their service, with the ability to add an outer `MetricsLayer` for further configuration of metrics collection, including defining their own custom metrics that can be set throughout the request/response lifecycle via extensions, which will all be folded together into a single metrics entry emitted at the end of the lifecycle.

Future scope can elicit a codegen change to add the HTTP plugin for default metrics via declarative opt-in in the `smithy-build.json` to avoid needing code configuration.

After the runtime crate is merged and before the codegen changes are made, users can add default metrics by adding the plugin to their service programmatically.

```rust,ignore
fn main() {
    let http_plugins = HttpPlugins::new().push(DefaultMetricsPlugin);
    let config = PokemonServiceConfig::builder().http_plugin(http_plugins).build();
    let app = PokemonService::builder(config).build()
}
```

For further configuration, a tower layer will be provided that takes an initialization function that gives the user full control over which metrics struct and sink they want to use.

For metrics control in user-defined operation handlers, the types of fields marked with `#[smithy_metrics(operation)]` will be available to be extracted in operation handlers.

Users can get these via `Metrics<T>`, which can be added as a parameter in operation handlers:

```rust,ignore
fn main() {
    let metrics_layer = MetricsLayer::builder()
        .init_metrics(|req| MyMetrics::default().append_on_drop(my_sink))
        .build();
}

#[smithy_metrics]
#[metrics]
struct MyMetrics {
    #[smithy_metrics(operation)]
    #[metrics(flatten)]
    operation_metrics: PokemonOperationMetrics,
    custom_metric: Option<String>
}

#[metrics]
struct PokemonOperationMetrics {
    #[metrics(flatten)]
    get_pokemon_species_metrics: GetPokemonSpeciesMetrics,
}

#[metrics]
struct GetPokemonSpeciesMetrics {
    request_pokemon_name: Option<String>,
}

/// Setting metrics in operation handler
pub async fn get_pokemon_species(
    input: input::GetPokemonSpeciesInput,
    state: Extension<Arc<State>>,
    mut metrics: Metrics<PokemonOperationMetrics>
) -> Result<output::GetPokemonSpeciesOutput, error::GetPokemonSpeciesError> {
    metrics.get_pokemon_species_metrics.requested_pokemon_name = Some(input.name.clone());
}
```

`new_with_sink` will be a convenience API to use the default metrics with a custom sink.

```rust,ignore
fn main() {
    let metrics_layer = MetricsLayer::new_with_sink(my_sink);
}
```

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

There will be two new rust-runtime crates `aws-smithy-http-server-metrics` and `aws-smithy-http-server-metrics-macro`, with types re-exported in the generated server.

### `DefaultMetricsPlugin` struct

The focal struct that users can add to their service as an HTTP plugin which provides the default metrics. If no outer metrics layer exists, it will initialize the metrics from this layer if a sink has been attached to the metrique's application-wide `ServiceMetrics`. If an outer metrics layer was added, it will retrieve the instance from the request/response extensions and add them.

We can later add this into the codegen as a declarative config option to make it fully batteries-included from the codegen layer. Though, precautions will need to be taken to prevent the possibility of multiple metrics plugins existing and creating duplicate metrics.

#### This will allow users the following construction experiences:

`DefaultMetricsPlugin` (for default)

- For a metrics plugin that uses the application-wide `metrique::ServiceMetrics`.

### `DefaultMetricsPluginService` struct

A tower service for metrics that will contain the logic for setting the default operation-specific metrics.

### Super traits

These super traits will contain blanket implementations for reducing duplication in trait bounds.

#### `ThreadSafeCloseEntry: CloseEntry + Send + Sync + 'static`

#### `ThreadSafeEntrySink<Entry>: EntrySink<RootEntry<Entry::Closed>> + Send + Sync + 'static`

#### `InitMetrics<Entry, Sink>: Fn() -> AppendAndCloseOnDrop<Entry, Sink> + Clone + Send + Sync + 'static`

#### `ResponseMetrics<Entry>: Fn(&mut Response<ResBody>, &mut Entry) + Clone + Send + Sync + 'static`

### `MetricsLayer` struct

The focal struct that users can add to their service as a tower `Layer` for metrics, containing a `builder` method.

Any metrics configuration such as altering the sink, using a custom metrique metrics type, etc will go through this metrics layer.

This gives users the ability to add a metrics tower layer that initializes metrics at the outermost level in order to fold all metrics throughout the request together. Request/response extensions provides a handle to the metrics at every subsequent layer. There will be logic in the `DefaultMetricsPlugin` to use the initialized metrics from the extension if a `MetricsLayer` has been added.

#### Will have following generics and trait bounds:

`Entry: ThreadSafeCloseEntry`

- For the metrique metrics struct (i.e. annotated with #[metrics]).

- Default type parameter of `DefaultMetrics`

`Sink: ThreadSafeEntrySink`

- For the entry sink where entries are stored in an in-memory buffer until they can be written to the destination.

- Default type parameter of `DefaultSink` (BoxEntrySink)

`Init: InitMetrics<Entry, Sink>`

- Closure trait bound for the metrics initialization function. The metrics initialization must be passed down and invoked in the `MetricsLayerService`'s, so that entries are appended and closed after each request when the metrics guard is dropped. In a future version, we may be able to implement manual appending and closing of entries to enable users to pass an instance of the metrics struct itself rather than a function that returns an `AppendAndCloseOnDrop`.

- Default type parameter of `fn() -> AppendAndCloseOnDrop<Entry, Sink>`

`Res: ResponseMetrics<Entry>`

- Closure trait bound to allow users to set metrics however they like from the response object, which will be invoked in the `MetricsLayerService` after the metrics have been initialized.

- Default type parameter of `fn(&Response<ResBody>, &mut Entry)`

#### Will have the following fields:

```rust,ignore
init_metrics: Init,
response_metrics: Option<Res>,
default_metrics_extension_fn: fn(
    &mut Request<ReqBody>,
    &mut Entry,
    DefaultRequestMetricsConfig,
    DefaultResponseMetricsConfig,
    DefaultMetricsServiceState,
),
default_req_metrics_config: DefaultRequestMetricsConfig,
default_res_metrics_config: DefaultResponseMetricsConfig,
_entry_sink: PhantomData<S>,
```

#### Will have the following implementations:

- A fully generic implementation with a `builder()` method that returns a `MetricsLayerBuilder`.

- Implements the tower `Layer` trait to map to the `MetricsLayerService`

#### This will allow users the following construction experiences:

`MetricsLayer::builder()`

- For a builder with a required metrics initialization and optional configuration for default metrics inclusion, setting custom request/response metrics, etc

### `MetricsLayerBuilder` struct

A typestate builder for `MetricsLayer` containing the same trait bounds. The `build` implementations for custom metrics type will be generated by the proc macro, which will set the `default_metrics_extension_fn` to add the default metrics extensions to the `DefaultMetricsPlugin`.

The states will be:

`NeedsInitialization`

- Exposes `init_metrics` method so an initialization closure can be provided.

- Exposes `try_init_with_defaults` with default metrics initialization using metrique's application-wide global entry sink [`metrique::ServiceMetrics`], returning an error if a sink has not been attached.

`WithRq`

A state where the field for setting metrics from the req object has been set.

- Exposes methods for disabling any/all of the default metrics.

- Exposes method for taking Fn closures for setting metrics from res objects.

- Exposes build method.

`WithRqAndRs`

A state where both the fields for setting metrics from the req and res object have been set.

- Exposes methods for disabling any/all of the default metrics.

- Exposes build method.

Implementations for each state will be made for metrics structs annotated with the `#[smithy_metrics]` proc macro that exposes a `build` method.

Declarative macros will be used for the typestate pattern.

### `MetricsLayerService` struct

A tower service for metrics that will contain the core logic for invoking the logic for metrics initialization and setting the metrics from the request/response objects. The `call` implementation will essentially invoke the passed closures.

### `DefaultMetrics` struct

A struct for default metrics. This will use the `#[smithy_metrics]` and be used as the default type parameter for `MetricsLayer`.

### `DefaultRequestMetrics` struct

A struct that will contain fields for the default request metrics, a field for which will be added to a struct containing the `#[smithy_metrics]` annotation.

### `DefaultResponseMetrics` struct

A struct that will contain fields for the default response metrics, a field for which will be added to a struct containing the `#[smithy_metrics]` annotation.

### `DefaultRequestMetricsConfig` struct

A struct that will contain fields for control of toggleability of the default request metrics.

### `DefaultResponseMetricsConfig` struct

A struct that will contain fields for control of toggleability of the default response metrics.

### `DefaultRequestMetricsExtension` struct

A struct that will contain a `DefaultRequestMetrics` and `DefaultRequestMetricsConfig` to be passed through the request extensions from an outer metrics layer to be used in the metrics plugin to set the default metrics with the given configuration. This enables us to fold all metrics together.

### `DefaultResponseMetricsExtension` struct

A struct that will contain a `DefaultResponseMetrics` and `DefaultResponseMetricsConfig` to be passed through the request extensions from an outer metrics layer to be used in the metrics plugin to set the default metrics with the given configuration. This enables us to fold all metrics together.

### `DefaultMetricsExtension` struct

A struct that will contain a `DefaultRequestMetricsExtension` and `DefaultResponseMetricsExtension` to be inserted into the request extensions so that metrics from `MetricsLayer` can be folded with default metrics set in `DefaultMetricsPlugin`.

### `operation::Metrics` struct

A struct that can be extracted from the operation handlers. This will implement `FromParts` to do the extraction of the actual `operation::MetricsExtension` that will be inserted in the request extensions in order to build and return a struct of this type. It will also implement `Deref` and `DerefMut` to allow direct setting of the metrics fields.

### `operation::MetricsExtension` struct

A wrapper struct for extensions that are inserted as a result of the `#[smithy_metrics(operation)]` attribute macro on a struct field. An `Arc<Mutex<SlotGuard<Slot<X>>>>>` will be used to wrap any extensions to be forward compatible with the `Clone` bound introduced in http 1.x's (smithy-rs currently is on http 0.2, but the http 1.x work is underway and soon to be complete). This struct will provide User's an API where they do not have to be exposed to this complicated type. The double slot will provide a better public API via the `operation::Metrics` struct. The outer `SlotGuard` will fold into the root level metrics while the inner is for the operation handler side.

This struct will need to be public to allow usage in aws-smithy-http-server-metrics-macro, but should have negative guidance in direct usage.

### `#[smithy_metrics]` attribute proc macro

A proc macro that can be placed on a metrique metrics struct for the adding of default metrics fields and the expansion of a `MetricsLayerBuilder` implementation for the annotated struct.

This will also come with `#[smithy_metrics(operation)]` to mark struct fields for insertion to the request extensions to be used in custom middleware or operation handlers.

<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [x] Create `rust-runtime` crates `aws-smithy-http-server-metrics` and `aws-smithy-http-server-metrics-macro`

- [x] Implement struct `MetricsLayer`

    - [x] Define struct with generics, trait bounds, and default type parameters

    - [x] Implementation of `builder()` method

    - [x] Implementation of `new_with_sink()` method to use the `DefaultMetrics` with a custom sink

    - [x] Layer implementation to map to `MetricsLayerService`

- [x] Implement common traits and types

- [x] Implement struct `MetricsLayerBuilder`

    - [x] Define struct with generics and trait bounds for metrics and typestate pattern

    - [x] `NeedsInitialization` state generic implementation with `init_metrics()` method

    - [x] `WithRq` state implementation

    - [x] `WithRqAndRs` state implementation

    - [x] `build()` implementations for each state for `DefaultMetrics` (may be replaced with the proc macro expansion when that is done)

    - [x] Declarative macros to reduce duplication

- [x] Implement struct `MetricsLayerService` to contain the tower service logic

    - [x] Implement `Clone`

    - [x] Implement tower `Service` to contain logic for:

        - [x] Invoking the initialization function

        - [x] Invoking the functions for adding the default request/response metrics extensions

        - [x] Invoking the request/response metrics functions

- [x] Implement struct `DefaultMetricsPlugin`

    - [x] Implement HTTPMarker

    - [x] Implement `Plugin` to map to `MetricsService`

- [x] Implement struct `DefaultMetricsPluginService`

    - [x] Implement `Clone`

    - [x] Implement tower `Service` to contain logic for:

        - [x] Retrieving `DefaultRequestMetricsExtension` from the request extensions when an outer metrics layer was added.

        - [x] In the case where there is no `DefaultRequestMetricsExtension` in the request extensions, it means there was no outer metrics layer added. Therefore, we will initialize default metrics in the plugin defaults if a global sink has been installed.

- [x] Define struct `DefaultMetrics` with a request and response metrics field of the types `DefaultRequestMetrics` and `DefaultResponseMetrics`, respectively

- [x] Define struct `DefaultRequestMetrics` to contain the out-of-the-box request metrics

- [x] Define struct `DefaultResponseMetrics`to contain the out-of-the-box response metrics

- [x] Define struct `DefaultRequestMetricsConfig` for default request metrics configuration

- [x] Define struct `DefaultResponseMetricsConfig` for default response metrics configuration

- [x] Define struct `DefaultRequestMetricsExtension` for passing default request metrics and config instances from an outer metrics layer to the metrics plugin via request extensions to be able to fold them together

- [x] Define struct `DefaultResponseMetricsExtension` for passing default response metrics and config instances from an outer metrics layer to the metrics plugin via response extensions to be able to fold them together

- [x] Define struct `DefaultMetricsExtension`

- [x] Define and implement struct `operation::Metrics`
    
    - [x] Implement FromParts, Deref, DerefMut

- [x] Define and implement struct `operation::MetricsExtension`

- [x] Implement proc macro attribute `#[smithy_metrics]`

    - [x] Implement expansion to wrap the types of fields annotated with `#[smithy_metrics(operation)]` with `Slotguard` if the user hasn't explicitly done so, or show an error message telling them to

    - [x] Implement expansion of a `MetricsLayerBuilder` implementation for receiving metrics type containing a `build` method

- [x] Create proc macro attribute `#[smithy_metrics(operation)]` on fields of a metrics struct to give users the ability to annotate the fields they want to be accessible from the request extensions down the line, such as in custom middleware or operation handlers

# Appendix A: Code snippet for what users need to do without this change

```rust,ignore
fn main() {
    let metrics_layer = MetricsLayer;
    let metrics_plugin = MetricsPlugin;

    let http_plugins = HttpPlugins::new()
        .push(metrics_plugin)
        .insert_operation_extension();

    let config = PokemonServiceConfig::builder()
        .http_plugin(http_plugins)
        .build();
    
    let app = PokemonService::builder(config)
        .get_pokemon_species(get_pokemon_species)
        .build()
        .expect("failed to build an instance of PokemonService");

    let service = metrics_layer.layer(app);
}

/// Operation handler for get_pokemon_species
async fn get_pokemon_species(
    input: input::GetPokemonSpeciesInput,
    state: Extension<Arc<State>>,
    Extension(mut metrics): OperationMetricsExtension,
) -> Result<output::GetPokemonSpeciesOutput, error::GetPokemonSpeciesError> {
    metrics.get_pokemon_species_metrics = Some("Pokemon Species Metrics".to_string());
    // ... Business logic
}

#[metrics(rename_all = "PascalCase")]
#[derive(Default)]
struct Metrics {
    #[metrics(flatten)]
    request_metrics: RequestMetrics,
    #[metrics(flatten)]
    operation_metrics: SlotGuard<OperationMetrics>,
    #[metrics(flatten)]
    response_metrics: ResponseMetrics
    // ... other metrics
}
impl Metrics {
    pub fn init() -> ApiRequestMetricsGuard<DefaultQueue> {
        Self {
            operation,
            ..Default::default()
        }
        .append_on_drop(ServiceMetrics::sink())
    }
}

#[metrics]
#[derive(Default)]
struct RequestMetrics {
    service_name: Option<String>,
    operation_name: Option<String>,
    service_version: Option<String>,
    request_id: Option<String>,
    #[metrics(timestamp)]
    start: Timestamp
    // ... other metrics
}

#[metrics]
#[derive(Default)]
struct ResponseMetrics {
    http_status_code: Option<String>
    // ... other metrics
}

#[metrics]
#[derive(Default)]
struct OperationMetrics {
    get_pokemon_species_metrics: Option<String>
}

#[derive(Debug)]
struct MetricsLayer {}

impl<S> Layer<S> for MetricsLayer {
    type Service = MetricsService<S, Init, Req, Res, ReqBody, ResBody, CE, ES>;

    fn layer(&self, inner: S) -> Self::Service {
        MetricsService {
            inner
        }
    }
}

#[derive(Debug)]
struct MetricsLayerService<S> {
    inner: S
}
impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for MetricsService<S, ReqBody, ResBody> {
    type Response = S::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>,
    >;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let mut metrics = Metrics::init();
        
        let mut request_metrics = metrics.request_metrics;
        let mut response_metrics = metrics.response_metrics;

        let api_operation_metrics = metrics.operation_metrics
            .open(OnParentDrop::Discard)
            .expect("unreachable: the slot was created in this scope and is not opened before this point");
        req.extensions_mut().insert(api_operation_metrics);
        
        request_metrics.request_id = try_extract_request_id();
        request_metrics.start = Default::default();
        // ... set other metrics

        let future = self.inner.call(req);

        futures::FutureExt::boxed(async move {
            let response = match future.await {
                Ok(resp) => resp,
                Err(e) => return Err(e),
            };

            response_metrics.http_status_code = Some(response.status().as_u16());
            // ... set other metrics

            Ok(response)
        })
    }
}

#[derive(Debug)]
struct MetricsPlugin {}

impl HttpMarker for MetricsPlugin {}

impl<Ser, Op, T> Plugin<Ser, Op, T> for MetricsPlugin
where
    Op: OperationShape,
    Ser: ServiceShape,
{
    type Output = MetricsPluginService<T>;

    fn apply(&self, inner: T) -> Self::Output {
        return MetricsPluginService {
            inner,
            operation_name: None,
            service_name: None,
            service_version: None,
        };

        MetricsPluginService {
            inner,
            operation_name: Op::ID.name(),
            service_name: Ser::ID.name(),
            service_version: Ser::VERSION,
        }
    }
}

#[derive(Debug)]
pub struct MetricsPluginService<T> {
    inner: T,
    operation_name: &'static str,
    service_name: &'static str,
    service_version: &'static str,
}

impl<T> Clone for MetricsPluginService<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            operation_name: self.operation_name,
            service_name: self.service_name,
            service_version: self.service_version,
        }
    }
}

impl<T, ReqBody, ResBody> Service<Request<ReqBody>> for MetricsPluginService<T>
where
    T: Service<Request<ReqBody>, Response = Response<ResBody>>,
    T::Future: Send + 'static,
{
    type Response = T::Response;
    type Error = T::Error;
    type Future = T::Future;

    fn call(&mut self, mut request: Request<ReqBody>) -> Self::Future {
        let request_metrics = request
            .extensions_mut()
            .get_mut::<SlotGuard<DefaultRequestMetrics>>();

        if let Some(request_metrics) = request_metrics {
            request_metrics.operation_name = Some(self.operation_name.to_string());
            request_metrics.service_name = Some(self.service_name.to_string());
            request_metrics.service_version = Some(self.service_version.to_string());
        }

        self.inner.call(request)
    }

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }
}

fn try_extract_request_id<B>(request: &Request<B>) -> Option<String> {
    request
        .headers()
        .get("requestid")?
        .to_str()
        .inspect_err(|error| {
            tracing::warn!(
                "requestid header was not representable as string: {:?}",
                error
            );
        })
        .ok()
        .map(Into::into)
}
```