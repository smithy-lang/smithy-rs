# The Anatomy of a Service

What is [Smithy](https://awslabs.github.io/smithy/2.0/index.html)? At a high-level, it's a grammar for specifying services while leaving the business logic undefined. A [Smithy Service](https://awslabs.github.io/smithy/2.0/spec/service-types.html#service) specifies a collection of function signatures in the form of [Operations](https://awslabs.github.io/smithy/2.0/spec/service-types.html#operation) which encapsulate business logic. A Smithy implementation should, for each Smithy Service, provide a builder, which accepts functions conforming to said signatures, and returns a service subject to the semantics specified by the model.

This survey is disinterested in the actual code generator Kotlin implementation and instead focuses on the structure of the generated Rust code and how it relates to the Smithy model. The intended audience is new contributors and users interested in the internal details.

During the survey we will use the [`pokemon.smithy`](https://github.com/awslabs/smithy-rs/blob/main/codegen-core/common-test-models/pokemon.smithy) model as a reference:

```smithy
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

## Operations

A [Smithy Operation](https://awslabs.github.io/smithy/2.0/spec/service-types.html#operation) specifies the input, output, and possible errors of an API operation. One might characterize a Smithy Operation as syntax for specifying a function type.

We represent this in Rust using the [`OperationShape`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/operation/shape.rs#L8-L22) trait:

```rust
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

```rust
/// Retrieve information about a Pokémon species.
pub struct GetPokemonSpecies;

impl OperationShape for GetPokemonSpecies {
    const NAME: &'static str = "com.aws.example#GetPokemonSpecies";

    type Input = GetPokemonSpeciesInput;
    type Output = GetPokemonSpeciesOutput;
    type Error = GetPokemonSpeciesError;
}
```

Note that `GetPokemonSpecies` marker structure is a zero-sized type (ZST), and therefore does not allocate - only existing in the type system as a way to hang operation specific data on.

The following nomenclature will aid us in our survey. We describe a `tower::Service` as a "model service" if its request and response are Smithy structures, as defined by the `OperationShape` trait - the `GetPokemonSpeciesInput`, `GetPokemonSpeciesOutput`, and `GetPokemonSpeciesError` described above. Similarly, we describe a `tower::Service` as a "HTTP service" if its request and response are [`http`](https://github.com/hyperium/http) structures - `http::Request` and `http::Response`.

In contrast to the marker ZSTs above, the [`Operation<S, L>`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/operation/mod.rs#L192-L198) structure holds the actual runtime behavior of an operation, which is specified, during construction, by the customer. The `S` here is a model service, this is specified during construction of the `Operation<S, L>`. The constructors exist on the marker ZSTs as an extension trait to `OperationShape`, namely [`OperationShapeExt`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/operation/shape.rs#L24-L45):

```rust
/// An extension trait over [`OperationShape`].
pub trait OperationShapeExt: OperationShape {
    /// Creates a new [`Operation`] for well-formed [`Handler`]s.
    fn from_handler<H>(handler: H) -> Operation<IntoService<Self, H>>
    where
        H: Handler<Self>,
        Self: Sized,
    {
        Operation::from_handler(handler)
    }

    /// Creates a new [`Operation`] for well-formed [`Service`](tower::Service)s.
    fn from_service<S>(svc: S) -> Operation<Normalize<Self, S>>
    where
        S: OperationService<Self>,
        Self: Sized,
    {
        Operation::from_service(svc)
    }
}
```

Observe that there are two constructors provided: `from_handler` which takes a `H: Handler` and `from_service` which takes a `S: OperationService`. In both cases `Self` is passed as a parameter to the traits - this constrains `handler: H` and `svc: S` to the signature given by the implementation of `OperationShape` on `Self`.

The [`Handler`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/operation/handler.rs#L21-L29) and [`OperationService`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/operation/operation_service.rs#L15-L29) both serve a similar purpose - they provide a common interface for converting to a model service `S`. The `Handler<GetPokemonSpecies>` trait covers all closures taking `GetPokemonSpeciesInput` and asynchronously returning a `Result<GetPokemonSpeciesOutput, GetPokemonSpeciesError>` - they are converted to a model service by a `IntoService`. The `OperationService<GetPokemonSpecies>` trait covers all `tower::Service`s with request `GetPokemonSpeciesInput`, response `GetPokemonSpeciesOutput` and error `GetPokemonSpeciesOutput` - they are converted to a model service by a `Normalize` (this is a very small conversion which flattens request tuples).
<!-- TODO(hidden_docs): More information about the conversions can be found here. -->

The `from_handler` constructor is used in the following way:

```rust
async fn get_pokemon_service(input: GetPokemonServiceInput) -> Result<GetPokemonServiceOutput, GetPokemonServiceError> {
    /* Handler logic */
}

let operation = GetPokemonService::from_handler(get_pokemon_service);
```

Alternatively, `from_service` constructor:

```rust
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

To summarize, the `S`, in `Operation<S, L>`, is a _model service_ constructed from a `Handler` or a `OperationService` subject to the constraints of an `OperationShape`.

Now, what about the `L` in `Operation<S, L>`? The `L` is a `tower::Layer`, or colloquially "middleware", that is applied to a _HTTP service_. Note that this means that `L` is _not_ applied directly to `S`. We can append to `L` using the `Operation::layer` method:

```rust
impl<S, L> Operation<S, L> {
    /// Applies a [`Layer`] to the operation _after_ it has been upgraded via [`Operation::upgrade`].
    pub fn layer<NewL>(self, layer: NewL) -> Operation<S, Stack<L, NewL>> {
        Operation {
            inner: self.inner,
            layer: Stack::new(self.layer, layer),
        }
    }
}
```

where [`Stack`](https://docs.rs/tower/latest/tower/layer/util/struct.Stack.html) is used to chains layers together.

A typical use of this might be:

```rust
let operation = GetPokemonSpecies::from_handler(handler).layer(RequestBodyLimitLayer::new(500));
```

where [`RequestBodyLimitLayer`](https://docs.rs/tower-http/latest/tower_http/limit/struct.RequestBodyLimitLayer.html) limits the size of the HTTP request to the `GetPokemonSpecies` operation.

As mentioned, `L` is applied _after_ the `Operation<S, L>` has been "upgraded" to a HTTP service. The procedure of upgrading a model service to a HTTP service is described in the [Upgrading a Model Service](#upgrading-a-model-service) section below.

## Serialization and Deserialization

A [Smithy protocol](https://awslabs.github.io/smithy/2.0/spec/protocol-traits.html#serialization-and-protocol-traits) specifies the serialization/deserialization scheme - how a HTTP request is transformed into a modelled input and a modelled output to a HTTP response. The is formalized using the [`FromRequest`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/request.rs#L156-L164) and [`IntoResponse`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/response.rs#L40-L44) traits:

```rust
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

```rust
/// [AWS REST JSON 1.0 Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restjson1-protocol.html).
pub struct AwsRestJson1;

/// [AWS REST XML Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restxml-protocol.html).
pub struct AwsRestXml;

/// [AWS JSON 1.0 Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-json-1_0-protocol.html).
pub struct AwsJson10;

/// [AWS JSON 1.1 Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-json-1_1-protocol.html).
pub struct AwsJson11;
```

## Upgrading a Model Service

Notice that we can "upgrade" a model service to a HTTP service using `FromRequest` and `IntoResponse` described in the prior section:

![Upgrade Data Flow Diagram](imgs/upgrade-dfd.png)

This formalized by the [`Upgrade<Protocol, Op, S>`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/operation/upgrade.rs#L76-L84) HTTP service. The `tower::Service` implementation is approximately:

```rust
impl<P, Op, S> Service<http::Request> for Upgrade<P, Op, S>
where
    // `Op` is used to specify the operation shape
    Op: OperationShape,
    // Smithy input must convert from a HTTP request
    Op::Input: FromRequest<P>,
    // Smithy output must convert into a HTTP response
    Op::Output: IntoResponse<P>,
    // Smithy error must convert into a HTTP response
    OpError: IntoResponse<P>,

    // The signature of the inner service is correct
    S: Service<Op::Input, Response = Op::Output, Error = Op::Error>,

    async fn call(&mut self, request: http::Request) -> http::Response {
        let model_request = match <Op::Input as OperationShape>::from_request(request).await {
            Ok(ok) => ok,
            Err(err) => return err.into_response()
        };
        let model_response = self.model_service.call(model_request).await;
        model_response.into_response()
    }
```

When we `GetPokemonService::from_handler` or `GetPokemonService::from_service`, the model service produced, `S`, will meet the constraints above.

There is an associated `tower::Layer`, `UpgradeLayer<P, Op, B>` which constructs `Upgrade` from a service.

The upgrade procedure is finalized by the application of the `tower::Layer` `L`, referenced in `Operation<S, L>`. In this way the entire upgrade procedure takes an `Operation<S, L>` and returns a HTTP service.

![Upgradable Diagram](imgs/upgradable.png)

Note that the `S` and `L` are specified by logic written, in Rust, by the customer, whereas `Upgrade`/`UpgradeLayer` is specified entirely by Smithy model via the protocol, [HTTP bindings](https://awslabs.github.io/smithy/2.0/spec/http-bindings.html), etc.

The procedure of taking a struct and transforming it into a HTTP service is formalized by the [`Upgradable`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/operation/upgrade.rs#L222-L229) trait:

```rust
impl<P, Op, S, L> Upgradable<P, Op> for Operation<S, L>
where
    // `Op` is used to specify the operation shape
    Op: OperationShape,

    // Smithy input must convert from a HTTP request
    Op::Input: FromRequest<P>,
    // Smithy output must convert into a HTTP response
    Op::Output: IntoResponse<P>,
    // Smithy error must convert into a HTTP response
    Op::Error: IntoResponse<P>,

    // The signature of the inner service is correct
    S: Service<Op::Input, Response = Op::Output, Error = Op::Error> + Clone,

    // The modified Layer applies correctly to `Upgrade<P, Op, S>`
    L: Layer<Upgrade<P, Op, S>>,

    // The signature of the output is correct
    L::Service: Service<http::Request<B>, Response = http::Response<BoxBody>>,
{
    type Service = L::Service;

    /// Takes the [`Operation<S, L>`](Operation), then applies [`UpgradeLayer`] to `S`, then finally applies the `L`.
    ///
    /// The composition is made explicit in the method constraints and return type.
    fn upgrade(self) -> Self::Service {
        let Operation { inner, layer } = self;
        let layer = Stack::new(UpgradeLayer::new(), layer);
        layer.layer(inner)
    }
}
```

Why do we need a trait for this? Why not simply write an `upgrade` method on `Operation<S, L>`? The reason is that we might _not_ want to supply an `Operation<S, L>` to the service builder, instead we might want to supply something that overrides the typical upgrade procedure.

Below we give an example of a ZST which can be provided to the builder, which also satisfies `Upgradable` and returns a `MissingFailure` `tower::Service`. This `MissingFailure` service simply returns a status code 500.

```rust
/// A marker struct indicating an [`Operation`] has not been set in a builder.
///
/// This _does_ implement [`Upgradable`] but produces a [`Service`] which always returns an internal failure message.
pub struct FailOnMissingOperation;

impl<P, Op, B> Upgradable<P, Op> for FailOnMissingOperation
where
    InternalFailureException: IntoResponse<P>,
{
    type Service = MissingFailure<P>;

    fn upgrade(self, _plugin: &Pl) -> Self::Service {
        MissingFailure { _protocol: PhantomData }
    }
}
```

We go into more detail on how the `Upgradable` trait is used in conjunction with builders in the [Builders](#builders) section below.

## Routers

Different protocols supported by Smithy enjoy different routing mechanisms, for example, [AWS JSON 1.0](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-json-1_0-protocol.html#protocol-behaviors) uses the `X-Amz-Target` header to select an operation, whereas [AWS REST XML](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restxml-protocol.html) uses the [HTTP label trait](https://awslabs.github.io/smithy/2.0/spec/http-bindings.html#httplabel-trait).

Despite their differences, all routing mechanisms satisfy a common interface. This is formalized using the `Router` trait:

```rust
/// An interface for retrieving an inner [`Service`] given a [`http::Request`].
pub trait Router {
    type Service;
    type Error;

    /// Matches a [`http::Request`] to a target [`Service`].
    fn match_route(&self, request: &http::Request) -> Result<Self::Service, Self::Error>;
}
```

which provides the ability to lookup an inner HTTP service from a collection using a `&http::Request`.

Types which implement the `Router` trait are converted to a HTTP service via the `RoutingService` struct:

```rust
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
    type Error = Box<dyn Error>;

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

![RouterService](imgs/router.png)

## Builders

The service builder is the primary public API, it can be generated for every [Smithy Service](https://awslabs.github.io/smithy/2.0/spec/service-types.html). At a high-level, the purpose of a service builder is to take a function for each Smithy Operation, whose signature is constrained by the Smithy model, and produce a single HTTP service.

```rust
/// The service builder for [`PokemonService`].
///
/// Constructed via [`PokemonService::builder`].
pub struct PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6> {
    capture_pokemon_operation: Op1,
    empty_operation: Op2,
    get_pokemon_species: Op3,
    get_server_statistics: Op4,
    get_storage: Op5,
    health_check_operation: Op6,
}
```

The builder has one generic variable for each [Smithy Operation](https://awslabs.github.io/smithy/2.0/spec/service-types.html#operation) in the [Smithy Service](https://awslabs.github.io/smithy/2.0/spec/service-types.html#service). Each setter switches the type of the `Op{N}` type parameter:

```rust
    /// Sets the [`GetPokemonSpecies`](crate::operation_shape::GetPokemonSpecies) operation.
    ///
    /// This should be an [`Operation`](aws_smithy_http_server::operation::Operation) created from
    /// [`GetPokemonSpecies`](crate::operation_shape::GetPokemonSpecies) using either
    /// [`OperationShape::from_handler`](aws_smithy_http_server::operation::OperationShapeExt::from_handler) or
    /// [`OperationShape::from_service`](aws_smithy_http_server::operation::OperationShapeExt::from_service).
    pub fn get_pokemon_species_operation<NewOp>(
        self,
        value: NewOp,
    ) -> PokemonServiceBuilder<
        Op1,
        Op2,
        NewOp,
        Op4,
        Op5,
        Op6,
    > {
        PokemonServiceBuilder {
            capture_pokemon_operation: self.capture_pokemon_operation,
            empty_operation: self.empty_operation,
            get_pokemon_species: value,
            get_server_statistics: self.get_server_statistics,
            get_storage: self.get_storage,
            health_check_operation: self.health_check_operation,
        }
    }

    /// Sets the [`GetPokemonSpecies`](crate::operation_shape::GetPokemonSpecies) operation.
    ///
    /// This should be a closure satisfying the [`Handler`](aws_smithy_http_server::operation::Handler) trait.
    /// See the [operation module documentation](aws_smithy_http_server::operation) for more information.
    pub fn get_pokemon_species<H, NewExts>(
        self,
        value: H,
    ) -> PokemonServiceBuilder<
        Op1,
        Op2,
        Operation<IntoService<GetPokemonSpecies, H>>,
        Op4,
        Op5,
        Op6,
    >
    where
        H: Handler<GetPokemonSpecies>,
    {
        self.get_pokemon_species_operation(GetPokemonSpecies::from_handler(value))
    }
```

To finalize the build and construct the complete service, `PokemonService`, each builder has a `build` method whose constraints list out all the requirements for composition:

```rust
    /// Constructs a [`PokemonService`] from the arguments provided to the builder.
    pub fn build(self) -> PokemonService<Route>
    where
        Op1: Upgradable<AwsRestJson1, CheckHealth>,
        Op1::Service: tower::Service<http::Request, Error = Infallible>,

        Op2: Upgradable<AwsRestJson1, DoNothing>,
        Op2::Service: tower::Service<http::Request, Error = Infallible>,

        /* ... */

        Op6: Upgradable<AwsRestJson1, GetStorage>,
        Op6::Service: tower::Service<http::Request, Error = Infallible>,
```

Notice the general form: `Op{N}` must upgrade to a HTTP service.

We provide two builder constructors:

```rust
    /// Constructs a builder for [`PokemonService`].
    pub fn builder() -> PokemonServiceBuilder<
        MissingOperation,
        MissingOperation,
        MissingOperation,
        MissingOperation,
        MissingOperation,
        MissingOperation,
    > {
        PokemonServiceBuilder {
            check_health: MissingOperation,
            do_nothing: MissingOperation,
            get_pokemon_species: MissingOperation,
            get_server_statistics: MissingOperation,
            capture_pokemon: MissingOperation,
            get_storage: MissingOperation,
        }
    }

    /// Constructs an unchecked builder for [`PokemonService`].
    ///
    /// This will not enforce that all operations are set, however if an unset operation is used at runtime
    /// it will return status code 500 and log an error.
    pub fn unchecked_builder() -> PokemonServiceBuilder<
        FailOnMissingOperation,
        FailOnMissingOperation,
        FailOnMissingOperation,
        FailOnMissingOperation,
        FailOnMissingOperation,
        FailOnMissingOperation,
    > {
        PokemonServiceBuilder {
            check_health: FailOnMissingOperation,
            do_nothing: FailOnMissingOperation,
            get_pokemon_species: FailOnMissingOperation,
            get_server_statistics: FailOnMissingOperation,
            capture_pokemon: FailOnMissingOperation,
            get_storage: FailOnMissingOperation,
        }
    }
```

The `builder` constructor provides a `PokemonServiceBuilder` where `build` cannot be called until all operations are set because `MissingOperation` purposefully doesn't implement `Upgradable`. In contrast, the `unchecked_builder` which sets all `Op{N}` to `FailOnMissingOperation` can be immediately built, however any unset operations are upgraded into a service which always returns status code 500, as noted in [Upgrading a Model Service](#upgrading-a-model-service).

The build method then proceeds as follows:

1. Upgrade all `Op{N}` to a HTTP service via their `Upgradable::upgrade` method.
2. Type erase them via [`Route`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/routing/route.rs#L49-L52) (basically amounts to `Box`ing them).
3. Pair each of them with their routing information and collect them all into a `Router`.
4. Transform the `Router` implementation into a HTTP service via `RouterService`.
5. Wrap the `RouterService` in a newtype given by the service name, `PokemonService`.

```rust
    /// Constructs a [`PokemonService`] from the arguments provided to the builder.
    pub fn build(self) -> PokemonService<Route>
    where
        Op1: Upgradable<AwsRestJson1, CheckHealth>,
        Op1::Service: tower::Service<http::Request, Error = Infallible>,

        Op2: Upgradable<AwsRestJson1, DoNothing>,
        Op2::Service: tower::Service<http::Request, Error = Infallible>,

        /* ... */
{
    let router = RestRouter::from_iter([
        (
            /* `CheckHealth` (Op1) routing information */,
            Route::new(self.check_health.upgrade())
        ),
        (
            /* `DoNothing` (Op2) routing information */,
            Route::new(self.do_nothing.upgrade())
        ),
        /* ... */
    ]);
    PokemonService {
        router: RoutingService::new(router)
    }
}
```

where

```rust
/// The Pokémon Service allows you to retrieve information about Pokémon species.
#[derive(Clone)]
pub struct PokemonService<S> {
    router: RoutingService<RestRouter<S>, AwsRestJson1>,
}
```

## Plugins
<!-- TODO(missing_doc): Link to "Write a Plugin" documentation -->

Smithy Rust also provides a way to hook into the upgrade procedure in order to modify the service behavior. This is done via the [`Plugin`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/plugin.rs#L31-L41) trait:

```rust
/// A mapping from one [`Operation`] to another. Used to modify the behavior of
/// [`Upgradable`](crate::operation::Upgradable) and therefore the resulting service builder,
///
/// The generics `Protocol` and `Op` allow the behavior to be parameterized.
///
/// Every service builder enjoys [`Pluggable`] and therefore can be provided with a [`Plugin`] using
/// [`Pluggable::apply`].
pub trait Plugin<Protocol, Op, S, L> {
    type Service;
    type Layer;

    /// Maps an [`Operation`] to another.
    fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer>;
}
```

The `Upgradable::upgrade` method on `Operation<S, L>`, previously presented in [Upgrading a Model Service](#upgrading-a-model-service), is more accurately:

```rust
    /// Takes the [`Operation<S, L>`](Operation), applies [`Plugin`], then applies [`UpgradeLayer`] to
    /// the modified `S`, then finally applies the modified `L`.
    ///
    /// The composition is made explicit in the method constraints and return type.
    fn upgrade(self, plugin: &Pl) -> Self::Service {
        let Operation { inner, layer } = plugin.map(self);
        let layer = Stack::new(UpgradeLayer::new(), layer);
        layer.layer(inner)
    }
```

![Upgradable Diagram](imgs/upgradable-plugin.png)

An example `Plugin` implementation can be found in [aws-smithy-http-server/examples/pokemon-service/src/plugin.rs](https://github.com/awslabs/smithy-rs/blob/main/rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/plugin.rs).

All builders implement the [`Pluggable`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/plugin.rs#L8-L29) trait, which allows them to apply plugins to service builders:

```rust
/// Provides a standard interface for applying [`Plugin`]s to a service builder. This is implemented automatically for
/// all builders.
///
/// As [`Plugin`]s modify the way in which [`Operation`]s are [`upgraded`](crate::operation::Upgradable) we can use
/// [`Pluggable`] as a foundation to write extension traits which are implemented for all service builders.
///
/// # Example
///
/// ```
/// # struct PrintPlugin;
/// # use aws_smithy_http_server::plugin::Pluggable;
/// trait PrintExt: Pluggable<PrintPlugin> {
///     fn print(self) -> Self::Output where Self: Sized {
///         self.apply(PrintPlugin)
///     }
/// }
///
/// impl<Builder> PrintExt for Builder where Builder: Pluggable<PrintPlugin> {}
/// ```
pub trait Pluggable<NewPlugin> {
    type Output;

    /// Applies a plugin to the service builder.
    fn apply(self, plugin: NewPlugin) -> Self::Output;
}
```

As seen in the `Pluggable` documentation, third-parties can use extension traits over `Pluggable` to extend the API of builders. In addition to all the `Op{N}` the service builder also holds a `Pl`:

```rust
/// The service builder for [`PokemonService`].
///
/// Constructed via [`PokemonService::builder`].
pub struct PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6, Pl> {
    capture_pokemon_operation: Op1,
    empty_operation: Op2,
    get_pokemon_species: Op3,
    get_server_statistics: Op4,
    get_storage: Op5,
    health_check_operation: Op6,

    plugin: Pl
}
```

which allows the following `Pluggable` implementation to be generated:

```rust
impl<Op1, Op2, /* ... */, Pl, NewPl> aws_smithy_http_server::plugin::Pluggable<NewPl>
    for PokemonServiceBuilder<Op1, Op2, /* ... */, Pl>
{
    type Output =
        PokemonServiceBuilder<Op1, Exts1, aws_smithy_http_server::plugin::PluginStack<Pl, NewPl>>;
    fn apply(self, plugin: NewPl) -> Self::Output {
        PokemonServiceBuilder {
            capture_pokemon_operation: self.capture_pokemon_operation,
            empty_operation: self.empty_operation,
            /* ... */,

            plugin: PluginStack::new(self.plugin, plugin),
        }
    }
}
```

Here `PluginStack` works in a similar way to [`tower::layer::util::Stack`](https://docs.rs/tower/latest/tower/layer/util/struct.Stack.html) - allowing users to append a new plugin rather than replacing the currently set one.

## Accessing Unmodelled Data

An additional omitted detail is that we provide an "escape hatch" allowing `Handler`s and `OperationService`s to accept data that isn't modelled. In addition to accepting `Op::Input` they can accept additional arguments which implement the [`FromParts`](https://github.com/awslabs/smithy-rs/blob/4c5cbc39384f0d949d7693eb87b5853fe72629cd/rust-runtime/aws-smithy-http-server/src/request.rs#L114-L121) trait:

```rust
use http::request::Parts;

/// Provides a protocol aware extraction from a [`Request`]. This borrows the
/// [`Parts`], in contrast to [`FromRequest`].
pub trait FromParts<Protocol>: Sized {
    type Rejection: IntoResponse<Protocol>;

    /// Extracts `self` from a [`Parts`] synchronously.
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection>;
}
```

This differs from `FromRequest` trait, introduced in [Serialization and Deserialization](#serialization-and-deserialization), as it's synchronous and has non-consuming access to [`Parts`](https://docs.rs/http/0.2.8/http/request/struct.Parts.html), rather than the entire [Request](https://docs.rs/http/0.2.8/http/request/struct.Request.html).

```rust
pub struct Parts {
    pub method: Method,
    pub uri: Uri,
    pub version: Version,
    pub headers: HeaderMap<HeaderValue>,
    pub extensions: Extensions,
    /* private fields */
}
```

This is commonly used to access types stored within [`Extensions`](https://docs.rs/http/0.2.8/http/struct.Extensions.html) which have been inserted by a middleware.
