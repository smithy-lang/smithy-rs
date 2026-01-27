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

Users need to define their own metrics middleware. This involves creating a metrics HTTP plugin to support operation-level metrics, as well as a metrics layer if there is a need to fold outer and route level metrics.

To avoid bloating this RFC, see below for an example of the code the user would need to write to integrate metrics into their smithy-rs service with compatibility with all middleware layers. This is with a barebones example with no imports, a few placeholder metrics, and no further logic for injection of customizations such as initialization settings.

https://play.rust-lang.org/?version=stable&mode=debug&edition=2024&gist=9c40172125518215d2b4482526d1e306

### Once this RFC is implemented

Users will get default metrics in their service, with the ability to add an outer `MetricsLayer` for further configuration of metrics collection, including defining their own custom metrics that can be set throughout the request/response lifecycle via extensions, which will all be folded together into a single metrics entry emitted at the end of the lifecycle.

Future scope can elicit a codegen change to add the HTTP plugin for default metrics via declarative opt-in in the `smithy-build.json` to avoid needing code configuration.

After the runtime crate is merged and before the codegen changes are made, users can add default metrics by adding the plugin to their service programmatically.

```rust
fn main() {
    let http_plugins = HttpPlugins::new().push(DefaultMetricsPlugin);
    let config = PokemonServiceConfig::builder().http_plugin(http_plugins).build();
    let app = PokemonService::builder(config).build()
}
```

For further configuration, a tower layer will be provided that takes an initialization function that gives the user full control over which metrics struct and sink they want to use.

For metrics control in user-defined operation handlers, the types of fields marked with `#[smithy_metrics::extension]` will be available in the request extensions.

Users can get these via `Extension<Metrics<OperationMetrics>>`, which can be added as a parameter in operation handlers:

```rust
fn main() {
    let metrics_layer = MetricsLayer::builder()
        .init_metrics(|| MyMetrics::default().append_on_drop(my_sink))
        .request_metrics(|req: &mut Request<ReqBody>, metrics: &mut MyMetricsGuard| {
            req.custom_metric = ...;
        })
        .build();
}

#[smithy_metrics::root]
#[metrics]
struct MyMetrics {
    #[smithy_metrics::extension]
    #[metrics(flatten)]
    operation_metrics: OperationMetrics,
    custom_metric: Option<String>
}

#[metrics]
struct OperationMetrics {
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
    Extension(mut metrics): Extension<Metrics<OperationMetrics>>
) -> Result<output::GetPokemonSpeciesOutput, error::GetPokemonSpeciesError> {
    metrics
        .set(|mut operation_metrics| {
            operation_metrics
                .get_pokemon_species_metrics
                .requested_pokemon_name = Some(input.name.clone());
        })
        .unwrap_or_else(|e| {
            tracing::error!("Error setting metrics in get_pokemon_species: {e}");
        });
}
```

`new_with_sink` will be a convenience API to use the default metrics with a custom sink.

```rust
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

#### `ThreadSafeEntrySink<E>: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static`

#### `InitMetrics<E, S>: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static`

#### `RequestMetrics<E>: Fn(&mut Request<ReqBody>, &mut E) + Clone + Send + Sync + 'static`

#### `ResponseMetrics<E>: Fn(&mut Response<ResBody>, &mut E) + Clone + Send + Sync + 'static`

### `MetricsLayer` struct

The focal struct that users can add to their service as a tower `Layer` for metrics, containing a `builder` method.

Any metrics configuration such as altering the sink, using a custom metrique metrics type, etc will go through this metrics layer.

This gives users the ability to add a metrics tower layer that initializes metrics at the outermost level in order to fold all metrics throughout the request together. Request/response extensions provides a handle to the metrics at every subsequent layer. There will be logic in the `DefaultMetricsPlugin` to use the initialized metrics from the extension if a `MetricsLayer` has been added.

#### Will have following generics and trait bounds:

`E: ThreadSafeCloseEntry`

