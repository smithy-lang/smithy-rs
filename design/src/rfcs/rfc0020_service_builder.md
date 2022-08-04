# RFC: Better Service Builders

> Status: RFC

One might characterize `smithy-rs` as a tool for transforming a [Smithy service](https://awslabs.github.io/smithy/1.0/spec/core/model.html#service) into a [tower::Service](https://docs.rs/tower-service/latest/tower_service/trait.Service.html) builder. A Smithy model defines semantics and constrains the behavior of the generated service, but not completely - closures must be passed to the builder to before the `tower::Service` is fully specified. This builder structure is the primary API surface we provide to the customer, as a result, it is important that it meets their needs.

This RFC proposes a new builder, deprecating the existing one, which addresses API deficiencies and takes steps to improve performance.

## Terminology

- **Model**: A [Smithy Model](https://awslabs.github.io/smithy/1.0/spec/core/model.html), usually pertaining to the one in use by the customer.
- **Smithy Service**: The entry point of an API that aggregates resources and operations together within a Smithy model. Described in detail [here](https://awslabs.github.io/smithy/1.0/spec/core/model.html#service).
- **Service**: The `tower::Service` trait is an interface for writing network applications in a modular and reusable way. `Service`s act on requests to produce responses.
- **Service Builder**: A `tower::Service` builder, generated from a Smithy service, by `smithy-rs`.
- **Middleware**: Broadly speaking, middleware modify requests and responses. Concretely, these are exist as implementations of [Layer](https://docs.rs/tower/latest/tower/layer/trait.Layer.html)/a `Service` wrapping an inner `Service`.

## Background

Before engaging with the proposal, a survey of the current state of affairs is required.

The following is a reference model we will use throughout the RFC:

```smithy
operation Operation0 {
    input: Input0,
    output: Output0
}

operation Operation1 {
    input: Input1,
    output: Output1
}

service Service {
    operations: [
        Operation0,
        Operation0,
    ]
}
```

We have purposely omitted details from the model that are unimportant to describing the proposal. We also omit distracting details from the Rust snippets.

### Handlers

Before we progress onto a more general look at the current service builder, we should familiarize ourselves with the specifics of the `Handler` trait:

```rust
pub trait Handler<T, Input> {
    async fn call(self, req: http::Request) -> http::Response;
}
```

Its purpose is to provide an even interface over closures of the form `FnOnce(Input) -> impl Future<Output = Output>` and `FnOnce(Input, State) -> impl Future<Output = Output>`. It's this abstraction which allows the customers to supply both `async fn operation0(input: Operation0Input) -> Operation0Output` and `async fn operation0(input: Operation0Input, state: Extension<S>) -> Operation0Output` to the service builder.

We generate `Handler` implementations for said closures in [ServerOperationHandlerGenerator.kt](https://github.com/awslabs/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationHandlerGenerator.kt), for `Operation0` these are:

```rust
impl<Fun, Fut> Handler<(), Operation0Input> for Fun
where
    Fun: FnOnce(crate::input::Operation0Input) -> Fut,
    Fut: Future<Output = Operation0Output>,
{
    async fn call(self, request: http::Request) -> http::Response {
        let input = /* Create `Operation0Input` from `request: http::Request` */;

        // Use closure on the input
        let output = self(input).await;

        let response = /* Create `http::Response` from `output: Operation0Output` */
        response
    }
}

impl<Fun, Fut> Handler<Extension<S>, Operation0Input> for Fun
where
    Fun: FnOnce(Operation0Input, Extension<S>) -> Fut,
    Fut: Future<Output = Operation0Output>,
{
    async fn call(self, request: http::Request) -> http::Response {
        let input = /* Create `Operation0Input` from `request: http::Request` */;

        // Use closure on the input and fetched extension data
        let extension = Extension(request.extensions().get::<T>().clone());
        let output = self(input, extension).await;

        let response = /* Create `http::Response` from `output: Operation0Output` */
        response
    }
}
```

Creating `Operation0Input` from a `http::Request` and `http::Response` from a `Operation0Output` involves the HTTP binding traits and protocol aware deserialization. The structure [RuntimeError](https://github.com/awslabs/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/rust-runtime/aws-smithy-http-server/src/runtime_error.rs#L53-L5) enumerates internal error cases such as serialization/deserialization failures, `extensions().get::<T>()` failures, etc. We omit error handling in the snippets above, but, in full, they also involve protocol aware conversions from the `RuntimeError` and `http::Response`. The reader should make note of the influence of the model/protocol on the different sections of this procedure.

The `request.extensions().get::<T>()` present in the `Fun: FnOnce(Operation0Input, Extension<S>) -> Fut` implementation is the current approach to injecting state into handlers. The customer is required to apply a [AddExtensionLayer](https://docs.rs/tower-http/latest/tower_http/add_extension/struct.AddExtensionLayer.html) to the output of the service builder so that, when the request reaches the handler, the `extensions().get::<T>()` will succeed.

To convert the closures described above into a `Service` `OperationHandler` is used:

```rust
pub struct OperationHandler<H, T, Input> {
    handler: H,
    _marker: PhantomData<(T, Input)>,
}

impl<H, T, Input> Service<Request<B>> for OperationHandler<H, T, Input>
where
    H: Handler<T, I>,
{
    type Response = http::Response;
    type Error = Infallible;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    async fn call(&mut self, req: Request<B>) -> Result<Self::Response, Self::Error> {
        self.handler.call(req).await.map(Ok)
    }
}
```

The service build uses both `Handler` and `OperationHandler`

### Builder

The service builder we currently provide to the customer takes the form of the `OperationRegistryBuilder`, generated from [ServerOperationRegistryGenerator.kt](https://github.com/awslabs/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationRegistryGenerator.kt).

Currently, the reference model would generate the following `OperationRegistryBuilder` and `OperationRegistry`:

```rust
pub struct OperationRegistryBuilder<Op0, In0, Op1, In1> {
    operation1: Option<Op0>,
    operation2: Option<Op1>,
    _phantom: PhantomData<(In0, In1)>,
}

pub struct OperationRegistry<Op0, In0, Op1, In1> {
    operation1: Op0,
    operation2: Op1,
    _phantom: PhantomData<(In0, In1)>,
}
```

The `OperationRegistryBuilder` includes a setter per operation, and a fallible `build` method:

```rust
impl<Op0, In0, Op1, In1> OperationRegistryBuilder<Op0, In0, Op1, In1> {
    pub fn operation0(mut self, value: Op0) -> Self {
        self.operation0 = Some(value);
        self
    }
    pub fn operation1(mut self, value: Op1) -> Self {
        self.operation1 = Some(value);
        self
    }
    pub fn build(
        self,
    ) -> Result<OperationRegistry<Op0, In0, Op1, In1>, OperationRegistryBuilderError> {
        Ok(OperationRegistry {
            operation0: self.operation0.ok_or(/* OperationRegistryBuilderError */)?,
            operation1: self.operation1.ok_or(/* OperationRegistryBuilderError */)?,
            _phantom: PhantomData,
        })
    }
}
```

The `OperationRegistry` does not include any methods of its own, however it does enjoy a `From<OperationRegistry> for Router<B>` implementation:

```rust
impl<B, Op0, In0, Op1, In1> From<OperationRegistry<B, Op0, In0, Op1, In1>> for Router<B>
where
    Op0: Handler<B, In0, Operation0Input>,
    Op1: Handler<B, In1, Operation1Input>,
{
    fn from(registry: OperationRegistry<B, Op0, In0, Op1, In1>) -> Self {
        let operation0_request_spec = /* Construct Operation0 routing information */;
        let operation1_request_spec = /* Construct Operation1 routing information */;

        // Convert handlers into boxed services
        let operation0_svc = Box::new(OperationHandler::new(registry.operation0));
        let operation1_svc = Box::new(OperationHandler::new(registry.operation1));

        // Initialize the protocol specific router
        // We demonstrate it here with `new_rest_json_router`, but note that there is a different router constructor
        // for each protocol.
        aws_smithy_http_server::routing::Router::new_rest_json_router(vec![
            (
                operation0_request_spec,
                operation0_svc
            ),
            (
                operation1_request_spec,
                operation1_svc
            )
        ])
    }
}
```

A customer using this API would follow this kind of pattern:

```rust
async fn handler0(input: Operation0Input) -> Operation0Output {
    todo!()
}

async fn handler1(input: Operation1Input) -> Operation1Output {
    todo!()
}

let app: Router = OperationRegistryBuilder::default()
    // Use the setters
    .operation0(handler0)
    .operation1(handler1)
    // Convert to `OperationRegistry`
    .build()
    .unwrap()
    // Convert to `Router`
    .into();
```

### Router

The router today exists as

```rust
pub struct Route {
    service: BoxCloneService<http::Request, http::Response, Infallible>,
}

enum Routes {
    RestXml(Vec<(Route, RequestSpec)>),
    RestJson1(Vec<(Route, RequestSpec)>),
    AwsJson10(TinyMap<String, Route>),
    AwsJson11(TinyMap<String, Route>),
}

pub struct Router {
    routes: Routes,
}
```

and enjoys the following `Service<http::Request>` implementation:

```rust
impl Service<http::Request> for Router
{
    type Response = http::Response;
    type Error = Infallible;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    async fn call(&mut self, request: http::Request) -> Result<Self::Response, Self::Error> {
        match &self.routes {
            Routes::/* protocol */(routes) => {
                let route: Result<_, _> = /* perform route matching logic */;
                match route {
                    Ok(ok) => ok.oneshot().await,
                    Err(err) => /* Convert routing error into http::Response */
                }
            }
        }
    }
}
```
