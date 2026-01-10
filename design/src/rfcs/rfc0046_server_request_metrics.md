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

Users will be able to add default metrics to their service like below, an easy way to get out-of-the-box metrics:

```rust
fn main() {
    let app = PokemonService::builder(config)
        ... // set operation handlers
        .build()
        .unwrap()

    let service = MetricsLayer::new().layer(app);
}
```

For further configuration, a builder will be provided that takes an initialization function that allows the user to initialize an `AppendAndCloseOnDrop` with a custom metrics struct and sink.

```rust
fn main() {
    let metrics_layer = MetricsLayer::builder()
        .init_metrics(|| MyMetrics::default().append_on_drop(my_sink))
        .set_request_metrics(set_request_metrics)
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
    get_pokemon_species_metrics: Option<String>
}

fn set_request_metrics(req: Request<Body>, metrics: MyMetricsGuard) {
    req.custom_metric = ...;
}
```

For metrics control in user-defined operation handlers, the types of fields marked with `#[smithy_metrics::extension]` will be available in the request extensions. To make this turnkey, a type alias will be made for any of the `#[smithy_metrics::extension]` annotated fields' types. In this case `OperationMetricsExtension` will be `Extension<SlotGuard<OperationMetrics>>`, which can be added as a parameter in the handler signature as shown below.

```rust
fn main() {
    let app = PokemonService::builder(config)
        .get_pokemon_species(get_pokemon_species)
        .build()
        .unwrap()

    let service = MetricsLayer::new().layer(app);
}

pub async fn get_pokemon_species(
    input: input::GetPokemonSpeciesInput,
    state: Extension<Arc<State>>,
    Extension(mut metrics): OperationMetricsExtension
) -> Result<output::GetPokemonSpeciesOutput, error::GetPokemonSpeciesError> {
    ...
}
```

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

There will be two new rust-runtime crates `aws-smithy-http-server-metrics` and `aws-smithy-http-server-metrics-macro`, with types re-exported in the generated server.

### `MetricsLayer` struct

The focal struct that users can add to their service as a tower `Layer` for metrics, containing `new` and `builder` methods to get a `MetricsLayer` with the default configuration or a `MetricsLayerBuilder`, respectively.

#### Will have following generics and trait bounds:

`E: CloseEntry + Send + Sync + 'static`

