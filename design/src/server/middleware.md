# Middleware

The following document provides a brief survey of the various positions middleware can be inserted in Smithy Rust.

We use the [Pokémon service](https://github.com/smithy-lang/smithy-rs/blob/main/codegen-core/common-test-models/pokemon.smithy) as a reference model throughout.

```smithy
/// A Pokémon species forms the basis for at least one Pokémon.
@title("Pokémon Species")
resource PokemonSpecies {
    identifiers: {
        name: String
    },
    read: GetPokemonSpecies,
}

/// A users current Pokémon storage.
resource Storage {
    identifiers: {
        user: String
    },
    read: GetStorage,
}

/// The Pokémon Service allows you to retrieve information about Pokémon species.
@title("Pokémon Service")
@restJson1
service PokemonService {
    version: "2021-12-01",
    resources: [PokemonSpecies, Storage],
    operations: [
        GetServerStatistics,
        DoNothing,
        CapturePokemon,
        CheckHealth
    ],
}
```

## Introduction to Tower

Smithy Rust is built on top of [`tower`](https://github.com/tower-rs/tower).

> Tower is a library of modular and reusable components for building robust networking clients and servers.

The `tower` library is centered around two main interfaces, the [`Service`](https://docs.rs/tower/latest/tower/trait.Service.html) trait and the [`Layer`](https://docs.rs/tower/latest/tower/trait.Layer.html) trait.

The `Service` trait can be thought of as an asynchronous function from a request to a response, `async fn(Request) -> Result<Response, Error>`, coupled with a mechanism to [handle back pressure](https://docs.rs/tower/latest/tower/trait.Service.html#backpressure), while the `Layer` trait can be thought of as a way of decorating a `Service`, transforming either the request or response.

Middleware in `tower` typically conforms to the following pattern, a `Service` implementation of the form

```rust
pub struct NewService<S> {
    inner: S,
    /* auxillary data */
}
```

and a complementary

```rust,ignore
# extern crate tower;
# pub struct NewService<S> { inner: S }
use tower::{Layer, Service};

pub struct NewLayer {
    /* auxiliary data */
}

impl<S> Layer<S> for NewLayer {
    type Service = NewService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        NewService {
            inner,
            /* auxiliary fields */
        }
    }
}
```

The `NewService` modifies the behavior of the inner `Service` `S` while the `NewLayer` takes auxiliary data and constructs `NewService<S>` from `S`.

Customers are then able to stack middleware by composing `Layer`s using combinators such as [`ServiceBuilder::layer`](https://docs.rs/tower/latest/tower/struct.ServiceBuilder.html#method.layer) and [`Stack`](https://docs.rs/tower/latest/tower/layer/util/struct.Stack.html).

<!-- TODO(Update documentation): There's a `Layer` implementation on tuples about to be merged, give it as an example here. -->

## Applying Middleware

One of the primary goals is to provide configurability and extensibility through the application of middleware. The customer is able to apply `Layer`s in a variety of key places during the request/response lifecycle. The following schematic labels each configurable middleware position from A to D:

```mermaid
stateDiagram-v2
    state in <<fork>>
    state "GetPokemonSpecies" as C1
    state "GetStorage" as C2
    state "DoNothing" as C3
    state "..." as C4
    direction LR
    [*] --> in : HTTP Request
    UpgradeLayer --> [*]: HTTP Response
    state A {
        state PokemonService {
            state RoutingService {
                in --> UpgradeLayer: HTTP Request
                in --> C2: HTTP Request
                in --> C3: HTTP Request
                in --> C4: HTTP Request
                state B {
                    state C1 {
                        state C {
                            state UpgradeLayer {
                                direction LR
                                [*] --> Handler: Model Input
                                Handler --> [*] : Model Output
                                state D {
                                    Handler
                                }
                            }
                        }
                    }
                    C2
                    C3
                    C4
                }
            }
        }
    }
    C2 --> [*]: HTTP Response
    C3 --> [*]: HTTP Response
    C4 --> [*]: HTTP Response
```

where `UpgradeLayer` is the `Layer` converting Smithy model structures to HTTP structures and the `RoutingService` is responsible for routing requests to the appropriate operation.

### A. Outer Middleware

The output of the Smithy service builder provides the user with a `Service<http::Request, Response = http::Response>` implementation. A `Layer` can be applied around the entire `Service`.

```rust,ignore
# extern crate aws_smithy_http_server;
# extern crate pokemon_service_server_sdk;
# extern crate tower;
# use std::time::Duration;
# struct TimeoutLayer;
# impl TimeoutLayer { fn new(t: Duration) -> Self { Self }}
# impl<S> Layer<S> for TimeoutLayer { type Service = S; fn layer(&self, svc: S) -> Self::Service { svc } }
# use pokemon_service_server_sdk::{input::*, output::*, error::*};
# let handler = |req: GetPokemonSpeciesInput| async { Result::<GetPokemonSpeciesOutput, GetPokemonSpeciesError>::Ok(todo!()) };
# use aws_smithy_http_server::protocol::rest_json_1::{RestJson1, router::RestRouter};
# use aws_smithy_http_server::routing::{Route, RoutingService};
use pokemon_service_server_sdk::{PokemonServiceConfig, PokemonService};
use tower::Layer;

let config = PokemonServiceConfig::builder().build();

// This is a HTTP `Service`.
let app = PokemonService::builder(config)
    .get_pokemon_species(handler)
    /* ... */
    .build()
    .unwrap();
# let app: PokemonService<RoutingService<RestRouter<Route>, RestJson1>>  = app;

// Construct `TimeoutLayer`.
let timeout_layer = TimeoutLayer::new(Duration::from_secs(3));

// Apply a 3 second timeout to all responses.
let app = timeout_layer.layer(app);
```

### B. Route Middleware

A _single_ layer can be applied to _all_ routes inside the `Router`. This
exists as a method on the `PokemonServiceConfig` builder object, which is passed into the
service builder.

```rust,ignore
# extern crate tower;
# extern crate pokemon_service_server_sdk;
# extern crate aws_smithy_http_server;
# use tower::{util::service_fn, Layer};
# use std::time::Duration;
# use aws_smithy_http_server::protocol::rest_json_1::{RestJson1, router::RestRouter};
# use aws_smithy_http_server::routing::{Route, RoutingService};
# use pokemon_service_server_sdk::{input::*, output::*, error::*};
# let handler = |req: GetPokemonSpeciesInput| async { Result::<GetPokemonSpeciesOutput, GetPokemonSpeciesError>::Ok(todo!()) };
# struct MetricsLayer;
# impl MetricsLayer { pub fn new() -> Self { Self } }
# impl<S> Layer<S> for MetricsLayer { type Service = S; fn layer(&self, svc: S) -> Self::Service { svc } }
use pokemon_service_server_sdk::{PokemonService, PokemonServiceConfig};

// Construct `MetricsLayer`.
let metrics_layer = MetricsLayer::new();

let config = PokemonServiceConfig::builder().layer(metrics_layer).build();

let app = PokemonService::builder(config)
    .get_pokemon_species(handler)
    /* ... */
    .build()
    .unwrap();
# let app: PokemonService<RoutingService<RestRouter<Route>, RestJson1>>  = app;
```

Note that requests pass through this middleware immediately _after_ routing succeeds and therefore will _not_ be encountered if routing fails. This means that the [TraceLayer](https://docs.rs/tower-http/latest/tower_http/trace/struct.TraceLayer.html) in the example above does _not_ provide logs unless routing has completed. This contrasts to [middleware A](#a-outer-middleware), which _all_ requests/responses pass through when entering/leaving the service.

### C. Operation Specific HTTP Middleware

A "HTTP layer" can be applied to specific operations.

```rust,ignore
# extern crate tower;
# extern crate pokemon_service_server_sdk;
# extern crate aws_smithy_http_server;
# use tower::{util::service_fn, Layer};
# use std::time::Duration;
# use pokemon_service_server_sdk::{operation_shape::GetPokemonSpecies, input::*, output::*, error::*};
# use aws_smithy_http_server::protocol::rest_json_1::{RestJson1, router::RestRouter};
# use aws_smithy_http_server::routing::{Route, RoutingService};
# use aws_smithy_http_server::{operation::OperationShapeExt, plugin::*, operation::*};
# let handler = |req: GetPokemonSpeciesInput| async { Result::<GetPokemonSpeciesOutput, GetPokemonSpeciesError>::Ok(todo!()) };
# struct LoggingLayer;
# impl LoggingLayer { pub fn new() -> Self { Self } }
# impl<S> Layer<S> for LoggingLayer { type Service = S; fn layer(&self, svc: S) -> Self::Service { svc } }
use pokemon_service_server_sdk::{PokemonService, PokemonServiceConfig, scope};

scope! {
    /// Only log on `GetPokemonSpecies` and `GetStorage`
    struct LoggingScope {
        includes: [GetPokemonSpecies, GetStorage]
    }
}

// Construct `LoggingLayer`.
let logging_plugin = LayerPlugin(LoggingLayer::new());
let logging_plugin = Scoped::new::<LoggingScope>(logging_plugin);
let http_plugins = HttpPlugins::new().push(logging_plugin);

let config = PokemonServiceConfig::builder().http_plugin(http_plugins).build();

let app = PokemonService::builder(config)
    .get_pokemon_species(handler)
    /* ... */
    .build()
    .unwrap();
# let app: PokemonService<RoutingService<RestRouter<Route>, RestJson1>>  = app;
```

This middleware transforms the operations HTTP requests and responses.

### D. Operation Specific Model Middleware

A "model layer" can be applied to specific operations.

```rust,ignore
# extern crate tower;
# extern crate pokemon_service_server_sdk;
# extern crate aws_smithy_http_server;
# use tower::{util::service_fn, Layer};
# use pokemon_service_server_sdk::{operation_shape::GetPokemonSpecies, input::*, output::*, error::*};
# let handler = |req: GetPokemonSpeciesInput| async { Result::<GetPokemonSpeciesOutput, GetPokemonSpeciesError>::Ok(todo!()) };
# use aws_smithy_http_server::{operation::*, plugin::*};
# use aws_smithy_http_server::protocol::rest_json_1::{RestJson1, router::RestRouter};
# use aws_smithy_http_server::routing::{Route, RoutingService};
# struct BufferLayer;
# impl BufferLayer { pub fn new(size: usize) -> Self { Self } }
# impl<S> Layer<S> for BufferLayer { type Service = S; fn layer(&self, svc: S) -> Self::Service { svc } }
use pokemon_service_server_sdk::{PokemonService, PokemonServiceConfig, scope};

scope! {
    /// Only buffer on `GetPokemonSpecies` and `GetStorage`
    struct BufferScope {
        includes: [GetPokemonSpecies, GetStorage]
    }
}

// Construct `BufferLayer`.
let buffer_plugin = LayerPlugin(BufferLayer::new(3));
let buffer_plugin = Scoped::new::<BufferScope>(buffer_plugin);
let config = PokemonServiceConfig::builder().model_plugin(buffer_plugin).build();

let app = PokemonService::builder(config)
    .get_pokemon_species(handler)
    /* ... */
    .build()
    .unwrap();
# let app: PokemonService<RoutingService<RestRouter<Route>, RestJson1>>  = app;
```

In contrast to [position C](#c-operation-specific-http-middleware), this middleware transforms the operations modelled inputs to modelled outputs.

## Plugin System

Suppose we want to apply a different `Layer` to every operation. In this case, position B (`PokemonService::layer`) will not suffice because it applies a single `Layer` to all routes and while position C (`Operation::layer`) would work, it'd require the customer constructs the `Layer` by hand for every operation.

Consider the following middleware:

```rust,ignore
# extern crate aws_smithy_http_server;
# extern crate tower;
use aws_smithy_http_server::shape_id::ShapeId;
use std::task::{Context, Poll};
use tower::Service;

/// A [`Service`] that adds a print log.
pub struct PrintService<S> {
    inner: S,
    operation_id: ShapeId,
    service_id: ShapeId
}

impl<R, S> Service<R> for PrintService<S>
where
    S: Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        println!("Hi {} in {}", self.operation_id.name(), self.service_id.name());
        self.inner.call(req)
    }
}
```

The plugin system provides a way to construct then apply `Layer`s in position [C](#c-operation-specific-http-middleware) and [D](#d-operation-specific-model-middleware), using the [protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/index.html) and [operation shape](https://awslabs.github.io/smithy/2.0/spec/service-types.html#service-operations) as parameters.

An example of a `PrintPlugin` which prints the operation name:

```rust,no_run
# extern crate aws_smithy_http_server;
# use aws_smithy_http_server::shape_id::ShapeId;
# pub struct PrintService<S> { inner: S, operation_id: ShapeId, service_id: ShapeId }
use aws_smithy_http_server::{plugin::Plugin, operation::OperationShape, service::ServiceShape};

/// A [`Plugin`] for a service builder to add a [`PrintService`] over operations.
#[derive(Debug)]
pub struct PrintPlugin;

impl<Ser, Op, T> Plugin<Ser, Op, T> for PrintPlugin
where
    Ser: ServiceShape,
    Op: OperationShape,
{
    type Output = PrintService<T>;

    fn apply(&self, inner: T) -> Self::Output {
        PrintService {
            inner,
            operation_id: Op::ID,
            service_id: Ser::ID,
        }
    }
}
```

You can provide a custom method to add your plugin to a collection of  `HttpPlugins` or `ModelPlugins` via an extension trait. For example, for `HttpPlugins`:

```rust,no_run
# extern crate aws_smithy_http_server;
# pub struct PrintPlugin;
# impl aws_smithy_http_server::plugin::HttpMarker for PrintPlugin { }
use aws_smithy_http_server::plugin::{HttpPlugins, PluginStack};

/// This provides a [`print`](PrintExt::print) method on [`HttpPlugins`].
pub trait PrintExt<ExistingPlugins> {
    /// Causes all operations to print the operation name when called.
    ///
    /// This works by applying the [`PrintPlugin`].
    fn print(self) -> HttpPlugins<PluginStack<PrintPlugin, ExistingPlugins>>;
}

impl<ExistingPlugins> PrintExt<ExistingPlugins> for HttpPlugins<ExistingPlugins> {
    fn print(self) -> HttpPlugins<PluginStack<PrintPlugin, ExistingPlugins>> {
        self.push(PrintPlugin)
    }
}
```

This allows for:

```rust,no_run
# extern crate pokemon_service_server_sdk;
# extern crate aws_smithy_http_server;
# use aws_smithy_http_server::plugin::{PluginStack, Plugin};
# struct PrintPlugin;
# impl<Ser, Op, T> Plugin<Ser, Op, T> for PrintPlugin { type Output = T; fn apply(&self, svc: T) -> Self::Output { svc }}
# impl aws_smithy_http_server::plugin::HttpMarker for PrintPlugin { }
# trait PrintExt<EP> { fn print(self) -> HttpPlugins<PluginStack<PrintPlugin, EP>>; }
# impl<EP> PrintExt<EP> for HttpPlugins<EP> { fn print(self) -> HttpPlugins<PluginStack<PrintPlugin, EP>> { self.push(PrintPlugin) }}
# use pokemon_service_server_sdk::{operation_shape::GetPokemonSpecies, input::*, output::*, error::*};
# let handler = |req: GetPokemonSpeciesInput| async { Result::<GetPokemonSpeciesOutput, GetPokemonSpeciesError>::Ok(todo!()) };
# use aws_smithy_http_server::protocol::rest_json_1::{RestJson1, router::RestRouter};
# use aws_smithy_http_server::routing::{Route, RoutingService};
use aws_smithy_http_server::plugin::{IdentityPlugin, HttpPlugins};
use pokemon_service_server_sdk::{PokemonService, PokemonServiceConfig};

let http_plugins = HttpPlugins::new()
    // [..other plugins..]
    // The custom method!
    .print();
let config = PokemonServiceConfig::builder().http_plugin(http_plugins).build();
let app /* : PokemonService<Route<B>> */ = PokemonService::builder(config)
    .get_pokemon_species(handler)
    /* ... */
    .build()
    .unwrap();
# let app: PokemonService<RoutingService<RestRouter<Route>, RestJson1>>  = app;
```

The custom `print` method hides the details of the `Plugin` trait from the average consumer.
They interact with the utility methods on `HttpPlugins` and enjoy the self-contained documentation.
