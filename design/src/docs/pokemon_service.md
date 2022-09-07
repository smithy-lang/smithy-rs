## Code generating the Pokémon Service

This is an overview of client and server of the Pokémon service. It introduces:

- How a smithy-rs server customer uses the vanilla SDK and writes their business logic
- What the runtime is and how code is generated
- The folder structure of the project

All the code shown and linked to is from the repository at this commit: [db48039065bec890ef387385773b37154b555b14][1]

The Smithy model used to generate the code snippets is: [Pokémon][2]

### Building the service

The entry point of a service is [main.rs][3]

The `PokemonService` service in the `pokemon.smithy` has these operations and resources:

```smithy
resources: [PokemonSpecies, Storage],
operations: [GetServerStatistics, EmptyOperation, CapturePokemonOperation, HealthCheckOperation],
```

The entry app is constructed as:

```rust
let app: Router = OperationRegistryBuilder::default()
```

`OperationRegistryBuilder` is a struct, generated [here][4],
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

1.  If the operation is not fallible and does not share any state:

```rust
pub async fn health_check_operation(_input: input::HealthCheckOperationInput) -> output::HealthCheckOperationOutput {...}
```

2.  If the operation is fallible and does not share any state:

```rust
pub async fn capture_pokemon(
    mut input: input::CapturePokemonOperationInput,
) -> Result<output::CapturePokemonOperationOutput, error::CapturePokemonOperationError> {...}
```

3.  If the operation is not fallible and shares some state:

```rust
pub async fn get_server_statistics(
    _input: input::GetServerStatisticsInput,
    state: Extension<Arc<State>>,
) -> output::GetServerStatisticsOutput {...}
```

4.  If the operation is fallible and shares some state:

```rust
pub async fn get_storage(
    input: input::GetStorageInput,
    _state: Extension<Arc<State>>,
) -> Result<output::GetStorageOutput, error::GetStorageError> {...}
```

All of these are operations which implementors define; they are the business logic of the application. The rest is code generated.

The `OperationRegistry` builds into a `Router` (`let app: Router = OperationRegistryBuilder...build().into()`).
The implementation is code generated [here][5].

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

For each operation, it registers a route; the specifics depend on the [protocol][6].
The PokemonService uses [restJson1][7] as its protocol, an operation like the `HealthCheckOperation` will be rendered as:

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

Finally, it creates a RestJSON `Router`, because that is the service's protocol.
You will have noticed, each operation is implemented as a `pub async fn`. Each operation is wrapped into an `OperationHandler`, generated [here][8].
`OperationHandler` implements tower's `Service` [trait][9]. Implementing `Service` means that
the business logic is written as protocol-agnostic and clients request a service by calling into them, similar to an RPC call.

```rust
aws_smithy_http_server::routing::Router::new_rest_json_router(vec![
    {
        let svc = crate::server_operation_handler_trait::operation(
            registry.capture_pokemon_operation,
        );
```

At this level, logging might be prohibited by the [`@sensitive`][10] trait. If there are no `@sensitive` shapes, the generated code looks like:

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
To not log the passcode, the code will be generated [here][11] as:

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

Each route is a pair, [`BoxCloneService`][12] wrapping the service operation (the implementation) and
the information to consume the service operation.

```rust
(tower::util::BoxCloneService::new(svc), capture_pokemon_operation_request_spec)
```

Now the `Router` is built. `Router` is not code generated, it instead lives in the [`aws-smithy-http-server`][13] crate.
We write Rust code in the runtime to:

- Aid development of the project
- Have service-specific code that the majority of services share in the runtime
  In Kotlin we generate code that is service-specific.

A `Router` is a [`tower::Service`][9] that routes requests to the implemented services; hence it [implements][14] `Service`
like the other operations.

The `Router` [implements][15]
the `tower::Layer` trait. Middleware are added as layers. The Pokémon example uses them:

```rust
let shared_state = Arc::new(State::default());
let app = app.layer(
    ServiceBuilder::new()
        .layer(TraceLayer::new_for_http())
        .layer(AddExtensionLayer::new(shared_state)),
);
```

The service is run by a [Hyper server][16]:

```rust
hyper::Server::bind(&bind).serve(app.into_make_service());
```

Generation of objects common to services, such as shapes, is described in [Code Generation][17].

[1]: https://github.com/awslabs/smithy-rs/tree/db48039065bec890ef387385773b37154b555b14
[2]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server-test/model/pokemon.smithy
[3]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/main.rs#L34
[4]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationRegistryGenerator.kt#L1
[5]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationRegistryGenerator.kt#L285
[6]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/smithy/protocols/Protocol.kt#L81
[7]: https://awslabs.github.io/smithy/1.0/spec/aws/aws-restjson1-protocol.html
[8]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationHandlerGenerator.kt#L30
[9]: https://docs.rs/tower-service/latest/tower_service/trait.Service.html
[10]: https://awslabs.github.io/smithy/1.0/spec/core/documentation-traits.html#sensitive-trait
[11]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerHttpSensitivityGenerator.kt#L58
[12]: https://docs.rs/tower/latest/tower/util/struct.BoxCloneService.html
[13]: https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/
[14]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/rust-runtime/aws-smithy-http-server/src/routing/mod.rs#L302
[15]: https://github.com/awslabs/smithy-rs/blob/db48039065bec890ef387385773b37154b555b14/rust-runtime/aws-smithy-http-server/src/routing/mod.rs#L146
[16]: https://docs.rs/hyper/latest/hyper/server/struct.Server.html
[17]: ./code_generation.md
