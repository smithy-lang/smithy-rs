# The Anatomy of a Service

What is [Smithy](https://awslabs.github.io/smithy/2.0/index.html)? At a high-level, it's a grammar for specifying services while leaving the business logic undefined. A [Smithy Service](https://awslabs.github.io/smithy/2.0/spec/service-types.html#service) specifies a collection of function signatures in the form of [Operations](https://awslabs.github.io/smithy/2.0/spec/service-types.html#operation), their purpose is to encapsulate business logic. A Smithy implementation should, for each Smithy Service, provide a builder, which accepts functions conforming to said signatures, and returns a service subject to the semantics specified by the model.

This survey is disinterested in the actual Kotlin implementation of the code generator, and instead focuses on the structure of the generated Rust code and how it relates to the Smithy model. The intended audience is new contributors and users interested in internal details.

During the survey we will use the [`pokemon.smithy`](https://github.com/awslabs/smithy-rs/blob/main/codegen-core/common-test-models/pokemon.smithy) model as a reference:

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

Smithy Rust will use this model to produce the following API:

```rust,ignore
// A handler for the `GetPokemonSpecies` operation (the `PokemonSpecies` resource).
async fn get_pokemon_species(input: GetPokemonSpeciesInput) -> Result<GetPokemonSpeciesOutput, GetPokemonSpeciesError> {
    /* implementation */
}

// Use the service builder to create `PokemonService`.
let pokemon_service = PokemonService::builder_without_plugins()
    // Pass the handler directly to the service builder...
    .get_pokemon_species(get_pokemon_species)
    // ...or pass the layered handler.
    .get_pokemon_species_operation(get_pokemon_species_op)
    /* other operation setters */
    .build()
    .expect("failed to create an instance of the Pokémon service");
```

## Operations

A [Smithy Operation](https://awslabs.github.io/smithy/2.0/spec/service-types.html#operation) specifies the input, output, and possible errors of an API operation. One might characterize a Smithy Operation as syntax for specifying a function type.

We represent this in Rust using the [`OperationShape`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/operation/trait.OperationShape.html) trait:

```rust,ignore
pub trait OperationShape {
    /// The name of the operation.
    const NAME: &'static str;

    /// The operation input.
    type Input;
    /// The operation output.
    type Output;
    /// The operation error. [`Infallible`](std::convert::Infallible) in the case where no error
    /// exists.
    type Error;
}
```

For each Smithy Operation shape,

```smithy
/// Retrieve information about a Pokémon species.
@readonly
@http(uri: "/pokemon-species/{name}", method: "GET")
operation GetPokemonSpecies {
    input: GetPokemonSpeciesInput,
    output: GetPokemonSpeciesOutput,
    errors: [ResourceNotFoundException],
}
```

the following implementation is generated

```rust,ignore
/// Retrieve information about a Pokémon species.
pub struct GetPokemonSpecies;

impl OperationShape for GetPokemonSpecies {
    const NAME: &'static str = "com.aws.example#GetPokemonSpecies";

    type Input = GetPokemonSpeciesInput;
    type Output = GetPokemonSpeciesOutput;
    type Error = GetPokemonSpeciesError;
}
```

where `GetPokemonSpeciesInput`, `GetPokemonSpeciesOutput` are both generated from the Smithy structures and `GetPokemonSpeciesError` is an enum generated from the `errors: [ResourceNotFoundException]`.

Note that the `GetPokemonSpecies` marker structure is a zero-sized type (ZST), and therefore does not exist at runtime - it is a way to attach operation-specific data on an entity within the type system.

The following nomenclature will aid us in our survey. We describe a `tower::Service` as a "model service" if its request and response are Smithy structures, as defined by the `OperationShape` trait - the `GetPokemonSpeciesInput`, `GetPokemonSpeciesOutput`, and `GetPokemonSpeciesError` described above. Similarly, we describe a `tower::Service` as a "HTTP service" if its request and response are [`http`](https://github.com/hyperium/http) structures - `http::Request` and `http::Response`.

The constructors exist on the marker ZSTs as an extension trait to `OperationShape`, namely [`OperationShapeExt`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/operation/trait.OperationShapeExt.html):

```rust,ignore
/// An extension trait over [`OperationShape`].
pub trait OperationShapeExt: OperationShape {
    /// Creates a new [`Service`] for well-formed [`Handler`]s.
    fn from_handler<H>(handler: H) -> IntoService<Self, H>
    where
        H: Handler<Self>,
        Self: Sized,
    {
        Operation::from_handler(handler)
    }

    /// Creates a new [`Service`] for well-formed [`Service`](tower::Service)s.
    fn from_service<S>(svc: S) -> Normalize<Self, S>
    where
        S: OperationService<Self>,
        Self: Sized,
    {
        Operation::from_service(svc)
    }
}
```

Observe that there are two constructors provided: `from_handler` which takes a `H: Handler` and `from_service` which takes a `S: OperationService`. In both cases `Self` is passed as a parameter to the traits - this constrains `handler: H` and `svc: S` to the signature given by the implementation of `OperationShape` on `Self`.

The [`Handler`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/operation/trait.Handler.html) and [`OperationService`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/operation/trait.OperationService.html) both serve a similar purpose - they provide a common interface for converting to a model service `S`.

- The `Handler<GetPokemonSpecies>` trait covers all async functions taking `GetPokemonSpeciesInput` and asynchronously returning a `Result<GetPokemonSpeciesOutput, GetPokemonSpeciesError>`.
- The `OperationService<GetPokemonSpecies>` trait covers all `tower::Service`s with request `GetPokemonSpeciesInput`, response `GetPokemonSpeciesOutput` and error `GetPokemonSpeciesOutput`.

The `from_handler` constructor is used in the following way:

```rust,ignore
async fn get_pokemon_service(input: GetPokemonServiceInput) -> Result<GetPokemonServiceOutput, GetPokemonServiceError> {
    /* Handler logic */
}

let operation = GetPokemonService::from_handler(get_pokemon_service);
```

Alternatively, `from_service` constructor:

```rust,ignore
struct Svc {
    /* ... */
}

impl Service<GetPokemonServiceInput> for Svc {
    type Response = GetPokemonServiceOutput;
    type Error = GetPokemonServiceError;

    /* ... */
}

let svc: Svc = /* ... */;
let operation = GetPokemonService::from_service(svc);
```

To summarize a _model service_ constructed can be constructed from a `Handler` or a `OperationService` subject to the constraints of an `OperationShape`. More detailed information on these conversions is provided in the [Handler and OperationService section](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/operation/index.html) Rust docs.

## Serialization and Deserialization

A [Smithy protocol](https://awslabs.github.io/smithy/2.0/spec/protocol-traits.html#serialization-and-protocol-traits) specifies the serialization/deserialization scheme - how a HTTP request is transformed into a modelled input and a modelled output to a HTTP response. The is formalized using the [`FromRequest`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/trait.FromRequest.html) and [`IntoResponse`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/response.rs#L40-L44) traits:

```rust,ignore
/// Provides a protocol aware extraction from a [`Request`]. This consumes the
/// [`Request`], in contrast to [`FromParts`].
pub trait FromRequest<Protocol>: Sized {
    type Rejection: IntoResponse<Protocol>;
    type Future: Future<Output = Result<Self, Self::Rejection>>;

    /// Extracts `self` from a [`Request`] asynchronously.
    fn from_request(request: http::Request) -> Self::Future;
}

/// A protocol aware function taking `self` to [`http::Response`].
pub trait IntoResponse<Protocol> {
    /// Performs a conversion into a [`http::Response`].
    fn into_response(self) -> http::Response<BoxBody>;
}
```

Note that both traits are parameterized by `Protocol`. These [protocols](https://awslabs.github.io/smithy/2.0/aws/protocols/index.html) exist as ZST marker structs:

```rust,ignore
/// [AWS REST JSON 1.0 Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restjson1-protocol.html).
pub struct RestJson1;

/// [AWS REST XML Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restxml-protocol.html).
pub struct RestXml;

/// [AWS JSON 1.0 Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-json-1_0-protocol.html).
pub struct AwsJson1_0;

/// [AWS JSON 1.1 Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-json-1_1-protocol.html).
pub struct AwsJson1_1;
```

## Upgrading a Model Service

We can "upgrade" a model service to a HTTP service using `FromRequest` and `IntoResponse` described in the prior section:

```mermaid
stateDiagram-v2
    direction LR
    HttpService: HTTP Service
    [*] --> from_request: HTTP Request
    state HttpService {
        direction LR
        ModelService: Model Service
        from_request --> ModelService: Model Input
        ModelService --> into_response: Model Output
    }
    into_response --> [*]: HTTP Response
```

This is formalized by the [`Upgrade<Protocol, Op, S>`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/operation/struct.Upgrade.html) HTTP service. The `tower::Service` implementation is approximately:

```rust,ignore
impl<P, Op, S> Service<http::Request> for Upgrade<P, Op, S>
where
    Input: FromRequest<P, B>,
    S: Service<Input>,
    S::Response: IntoResponse<P>,
    S::Error: IntoResponse<P>,
{
    async fn call(&mut self, request: http::Request) -> http::Response {
        let model_request = match <Op::Input as OperationShape>::from_request(request).await {
            Ok(ok) => ok,
            Err(err) => return err.into_response()
        };
        let model_response = self.model_service.call(model_request).await;
        model_response.into_response()
    }
}
```

When we `GetPokemonService::from_handler` or `GetPokemonService::from_service`, the model service produced, `S`, will meet the constraints above.

There is an associated `Plugin`, `UpgradePlugin` which constructs `Upgrade` from a service.

The upgrade procedure is finalized by the application of the `Layer` `L`, referenced in `Operation<S, L>`. In this way the entire upgrade procedure takes an `Operation<S, L>` and returns a HTTP service.

```mermaid
stateDiagram-v2
    direction LR
    [*] --> UpgradePlugin: HTTP Request
    state HttpPlugin {
        state UpgradePlugin {
            direction LR
            [*] --> S: Model Input
            S --> [*] : Model Output
            state ModelPlugin {
                S
            }
        }
    }
    UpgradePlugin --> [*]: HTTP Response
```

Note that the `S` is specified by logic written, in Rust, by the customer, whereas `UpgradePlugin` is specified entirely by Smithy model via the protocol, [HTTP bindings](https://awslabs.github.io/smithy/2.0/spec/http-bindings.html), etc.

## Routers

Different protocols supported by Smithy enjoy different routing mechanisms, for example, [AWS JSON 1.0](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-json-1_0-protocol.html#protocol-behaviors) uses the `X-Amz-Target` header to select an operation, whereas [AWS REST XML](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restxml-protocol.html) uses the [HTTP label trait](https://awslabs.github.io/smithy/2.0/spec/http-bindings.html#httplabel-trait).

Despite their differences, all routing mechanisms satisfy a common interface. This is formalized using the [Router](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/routing/trait.Router.html) trait:

```rust,ignore
/// An interface for retrieving an inner [`Service`] given a [`http::Request`].
pub trait Router {
    type Service;
    type Error;

    /// Matches a [`http::Request`] to a target [`Service`].
    fn match_route(&self, request: &http::Request) -> Result<Self::Service, Self::Error>;
}
```

which provides the ability to determine an inner HTTP service from a collection using a `&http::Request`.

Types which implement the `Router` trait are converted to a HTTP service via the `RoutingService` struct:

```rust,ignore
/// A [`Service`] using a [`Router`] `R` to redirect messages to specific routes.
///
/// The `Protocol` parameter is used to determine the serialization of errors.
pub struct RoutingService<R, Protocol> {
    router: R,
    _protocol: PhantomData<Protocol>,
}

impl<R, P> Service<http::Request> for RoutingService<R, P>
where
    R: Router<B>,
    R::Service: Service<http::Request, Response = http::Response>,
    R::Error: IntoResponse<P> + Error,
{
    type Response = http::Response;
    type Error = /* implementation detail */;

    async fn call(&mut self, req: http::Request<B>) -> Result<Self::Response, Self::Error> {
        match self.router.match_route(&req) {
            // Successfully routed, use the routes `Service::call`.
            Ok(ok) => ok.oneshot(req).await,
            // Failed to route, use the `R::Error`s `IntoResponse<P>`.
            Err(error) => {
                debug!(%error, "failed to route");
                Err(Box::new(error.into_response()))
            }
        }
    }
}
```

The `RouterService` is the final piece necessary to form a functioning composition - it is used to aggregate together the HTTP services, created via the upgrade procedure, into a single HTTP service which can be presented to the customer.

```mermaid
stateDiagram
state in <<fork>>
    direction LR
    [*] --> in
    state RouterService {
        direction LR
        in -->  ServiceA
        in --> ServiceB
        in --> ServiceC
    }
    ServiceA --> [*]
    ServiceB --> [*]
    ServiceC --> [*]
```

## Plugins
<!-- TODO(missing_doc): Link to "Write a Plugin" documentation -->

A [`Plugin`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/plugin/trait.Plugin.html) is a
[`tower::Layer`] with two extra type parameters, `Protocol` and `Operation`. This allows the middleware to be
parameterized them and change behavior depending on the context in which it's applied.

```rust
trait Plugin<Protocol, Operation, S> {
    type Service;

    fn apply(&self, svc: S) -> Self::Service;
}
```

An example `Plugin` implementation can be found in [/examples/pokemon-service/src/plugin.rs](https://github.com/awslabs/smithy-rs/blob/main/examples/pokemon-service/src/plugin.rs).

Plugins can be applied in two places:

- HTTP plugins, which are applied pre-deserialization/post-serialization, acting on HTTP requests/responses.
- Model plugins, which are applied post-deserialization/pre-serialization, acting on model inputs/outputs/errors.

```mermaid
stateDiagram-v2
    direction LR
    [*] --> S: HTTP Request
    state HttpPlugin {
        state UpgradePlugin {
            state ModelPlugin {
                S
            }
        }
    }
    S --> [*]: HTTP Response
```

The service builder API requires plugins to be specified upfront - they must be passed as an argument to `builder_with_plugins` and cannot be modified afterwards.

You might find yourself wanting to apply _multiple_ plugins to your service.
This can be accommodated via [`PluginPipeline`].

```rust
use aws_smithy_http_server::plugin::PluginPipeline;
# use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
# use aws_smithy_http_server::plugin::IdentityPlugin as MetricsPlugin;

let pipeline = PluginPipeline::new().push(LoggingPlugin).push(MetricsPlugin);
```

The plugins' runtime logic is executed in registration order.
In the example above, `LoggingPlugin` would run first, while `MetricsPlugin` is executed last.

If you are vending a plugin, you can leverage `PluginPipeline` as an extension point: you can add custom methods to it using an extension trait.
For example:

```rust
use aws_smithy_http_server::plugin::{PluginPipeline, PluginStack};
# use aws_smithy_http_server::plugin::IdentityPlugin as LoggingPlugin;
# use aws_smithy_http_server::plugin::IdentityPlugin as AuthPlugin;

pub trait AuthPluginExt<CurrentPlugins> {
    fn with_auth(self) -> PluginPipeline<PluginStack<AuthPlugin, CurrentPlugins>>;
}

impl<CurrentPlugins> AuthPluginExt<CurrentPlugins> for PluginPipeline<CurrentPlugins> {
    fn with_auth(self) -> PluginPipeline<PluginStack<AuthPlugin, CurrentPlugins>> {
        self.push(AuthPlugin)
    }
}

let pipeline = PluginPipeline::new()
    .push(LoggingPlugin)
    // Our custom method!
    .with_auth();
```

## Builders

The service builder is the primary public API, generated for every [Smithy Service](https://awslabs.github.io/smithy/2.0/spec/service-types.html).
At a high-level, the service builder takes as input a function for each Smithy Operation and returns a single HTTP service. The signature of each function, also known as _handlers_, must match the constraints of the corresponding Smithy model.

You can create an instance of a service builder by calling either `builder_without_plugins` or `builder_with_plugins` on the corresponding service struct.

```rust
/// The service builder for [`PokemonService`].
///
/// Constructed via [`PokemonService::builder`].
pub struct PokemonServiceBuilder<Body, HttpPlugin, ModelPlugin> {
    capture_pokemon_operation: Option<Route<Body>>,
    empty_operation: Option<Route<Body>>,
    get_pokemon_species: Option<Route<Body>>,
    get_server_statistics: Option<Route<Body>>,
    get_storage: Option<Route<Body>>,
    health_check_operation: Option<Route<Body>>,
    http_plugin: HttpPlugin,
    model_plugin: ModelPlugin
}
```

The builder has two setter methods for each [Smithy Operation](https://awslabs.github.io/smithy/2.0/spec/service-types.html#operation) in the [Smithy Service](https://awslabs.github.io/smithy/2.0/spec/service-types.html#service):

```rust
    pub fn get_pokemon_species<HandlerType, HandlerExtractors, UpgradeExtractors>(self, handler: HandlerType) -> Self
    where
        HandlerType:Handler<GetPokemonSpecies, HandlerExtractors>,

        ModelPlugin: Plugin<
            RestJson1,
            GetPokemonSpecies,
            IntoService<GetPokemonSpecies, HandlerType>
        >,
        UpgradePlugin::<UpgradeExtractors>: Plugin<
            RestJson1,
            GetPokemonSpecies,
            ModelPlugin::Service
        >,
        HttpPlugin: Plugin<
            RestJson1,
            GetPokemonSpecies,
            UpgradePlugin::<UpgradeExtractors>::Service
        >,
    {
        let svc = GetPokemonSpecies::from_handler(handler);
        let svc = self.model_plugin.apply(svc);
        let svc = UpgradePlugin::<UpgradeExtractors>::new()
            .apply(svc);
        let svc = self.http_plugin.apply(svc);
        self.get_pokemon_species_custom(svc)
    }

    pub fn get_pokemon_species_service<S, ServiceExtractors, UpgradeExtractors>(self, service: S) -> Self
    where
        S: OperationService<GetPokemonSpecies, ServiceExtractors>,

        ModelPlugin: Plugin<
            RestJson1,
            GetPokemonSpecies,
            Normalize<GetPokemonSpecies, S>
        >,
        UpgradePlugin::<UpgradeExtractors>: Plugin<
            RestJson1,
            GetPokemonSpecies,
            ModelPlugin::Service
        >,
        HttpPlugin: Plugin<
            RestJson1,
            GetPokemonSpecies,
            UpgradePlugin::<UpgradeExtractors>::Service
        >,
    {
        let svc = GetPokemonSpecies::from_service(service);
        let svc = self.model_plugin.apply(svc);
        let svc = UpgradePlugin::<UpgradeExtractors>::new().apply(svc);
        let svc = self.http_plugin.apply(svc);
        self.get_pokemon_species_custom(svc)
    }

    pub fn get_pokemon_species_custom<S>(mut self, svc: S) -> Self
    where
        S: Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible>,
    {
        self.get_pokemon_species = Some(Route::new(svc));
        self
    }
```

Handlers and operations are upgraded to a [`Route`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/routing/route.rs#L49-L52) as soon as they are registered against the service builder. You can think of `Route` as a boxing layer in disguise.

You can transform a builder instance into a complete service (`PokemonService`) using one of the following methods:

- `build`. The transformation fails if one or more operations do not have a registered handler;
- `build_unchecked`. The transformation never fails, but we return `500`s for all operations that do not have a registered handler.

Both builder methods take care of:

1. Pair each handler with the routing information for the corresponding operation;
2. Collect all `(routing_info, handler)` pairs into a `Router`;
3. Transform the `Router` implementation into a HTTP service via `RouterService`;
4. Wrap the `RouterService` in a newtype given by the service name, `PokemonService`.

The final outcome, an instance of `PokemonService`, looks roughly like this:

```rust,ignore
/// The Pokémon Service allows you to retrieve information about Pokémon species.
#[derive(Clone)]
pub struct PokemonService<S> {
    router: RoutingService<RestRouter<S>, RestJson1>,
}
```

The following schematic summarizes the composition:

```mermaid
stateDiagram-v2
    state in <<fork>>
    state "GetPokemonSpecies" as C1
    state "GetStorage" as C2
    state "DoNothing" as C3
    state "..." as C4
    direction LR
    [*] --> in : HTTP Request
    UpgradePlugin --> [*]: HTTP Response
    state PokemonService {
        state RoutingService {
            in --> UpgradePlugin: HTTP Request
            in --> C2: HTTP Request
            in --> C3: HTTP Request
            in --> C4: HTTP Request
            state C1 {
                state HttpPlugin {
                    state UpgradePlugin {
                        direction LR
                        [*] --> S: Model Input
                        S --> [*] : Model Output
                        state ModelPlugin {
                            S
                        }
                    }
                }
            }
            C2
            C3
            C4
        }

    }
    C2 --> [*]: HTTP Response
    C3 --> [*]: HTTP Response
    C4 --> [*]: HTTP Response
```

## Accessing Unmodelled Data

An additional omitted detail is that we provide an "escape hatch" allowing `Handler`s and `OperationService`s to accept data that isn't modelled. In addition to accepting `Op::Input` they can accept additional arguments which implement the [`FromParts`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/trait.FromParts.html) trait:

```rust,ignore
use http::request::Parts;

/// Provides a protocol aware extraction from a [`Request`]. This borrows the
/// [`Parts`], in contrast to [`FromRequest`].
pub trait FromParts<Protocol>: Sized {
    /// The type of the failures yielded extraction attempts.
    type Rejection: IntoResponse<Protocol>;

    /// Extracts `self` from a [`Parts`] synchronously.
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection>;
}
```

This differs from `FromRequest` trait, introduced in [Serialization and Deserialization](#serialization-and-deserialization), as it's synchronous and has non-consuming access to [`Parts`](https://docs.rs/http/latest/http/request/struct.Parts.html), rather than the entire [Request](https://docs.rs/http/latest/http/request/struct.Request.html).

```rust,ignore
pub struct Parts {
    pub method: Method,
    pub uri: Uri,
    pub version: Version,
    pub headers: HeaderMap<HeaderValue>,
    pub extensions: Extensions,
    /* private fields */
}
```

This is commonly used to access types stored within [`Extensions`](https://docs.rs/http/0.2.8/http/struct.Extensions.html) which have been inserted by a middleware. An `Extension` struct implements `FromParts` to support this use case:

```rust,ignore
/// Generic extension type stored in and extracted from [request extensions].
///
/// This is commonly used to share state across handlers.
///
/// If the extension is missing it will reject the request with a `500 Internal
/// Server Error` response.
///
/// [request extensions]: https://docs.rs/http/latest/http/struct.Extensions.html
#[derive(Debug, Clone)]
pub struct Extension<T>(pub T);

impl<Protocol, T> FromParts<Protocol> for Extension<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Rejection = MissingExtension;

    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        parts.extensions.remove::<T>().map(Extension).ok_or(MissingExtension)
    }
}

/// The extension has not been added to the [`Request`](http::Request) or has been previously removed.
#[derive(Debug, Error)]
#[error("the `Extension` is not present in the `http::Request`")]
pub struct MissingExtension;

impl<Protocol> IntoResponse<Protocol> for MissingExtension {
    fn into_response(self) -> http::Response<BoxBody> {
        let mut response = http::Response::new(empty());
        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        response
    }
}
```
