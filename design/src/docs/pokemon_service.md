## The Pokémon Service

This is an overview of the Pokémon service. It describes:
* How a smithy-rs customer uses the vanilla SDK and writes their business logic
* What the runtime is and how code is generated
* The folder structure of the project

The repository is at commit: db48039065bec890ef387385773b37154b555b14

The Smithy model used here is: [Pokémon](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server-test/model/pokemon.smithy#L1)

### Building the service

Entry point: [main.rs](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/main.rs#L34)

The PokemonService service in the `pokemon.smithy` has these operations and resources:
```smithy
resources: [PokemonSpecies, Storage],
operations: [GetServerStatistics, EmptyOperation, CapturePokemonOperation, HealthCheckOperation],
```
The entry app is constructed as:
```rust
let app: Router = OperationRegistryBuilder::default()
```

`OperationRegistryBuilder` is a struct, generated [here](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationRegistryGenerator.kt#L1),
used by service implementors to register, for each operation, the operation's implementation logic, input and output.
```rust
pub struct OperationRegistry<B, Op0, In0, Op1, In1, Op2, In2, Op3, In3, Op4, In4, Op5, In5> {
    capture_pokemon_operation: Op0,
    empty_operation: Op1,
    get_pokemon_species: Op2,
    get_server_statistics: Op3,
    get_storage: Op4,
    health_check_operation: Op5,
    _phantom: std::marker::PhantomData<(B, In0, In1, In2, In3, In4, In5)>,
}
```
The builder is constructed by a `OperationRegistryBuilder`; if an operation is not passed to the builder, it will return an error.
```rust
let app: Router = OperationRegistryBuilder::default()
    .get_pokemon_species(get_pokemon_species)
    .get_storage(get_storage)
    .get_server_statistics(get_server_statistics)
    .capture_pokemon_operation(capture_pokemon)
    .empty_operation(empty_operation)
    .health_check_operation(health_check_operation)
    .build()
    .expect("Unable to build operation registry")
    .into();
```
Each of these operations is a function that can take any of these signatures.
1. If the operation does not return any error and does not save any state between invocations:
```rust
pub async fn health_check_operation(_input: input::HealthCheckOperationInput) -> output::HealthCheckOperationOutput {...}
```
2. If the operation does return errors and does not save any state:
```rust
pub async fn capture_pokemon(
    mut input: input::CapturePokemonOperationInput,
) -> Result<output::CapturePokemonOperationOutput, error::CapturePokemonOperationError> {...}
```
3. If the operation returns no error and saves some state:
```rust
pub async fn get_server_statistics(
    _input: input::GetServerStatisticsInput,
    state: Extension<Arc<State>>,
) -> output::GetServerStatisticsOutput {...}
```
4. If the operation returns errors and saves some state:
```rust
pub async fn get_storage(
    input: input::GetStorageInput,
    _state: Extension<Arc<State>>,
) -> Result<output::GetStorageOutput, error::GetStorageError> {...}
```
All of these are operations which implementors create; they are the business logic of the application. The rest is code generated.

The `OperationRegistry` builds into a `Router` (`let app: Router = OperationRegistryBuilder...build().into()`).
The implementation is code generated [here](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationRegistryGenerator.kt#L285).
```rust
impl<B, Op0, In0, Op1, In1, Op2, In2, Op3, In3, Op4, In4, Op5, In5>
    std::convert::From<
        OperationRegistry<B, Op0, In0, Op1, In1, Op2, In2, Op3, In3, Op4, In4, Op5, In5>,
    > for aws_smithy_http_server::routing::Router<B>
where
    B: Send + 'static,
    Op0: crate::server_operation_handler_trait::Handler<
        B,
        In0,
        crate::input::CapturePokemonOperationInput,
    >,
    In0: 'static + Send,
    ... for all Op, In
{
    fn from(
        registry: OperationRegistry<B, Op0, In0, Op1, In1, Op2, In2, Op3, In3, Op4, In4, Op5, In5>,
    ) -> Self {...}
}
```
For each operation, it registers a route; the specifics depend on the [protocol](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/protocols/Protocol.kt#L81).
For RestJSON of the PokemonService, an operation like the `HealthCheckOperation` will be rendered as:
```rust
let capture_pokemon_operation_request_spec = aws_smithy_http_server::routing::request_spec::RequestSpec::new(
    http::Method::POST,
    aws_smithy_http_server::routing::request_spec::UriSpec::new(
        aws_smithy_http_server::routing::request_spec::PathAndQuerySpec::new(
            aws_smithy_http_server::routing::request_spec::PathSpec::from_vector_unchecked(vec![
                aws_smithy_http_server::routing::request_spec::PathSegment::Literal(String::from("capture-pokemon-event")),
                aws_smithy_http_server::routing::request_spec::PathSegment::Label,
            ]),
            aws_smithy_http_server::routing::request_spec::QuerySpec::from_vector_unchecked(vec![]))),);
```
because the URI is `/capture-pokemon-event/{region}`, with method `POST` and `region` a `Label` (then passed to the operation with its `CapturePokemonOperationInput` input struct).

Finally it creates a RestJSON `Router`, because that is the service's protocol.
You will have noticed, each operation is implemented as a `pub async fn`. Each operation is wrapped into an `OperationHandler`, generated [here](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationHandlerGenerator.kt#L30).
`OperationHandler` implements tower's `Service` [trait](https://docs.rs/tower-service/latest/tower_service/trait.Service.html). Implementing `Service` means that
the business logic is written as protocol-agnostic and clients request a service by calling into them, similar to an RPC call.
```rust
aws_smithy_http_server::routing::Router::new_rest_json_router(vec![
    {
        let svc = crate::server_operation_handler_trait::operation(
            registry.capture_pokemon_operation,
        );
```
At this level, logging might be prohibited by the `@sensitive` trait. If there are no `@sensitive` shapes, the generated code looks like:
```rust
let request_fmt = aws_smithy_http_server::logging::sensitivity::RequestFmt::new();
let response_fmt = aws_smithy_http_server::logging::sensitivity::ResponseFmt::new();
let svc = aws_smithy_http_server::logging::InstrumentOperation::new(
    svc,
    "capture_pokemon_operation",
)
.request_fmt(request_fmt)
.response_fmt(response_fmt);
```
Accessing the Pokédex is modeled as a restricted operation: a passcode is needed by the Pokémon trainer.
Not to log the passcode, the code will be generated [here](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerHttpSensitivityGenerator.kt#L58) as:
```rust
let request_fmt = aws_smithy_http_server::logging::sensitivity::RequestFmt::new()
    .header(|name: &http::header::HeaderName| {
        #[allow(unused_variables)]
        let name = name.as_str();
        let name_match = matches!(name, "passcode");
        let key_suffix = None;
        let value = name_match;
        aws_smithy_http_server::logging::sensitivity::headers::HeaderMarker {
            value,
            key_suffix,
        }
    })
    .label(|index: usize| matches!(index, 1));
let response_fmt = aws_smithy_http_server::logging::sensitivity::ResponseFmt::new();
```
Each route is a pair, [`BoxCloneService`](https://docs.rs/tower/latest/tower/util/struct.BoxCloneService.html) wrapping the service operation (the implementation) and
the information to consume the service operation.
```rust
(tower::util::BoxCloneService::new(svc), capture_pokemon_operation_request_spec)
```
Now the `Router` is built. `Router` is not code generated, instead lives in the aws-smithy-http-server crate; it is, whenever possible, better to write Rust code in Rust, rather than generate it to aid development.
A `Router` is a tower `Service` that routes requests to the implemented services; hence it [implements](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/rust-runtime/aws-smithy-http-server/src/routing/mod.rs#L302) `Service`
like the other operations.

To re-use code and not to overload a router with possibly unneeded operations, it also [implements](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/rust-runtime/aws-smithy-http-server/src/routing/mod.rs#L146)
the `tower::Layer` trait. Middleware are added as layers. The Pokémon example uses them:
```rust
let shared_state = Arc::new(State::default());
let app = app.layer(
    ServiceBuilder::new()
        .layer(TraceLayer::new_for_http())
        .layer(AddExtensionLayer::new(shared_state)),
);
```
The service is run by `hyper`:
```rust
hyper::Server::bind(&bind).serve(app.into_make_service());
```


### Folder structure
Before describing how shapes are generated, it is helpful to know the folder structure of smithy-rs.
Code generation happens in:
* `/codegen`: it contains shared code for both client and server, but only generates a client
* `/codegen-server`: server only. This project started with `codegen` to generate a client, but client and server share common code; that code lives in `codegen`, which `codegen-server` depends on
* `/aws`: the AWS SDK, it deals with AWS services specifically. The folder structure reflects the project's, with the `rust-runtime` and the `codegen`
* `/rust-runtime`: the generated client and server crates may depend on crates in this folder. Crates here are not code generated

`/rust-runtime` crates ("runtime crates") are added to a crate's dependency only when used. If a model uses event streams, it will depend on `aws-smithy-eventstream`.

### Generating code
`smithy-rs` is a gradle plugin, not a command. Its entry point is in [RustCodegenPlugin::execute](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/RustCodegenPlugin.kt#L34) and
inherits from `SmithyBuildPlugin` in [smithy-build](https://github.com/awslabs/smithy/tree/main/smithy-build). Code generation is in Kotlin and shared common, non-Rust specific code with the `smithy` Java repository.

The comment at the beginning of `execute` described what a `Decorator` is and uses the following terms:
* Context: contains the model being generated, projection and settings for the build
* Decorator: (also referred to as customizations) customizes how code is being generated. AWS services are required to sign with the SigV4 protocol, and [a decorator](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/aws/sdk-codegen/src/main/kotlin/software/amazon/smithy/rustsdk/SigV4SigningDecorator.kt#L45) adds Rust code to sign requests and responses.
Decorators are applied in reverse order of being added and have a priority order.
* Writer: creates files and adds content; it supports templating, using `#` for substitutions
* Location: the file where a symbol will be written to

The only task of a `RustCodegenPlugin` is to construct a `CodegenVisitor` and call its [execute()](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L115-L115) method.

`CodegenVisitor::execute()` is given a `Context`, constructs the sequence of decorators and calls a [CodegenVisitor](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L44).

CodegenVisitor, RustCodegenPlugin, and wherever there are different implementations between client and server, such as in generating error types,
have corresponding server versions.

Objects used throughout code generation are:
* Symbol: a node in a graph, an abstraction that represents the qualified name of a type; symbols reference and depend on other symbols, and have some common properties among languages (such as a namespace or a definition file). For Rust, we add properties to include more metadata about a symbol, such as its [type](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/SymbolVisitor.kt#L363-L363)
* [RustType](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/rustlang/RustTypes.kt#L25-L25): `Option<T>`, `HashMap`, ... along with their namespaces of origin such as `std::collections`
* [RuntimeType](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/RuntimeTypes.kt#L113-L113): the information to locate a type, plus the crates it depends on
* ShapeId: an immutable object that identifies a `Shape`

Useful conversions are:
```kotlin
SymbolProvider.toSymbol(shape)
```
where `SymbolProvider` constructs symbols for shapes. Some symbols require to create other symbols and types;
[event streams](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/EventStreamSymbolProvider.kt#L65-L65) and [other streaming shapes](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/StreamingTraitSymbolProvider.kt#L26-L26) are an example.
Symbol providers are deterministic and are all [applied](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/RustCodegenPlugin.kt#L62-L62) in order; if a shape uses a reserved keyword in Rust, its name is converted to a new name by a [symbol provider](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/rustlang/RustReservedWords.kt#L26-L26),
and all other providers will work with this [new](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/EventStreamSymbolProvider.kt#L38-L38) symbol.

```kotlin
Model.expectShape(shapeId)
```
Each model has a `shapeId` to `shape` map; this method returns the shape associated with this shapeId.

Some objects implement a `transform` [method](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/transformers/OperationNormalizer.kt#L52-L52) that only change the input model, so that code generation will work on that new model. This is used to, for example, add a trait to a shape.

`CodegenVisitor` is a `ShapeVisitor`. For all services in the input model, shapes are [converted into Rust](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L119-L119);
[here](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L150-L150) is how a service is constructed,
[here](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L172-L172) a structure and so on.

Code generation flows from writer to files and entities are (mostly) generated only on a [need-by-need basis](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenDelegator.kt#L119-L126).
The complete result is a [Rust crate](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenDelegator.kt#L42-L42),
in which all dependencies are written into their modules and `lib.rs` is generated ([here](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenDelegator.kt#L96-L107)).
`execute()` ends by running [cargo fmt](https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/CodegenVisitor.kt#L133-L133),
to avoid having to format correctly Rust in `Writer`s and to be sure the generated code follows the styling rules.