- For the metrique metrics struct (i.e. annotated with #[metrics]).

- Default type parameter of `DefaultMetrics`

`S: ThreadSafeEntrySink`

- For the entry sink where entries are stored in an in-memory buffer until they can be written to the destination.

- Default type parameter of `DefaultSink` (BoxEntrySink)

`I: InitMetrics<E, S>`

- Closure trait bound for the metrics initialization function. The metrics initialization must be passed down and invoked in the `MetricsLayerService`'s, so that entries are appended and closed after each request when the metrics guard is dropped. In a future version, we may be able to implement manual appending and closing of entries to enable users to pass an instance of the metrics struct itself rather than a function that returns an `AppendAndCloseOnDrop`.

- Default type parameter of `fn() -> AppendAndCloseOnDrop<E, S>`

`Rq: RequestMetrics<E>`

- Closure trait bound to allow users to set metrics however they like from the request object, which will be invoked in the `MetricsLayerService` after the metrics have been initialized. The `Request` parameter needs to be a mutable reference so adding to the request extensions is possible.

- Default type parameter of `fn(&mut Request<ReqBody>, &mut E)`

`Rs: ResponseMetrics<E>`

- Closure trait bound to allow users to set metrics however they like from the response object, which will be invoked in the `MetricsLayerService` after the metrics have been initialized.

- Default type parameter of `fn(&Response<ResBody>, &mut E)`

#### Will have the following fields:

```rust
init_metrics: I,
request_metrics: Option<Rq>,
response_metrics: Option<Rs>,
default_req_metrics_extension_fn: fn(&mut Request<ReqBody>, &mut E, DefaultRequestMetricsConfig),
default_res_metrics_extension_fn: fn(&mut Response<ResBody>, &mut E, DefaultResponseMetricsConfig),
default_req_metrics_config: DefaultRequestMetricsConfig,
default_res_metrics_config: DefaultResponseMetricsConfig,
```

#### Will have the following implementations:

- A fully generic implementation with a `builder()` method that returns a `MetricsLayerBuilder`.

- Implements the tower `Layer` trait to map to the `MetricsLayerService`

#### This will allow users the following construction experiences:

`MetricsLayer::builder()`

- For a builder with a required metrics initialization and optional configuration for default metrics inclusion, setting custom request/response metrics, etc

### `MetricsLayerBuilder` struct

A typestate builder for `MetricsLayer` containing the same trait bounds. The `build` implementations for custom metrics type will be generated by the proc macro, which will set the `default_req_metrics_extension_fn` and `default_res_metrics_extension_fn` to add the default req/res metrics extensions to the `DefaultMetricsPlugin`.

The states will be:

`NeedsInitialization`

- Exposes `init_metrics` method so an initialization closure can be provided.

- Exposes `try_init_with_defaults` with default metrics initialization using metrique's application-wide global entry sink [`metrique::ServiceMetrics`], returning an error if a sink has not been attached.

`WithDefaults`

A state where the fields for setting metrics from the req/res objects are `None`, using the default function pointer types as concrete type placeholders.

- Exposes methods for disabling any/all of the default metrics.

- Exposes methods for taking Fn closures for setting metrics from req and res objects.

- Exposes build method.

`WithRq`

A state where the field for setting metrics from the req object has been set.

- Exposes methods for disabling any/all of the default metrics.

- Exposes method for taking Fn closures for setting metrics from res objects.

- Exposes build method.

`WithRs`

A state where the field for setting metrics from the res object has been set.

- Exposes methods for disabling any/all of the default metrics.

- Exposes method for taking Fn closures for setting metrics from req objects.

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

### `extension::Metrics` struct

A wrapper struct for extensions that are inserted as a result of the `#[smithy_metrics(extension)]` attribute macro on a struct field. An `Arc<Mutex<SlotGuard<X>>>>` will be used to wrap any extensions to be forward compatible with the Clone bound introduced in http 1.x's (smithy-rs currently is on http 0.2, but the http 1.x work is underway and soon to be complete). This struct will provide User's an API where they do not have to be exposed to this complicated type.

### `#[smithy_metrics]` attribute proc macro

A proc macro that can be placed on a metrique metrics struct for the adding of default metrics fields and the expansion of a `MetricsLayerBuilder` implementation for the annotated struct.

This will also come with `#[smithy_metrics(extension)]` to mark struct fields for insertion to the request extensions to be used in custom middleware or operation handlers.

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

    - [x] `WithRs` state implementation

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

- [x] Define and implement struct `extension::Metrics`

- [x] Implement proc macro attribute `#[smithy_metrics]`

    - [x] Implement expansion to wrap the types of fields annotated with `#[smithy_metrics(extension)]` with `Slotguard` if the user hasn't explicitly done so, or show an error message telling them to

    - [x] Implement expansion of a `MetricsLayerBuilder` implementation for receiving metrics type containing a `build` method

- [x] Create proc macro attribute `#[smithy_metrics(extension)]` on fields of a metrics struct to give users the ability to annotate the fields they want to be accessible from the request extensions down the line, such as in custom middleware or operation handlers
