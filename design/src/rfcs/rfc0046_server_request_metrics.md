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

Users will be able to add default metrics to their service like this:

```rust
fn main() {
    let metrics_layer = MetricsLayer::new();

    let app = PokemonService::builder(config)
        .get_pokemon_species(get_pokemon_species)
        .build()
        .expect("failed to build an instance of PokemonService");
    
    let service = metrics_layer.layer(app);
}
```

For configuration of things like initialization, opting out, overriding how certain default metrics are set, etc, a builder will be provided:

```rust
fn main() {
    let config = MetricsLayerConfig::builder()
        .without_start_metric()
        .build();
    let metrics_layer = MetricsLayer::builder(config)
        .init_metrics(|| { DefaultMetrics::default().append_on_drop(custom_sink) })
        .build();
}
```

To define additional metrics or add to the request extensions on top of the defaults:

```rust
fn main() {
    let metrics_layer: MetricsLayer<MyMetrics> = MetricsLayer::builder(MetricsLayerConfig::default())
        .set_request_metrics(set_request_metrics)
        .build();
}

#[smithy_metrics]
#[metrics]
struct MyMetrics {
    #[smithy_metrics(extension)]
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

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

There will be two new rust-runtime crates `aws-smithy-http-server-metrics` and `aws-smithy-http-server-metrics-macro`, with types re-exported in the generated server.

### `MetricsLayer` struct

The focal struct that users can add to their service as a tower `Layer` for metrics, containing `new` and `builder` methods to get a `MetricsLayer` with the default configuration or a `MetricsLayerBuilder`, respectively. Because the `MetricsLayer` is specific to the struct provided in the type parameter, the `MetricsLayerBuilder` will be a product of the `#[smithy_metrics]` proc macro expansion, responsible for providing methods to customize things like the metrics initialization and setting custom request/response metrics. Contrarily, the `MetricsLayerConfig` along with its builder will be explicitly defined for general configuration not bound to any specific type parameter, such as enabling/disabling default metrics.

Contains a generic type parameter bound by a marker trait with a default of `DefaultMetrics`, which will be a type defined in the library that uses the `#[smithy_metrics]` expansion.

This will allow users the following construction experiences:

- `MetricsLayer::new()` for the default metrics and extensions

- `MetricsLayer::<CustomMetrics>::new()` for the default metrics with potential renaming or additional extensions from attribute proc macros

- `MetricsLayer::builder(config)` for a builder to configure things like metrics initialization, how default metrics are set from the request/response objects, etc

- `MetricsLayer::<CustomMetrics>::builder` for a builder with the ability to set their custom-defined metrics as well

### `MetricsLayerConfig` struct

Along with `MetricsLayerConfigBuilder`, structs for the general configuration of constructing a `MetricsLayer` not bound to any specific metrics type. This will include things like enabling/disabling default metrics.

### `MetricsLayerService` struct

A tower service for metrics that will contain the core logic for setting the metrics from the request/response objects.

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

A proc macro that can be placed on a metrique metrics struct for purposes such as the addition of default request/response metrics fields, the implementation of a marker trait, and the expansion of a `MetricsLayerBuilder` with concrete type being the annotated struct.

This will also come with `#[smithy_metrics(rename(x = "y"))]` to rename default fields and `#[smithy_metrics(extension)]` to mark struct fields for insertion to the request extensions to be used in custom middleware or operation handlers.

<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [] Create `rust-runtime` crates `aws-smithy-http-server-metrics` and `aws-smithy-http-server-metrics-macro`

- [] Implement struct `MetricsLayer<T>`

    - [] Implement `new` and `builder` methods

- [] Implement struct `MetricsLayerBuilder<T>`

    - [] Implement method for custom metrics initialization

    - [] Implement methods for setting custom metrics from request and response objects

- [] Implement struct `MetricsLayerConfig`

    - [] Implement default with out-of-the-box metrics enabled

- [] Implement struct `MetricsLayerConfigBuilder`
    
    - [] Implement methods to opt out of all or individual default request/response metrics

- [] Implement struct `MetricsPlugin`

    - [] Implement HTTPMarker

    - [] Implement `Plugin` to map to `MetricsService`

- [] Implement struct `MetricsService` to contain the tower service logic for invoking the passed functions for setting request/response metrics and adding to the request extensions

    - [] Implement `Clone`

    - [] Implement tower `Service` to contain logic for:

        - [] Initializing the metrics using the initialization function if passed, `Default` and a global entry sink, or emitting a compiler error saying one of these two must be satisfied

        - [] Calling the request/response metrics handlers

- [] Implement struct `DefaultMetrics` to be a unit struct with the `#[smithy_metrics]` attribute to expand a builder with just the default metrics

- [] Implement struct `DefaultRequestMetrics` to be the type of a field that gets added to a `#[smithy_metrics]`-annotated struct to add the default request metrics to

- [] Implement struct `DefaultResponseMetrics`to be the type of a field that gets added to a `#[smithy_metrics]`-annotated struct to add the default response metrics to

- [] Implement proc macro attribute `#[smithy_metrics]`

    - [] Implement expansion for implementing a marker struct

    - [] Implement expansion to wrap the types of fields annotated with `#[smithy_metrics(extension)]` with `Slotguard` if the user hasn't explicitly done so, or show an error message telling them to

    - [] Implement expansion to define and implement a `MetricsLayerBuilder` with the concrete type parameter being the annotated struct with methods to:
        
        - [] Pass a custom metrics initialization function

        - [] Pass custom request/response setter functions

- [] Create proc macro attribute `#[smithy_metrics(extension)]` on fields of a metrics struct to give users the ability to annotate the fields they want to be accessible from the request extensions down the line, such as in custom middleware or operation handlers

- [] Create proc macro attribute `#[smithy_metrics(rename(default_metric_name = "custom_name"))]` to give users the ability to rename default metrics

- [] Create a type alias for extension type containing the slotguards of the types from the annotated fields of the metrics struct, e.g. `type OperationMetricsExtension = Extension<SlotGuard<OperationMetrics>>`

- [] In the server codegen, apply the metrics plugin in all operation setters