- For the metrique metrics struct (i.e. annotated with #[metrics]).

- Default type parameter of `DefaultMetrics`

`S: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static`

- For the entry sink where entries are stored in an in-memory buffer until they can be written to the destination.

- Default type parameter of `DefaultSink` (BoxEntrySink)

`I: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static`

- Closure trait bound for the metrics initialization function. The metrics initialization must be passed down and invoked in the `MetricsLayerService`'s, so that entries are appended and closed after each request when the metrics guard is dropped. In a future version, we may be able to implement manual appending and closing of entries to enable users to pass an instance of the metrics struct itself rather than a function that returns an `AppendAndCloseOnDrop`.

- Default type parameter of `fn() -> AppendAndCloseOnDrop<E, S>`

`Rq: Fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static`

- Closure trait bound to allow users to set metrics however they like from the request object, which will be invoked in the `MetricsLayerService` after the metrics have been initialized. The `Request` parameter needs to be a mutable reference so adding to the request extensions is possible.

- Default type parameter of `fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>)`

`Rs: Fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>) + Clone + Send + Sync + 'static`

- Closure trait bound to allow users to set metrics however they like from the response object, which will be invoked in the `MetricsLayerService` after the metrics have been initialized.

- Default type parameter of `fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>)`

#### Will have the following fields:

```rust
init_metrics: I,
set_default_request_metrics: Option<fn(&mut Request<ReqBody>, &mut AppendAndCloseOnDrop<E, S>)>,
set_default_response_metrics: Option<fn(&Response<ResBody>, &mut AppendAndCloseOnDrop<E, S>)>,
set_request_metrics: Option<Rq>,
set_response_metrics: Option<Rs>,
```

#### Will have the following implementations:

- A fully generic implementation with a `builder()` method that returns a `MetricsLayerBuilder`.

- An implementation using the default type parameters with a `new()` method that returns a fully default `MetricsLayer` using a global sink.

- Implements the tower `Layer` trait to map to the `MetricsLayerService`

#### This will allow users the following construction experiences:

`MetricsLayer::new()`

- For the complete default experience, being the out-of-the-box default metrics.

`MetricsLayer::builder()`

- For a builder with a required metrics initialization and optional configuration for default metrics inclusion, setting custom request/response metrics, etc

### `MetricsLayerBuilder` struct

A typestate builder for `MetricsLayer`. The states will be

`NeedsInitialization`

- Exposes a single `init_metrics` method so an initialization closure can be provided.

`Ready`

- Exposes methods for disabling any/all of the default metrics, and methods for taking closures for setting metrics from req/res objects.

An implementation will be made for metrics structs annotated with the `#[smithy_metrics]` proc macro that exposes a `build` method.

### `MetricsLayerService` struct

A tower service for metrics that will contain the core logic for invoking the logic for metrics initialization and setting the metrics from the request/response objects. The `call` implementation will essentially invoke the passed closures.

### `MetricsPlugin` struct

An HTTP plugin metrics struct to allow for default operation-level metrics.

The plugin system will mitigate the need to define a layer per operation for metrics like the operation name. A Metrics Plugin will be implemented to set the operation and service metrics from the `Operation` and `Service` type parameters. It will be applied to the service in the operation setters, which will require a change in the server codegen.

### `MetricsPluginService` struct

A tower service for metrics that will contain the logic for setting the default operation-specific metrics.

### `DefaultMetrics` struct

A struct for default metrics. This will use the `#[smithy_metrics]` and be used as the default type parameter for `MetricsLayer`.

### `DefaultRequestMetrics` struct

A struct that will contain fields for the default request metrics, a field for which will be added to a struct containing the `#[smithy_metrics]` annotation.

### `DefaultResponseMetrics` struct

A struct that will contain fields for the default response metrics, a field for which will be added to a struct containing the `#[smithy_metrics]` annotation.

### `#[smithy_metrics]` attribute proc macro

A proc macro that can be placed on a metrique metrics struct for the adding of default metrics fields and the expansion of a `MetricsLayerBuilder` implementation for the annotated struct.

This will also come with `#[smithy_metrics(rename(x = "y"))]` to rename default fields and `#[smithy_metrics::extension]` (or `#[smithy_metrics::extension(response)]` to be explicit about whether it should be a request or response extension) to mark struct fields for insertion to the request extensions to be used in custom middleware or operation handlers.

<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [] Create `rust-runtime` crates `aws-smithy-http-server-metrics` and `aws-smithy-http-server-metrics-macro`

- [] Implement struct `MetricsLayer`

    - [] Define struct with generics, trait bounds, and default type parameters

    - [] Generic implementation with `builder()` method

    - [] Default implementation with `new()` method

    - [] Layer implementation to map to `MetricsLayerService`

- [] Implement struct `MetricsLayerBuilder`

    - [] Define struct with generics and trait bounds for metrics and typestate pattern

    - [] `NeedsInitialization` state generic implementation with `init_metrics()` method

    - [] `Ready` state generic implementation with builder methods to disable any/all default metrics and passing closures to set metrics from req/res objects.

    - [] Implementation of `build` for `DefaultMetrics` (may be replaced with the proc macro expansion when that is done)

- [] Implement struct `MetricsPlugin`

    - [] Implement HTTPMarker

    - [] Implement `Plugin` to map to `MetricsService`

- [] Implement struct `MetricsLayerService` to contain the tower service logic

    - [] Implement `Clone`

    - [] Implement tower `Service` to contain logic for:

        - [] Initializing the metrics using the initialization function if passed, `Default` and a global entry sink, or emitting a compiler error saying one of these two must be satisfied

        - [] Calling the request/response metrics handlers

- [] Define struct `DefaultMetrics` with a request and response metrics field of the types `DefaultRequestMetrics` and `DefaultResponseMetrics`, respectively

- [] Define struct `DefaultRequestMetrics` to contain the out-of-the-box request metrics

- [] Define struct `DefaultResponseMetrics`to contain the out-of-the-box response metrics

- [] Implement proc macro attribute `#[smithy_metrics]`

    - [] Implement expansion to wrap the types of fields annotated with `#[smithy_metrics(extension)]` with `Slotguard` if the user hasn't explicitly done so, or show an error message telling them to

    - [] Implement expansion of a `MetricsLayerBuilder` implementation for receiving metrics type containing a `build` method

- [] Create proc macro attribute `#[smithy_metrics(extension)]` on fields of a metrics struct to give users the ability to annotate the fields they want to be accessible from the request extensions down the line, such as in custom middleware or operation handlers

- [] Create proc macro attribute `#[smithy_metrics(rename(default_metric_name = "custom_name"))]` to give users the ability to rename default metrics

- [] Create a type alias for extension type containing the slotguards of the types from the annotated fields of the metrics struct, e.g. `type OperationMetricsExtension = Extension<SlotGuard<OperationMetrics>>`

- [] In the server codegen, apply the metrics plugin in all operation setters

- [] Replace the manual `MetricsLayerBuilder` implementation for `DefaultMetrics` with the proc macro expansion
