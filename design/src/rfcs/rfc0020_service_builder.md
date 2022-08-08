# RFC: Service Builder Improvements

> Status: RFC

One might characterize `smithy-rs` as a tool for transforming a [Smithy service](https://awslabs.github.io/smithy/1.0/spec/core/model.html#service) into a [tower::Service](https://docs.rs/tower-service/latest/tower_service/trait.Service.html) builder. A Smithy model defines behavior of the generated service partially - handlers must be passed to the builder before the `tower::Service` is fully specified. This builder structure is the primary API surface we provide to the customer, as a result, it is important that it meets their needs.

This RFC proposes a new builder, deprecating the existing one, which addresses API deficiencies and takes steps to improve performance.

## Terminology

- **Model**: A [Smithy Model](https://awslabs.github.io/smithy/1.0/spec/core/model.html), usually pertaining to the one in use by the customer.
- **Smithy Service**: The entry point of an API that aggregates resources and operations together within a Smithy model. Described in detail [here](https://awslabs.github.io/smithy/1.0/spec/core/model.html#service).
- **Service**: The `tower::Service` trait is an interface for writing network applications in a modular and reusable way. `Service`s act on requests to produce responses.
- **Service Builder**: A `tower::Service` builder, generated from a Smithy service, by `smithy-rs`.
- **Middleware**: Broadly speaking, middleware modify requests and responses. Concretely, these are exist as implementations of [Layer](https://docs.rs/tower/latest/tower/layer/trait.Layer.html)/a `Service` wrapping an inner `Service`.
- **Handler**: A closure defining the behavior of a particular request after routing. These are provided to the service builder to complete the description of the service.

## Background

To provide context for the proposal we perform a survey of the current state of affairs.

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
        Operation1,
    ]
}
```

We have purposely omitted details from the model that are unimportant to describing the proposal. We also omit distracting details from the Rust snippets. Code generation is linear in the sense that, code snippets can be assumed to extend to multiple operations in a predictable way. In the case where we do want to speak generally about an operation and it's associated types, we use `{Operation}`, for example `{Operation}Input` is the input type of an unspecified operation.

Here is a quick example of what a customer might write when using the service builder:

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

During the survey we touch on the major mechanisms used to acheive this API.

### Handlers

A core concept in the service builder is the `Handler` trait:

```rust
pub trait Handler<T, Input> {
    async fn call(self, req: http::Request) -> http::Response;
}
```

Its purpose is to provide an even interface over closures of the form `FnOnce({Operation}Input) -> impl Future<Output = {Operation}Output>` and `FnOnce({Operation}Input, State) -> impl Future<Output = {Operation}Output>`. It's this abstraction which allows the customers to supply both `async fn handler(input: {Operation}Input) -> {Operation}Output` and `async fn handler(input: {Operation}Input, state: Extension<S>) -> {Operation}Output` to the service builder.

We generate `Handler` implementations for said closures in [ServerOperationHandlerGenerator.kt](https://github.com/awslabs/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationHandlerGenerator.kt):

```rust
impl<Fun, Fut> Handler<(), Operation0Input> for Fun
where
    Fun: FnOnce(Operation0Input) -> Fut,
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

Creating `{Operation}Input` from a `http::Request` and `http::Response` from a `{Operation}Output` involves the [HTTP binding traits](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html) and protocol aware serialization/deserialization. The [RuntimeError](https://github.com/awslabs/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/rust-runtime/aws-smithy-http-server/src/runtime_error.rs#L53-L5) enumerates error cases such as serialization/deserialization failures, `extensions().get::<T>()` failures, etc. We omit error handling in the snippets above, but, in full, they also involve protocol aware conversions from the `RuntimeError` and `http::Response`. The reader should make note of the influence of the model on the different sections of this procedure.

The `request.extensions().get::<T>()` present in the `Fun: FnOnce(Operation0Input, Extension<S>) -> Fut` implementation is the current approach to injecting state into handlers. The customer is required to apply a [AddExtensionLayer](https://docs.rs/tower-http/latest/tower_http/add_extension/struct.AddExtensionLayer.html) to the output of the service builder so that, when the request reaches the handler, the `extensions().get::<T>()` will succeed.

To convert the closures described above into a `Service` an `OperationHandler` is used:

```rust
pub struct OperationHandler<H, T, Input> {
    handler: H,
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

### Builder

The service builder we provide to the customer takes the form of the `OperationRegistryBuilder`, generated from [ServerOperationRegistryGenerator.kt](https://github.com/awslabs/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationRegistryGenerator.kt).

Currently, the reference model would generate the following `OperationRegistryBuilder` and `OperationRegistry`:

```rust
pub struct OperationRegistryBuilder<Op0, In0, Op1, In1> {
    operation1: Option<Op0>,
    operation2: Option<Op1>,
}

pub struct OperationRegistry<Op0, In0, Op1, In1> {
    operation1: Op0,
    operation2: Op1,
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

### Router

The [aws_smithy_http::routing::Router](https://github.com/awslabs/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/rust-runtime/aws-smithy-http-server/src/routing/mod.rs#L58-L60) provides the protocol aware routing of requests to their target service, it exists as

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
                let route: Result<Route, _> = /* perform route matching logic */;
                match route {
                    Ok(ok) => ok.oneshot().await,
                    Err(err) => /* Convert routing error into http::Response */
                }
            }
        }
    }
}
```

Along side the protocol specific constructors, `Router` includes a `layer` method. This provides a way for the customer to apply a `tower::Layer` to all routes. For every protocol, `Router::layer` has the approximately the same behavior:

```rust
let new_routes = old_routes
    .into_iter()
    // Apply the layer
    .map(|route| layer.layer(route))
    // Re-box the service, to restore `Route` type
    .map(|svc| Box::new(svc))
    // Collect the iterator back into a collection (`Vec` or `TinyMap`)
    .collect();
```

### Comparison to Axum

Historically, `smithy-rs` has borrowed from [axum](https://github.com/tokio-rs/axum). Despite various divergences the code bases still have much in common:

* Reliance on `Handler` trait to abstract over different closure signatures:
  * [axum::handler::Handler](https://docs.rs/axum/latest/axum/handler/trait.Handler.html)
  * [Handlers](#handlers)
* A mechanism for turning `H: Handler` into a `tower::Service`:
  * [axum::handler::IntoService](https://docs.rs/axum/latest/axum/handler/struct.IntoService.html)
  * [OperationHandler](#handlers)
* A `Router` to route requests to various handlers:
  * [axum::Router](https://docs.rs/axum/latest/axum/struct.Router.html)
  * [aws_smithy_http_server::routing::Router](#router)

To identify where the implementations should differ we should classify in what ways the use cases differ. There are two primary areas which we describe below.

#### Extractors and Responses

In `axum` there is a notion of [Extractor](https://docs.rs/axum/latest/axum/extract/index.html), which allows the customer to easily define a decomposition of an incoming `http::Request` by specify the arguments to the handlers. For example,

```rust
async fn request(Json(payload): Json<Value>, Query(params): Query<HashMap<String, String>>, headers: HeaderMap) {
    todo!()
}
```

is a valid handler - each argument satisfies the [axum::extract::FromRequest](https://docs.rs/axum/latest/axum/extract/trait.FromRequest.html) trait, therefore satisfies one of `axum` blanket `Handler` implementations:

```rust
macro_rules! impl_handler {
    ( $($ty:ident),* $(,)? ) => {
        impl<F, Fut, Res, $($ty,)*> Handler<($($ty,)*)> for F
        where
            F: FnOnce($($ty,)*) -> Fut + Clone + Send + 'static,
            Fut: Future<Output = Res> + Send,
            Res: IntoResponse,
            $( $ty: FromRequest + Send,)*
        {
            fn call(self, req: http::Request) -> Self::Future {
                async {
                    let mut req = RequestParts::new(req);

                    $(
                        let $ty = match $ty::from_request(&mut req).await {
                            Ok(value) => value,
                            Err(rejection) => return rejection.into_response(),
                        };
                    )*

                    let res = self($($ty,)*).await;

                    res.into_response()
                }
            }
        }
    };
}
```

The implementations of `Handler` in `axum` and `smithy-rs` follow a similar pattern - convert `http::Request` into the closures input, run the closure, convert the output of the closure to `http::Response`.

In `smithy-rs` we do not need a notion of "extractor", that role is fulfilled by HTTP binding traits. In `smithy-rs` the `http::Request` decomposition is determined by the Smithy model and the service protocol, whereas in `axum` it's defined by the handlers signature. In `smithy-rs` the only remaining degree of freedom in the signature of the handler is whether or not state is included.

Dual to `FromRequest` is the [axum::response::IntoResponse](https://docs.rs/axum/latest/axum/response/trait.IntoResponse.html) trait, this plays the role of converting the output of the handler to `http::Response`. Again, the difference between `axum` and `smithy-rs` is that `smithy-rs` has the conversion from `{Operation}Output` to `http::Response` specified by the Smithy model, whereas `axum` the customer is free to specify a return type which implements `axum::response::IntoResponse`.

#### Routing

The Smithy model not only specifies the `http::Request` decomposition and `http::Response` composition for a given service, it also determines the routing. The `From<OperationRegistry>` implementation, described in [Builder](#builder), yields a fully formed router based on the protocol and [http traits](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#http-trait) specified.

This is in contrast to `axum`, where the user specifies the routing by use of various combinators included on the `axum::Router`, applied to other `tower::Service`s. In an `axum` application one might encounter the following code:

```rust
let user_routes = Router::new().route("/:id", /* service */);

let team_routes = Router::new().route("/", /* service */);

let api_routes = Router::new()
    .nest("/users", user_routes)
    .nest("/teams", team_routes);

let app = Router::new().nest("/api", api_routes);
```

Note that, in `axum` handlers are quickly converted to `tower::Service` (via `IntoService`) before they are passed into the `Router`. In contrast, in `smithy-rs` handlers are passed into a builder and then the conversion to `tower::Service` is performed (via `OperationHandler`).

Introducing state to handlers in `axum` is done in the same way as `smithy-rs`, described briefly in [Handlers](#handlers) - a layer is used to insert state into incoming `http::Request`s and the `Handler` implementation pops it out of the type map layer. In `axum`, if a customer wanted to scope state to all routes within `/users/` they are able to do the following:

```rust
async fn handler(Extension(state): Extension</* State */>) -> /* Return Type */ {}

let api_routes = Router::new()
    .nest("/users", user_routes.layer(Extension(/* state */)))
    .nest("/teams", team_routes);
```

In `smithy-rs` a customer is only able to apply a layer to either the `aws_smithy_http::routing::Router` or every route via the [layer method](#router) described above.

# Proposal

The proposal is presented as a series of compatible transforms to the existing service builder, each paired with a motivation. Most of these can be independently implemented, but in the case where there exists an interdependency it is stated.

Although presented as a mutation to the existing service builder, the actual implementation should exist as a entirely separate builder - reusing code generation from the old builder but exposes a new Rust API. Preserving the old API surface will prevent breakage and make it easier to perform comparative benchmarks and testing.

## Remove two-step build procedure

As described in [Builder](#builder), the customer is required to perform two conversions. One from `OperationRegistryBuilder` via `OperationRegistryBuilder::build`, the second from `OperationRegistryBuilder` to `Router` via the `From<OperationRegistry> for Router` implementation. The intermediary stop at `OperationRegistry` is not required and can be removed.

## Statically check for missing Handlers

As described in [Builder](#builder), the `OperationRegistryBuilder::build` method is fallible - it yields a runtime error when one of the handlers has not been set.

```rust
    pub fn build(
        self,
    ) -> Result<OperationRegistry<Op0, In0, Op1, In1>, OperationRegistryBuilderError> {
        Ok(OperationRegistry {
            operation0: self.operation0.ok_or(/* OperationRegistryBuilderError */)?,
            operation1: self.operation1.ok_or(/* OperationRegistryBuilderError */)?,
        })
    }
```

We can do away with fallibility if we allow for on `Op0`, `Op1`, to switch types during build. For example,

```rust
impl OperationRegistryBuilder<Op0, In0, Op1, In1> {
    pub fn operation0<NewOp0>(mut self, value: NewOp0) -> OperationRegistryBuilder<NewOp0, In0, Op1, In1> {
        OperationRegistryBuilder {
            operation0: value,
            operation1: self.operation1
        }
    }
    pub fn operation1<NewOp1>(mut self, value: NewOp1) -> OperationRegistryBuilder<Op0, In0, NewOp1, In1> {
        OperationRegistryBuilder {
            operation0: self.operation0,
            operation1: value
        }
    }
}

impl OperationRegistryBuilder<Op0, In0, Op1, In1>
where
    Op0: Handler<B, In0, Operation0Input>,
    Op1: Handler<B, In1, Operation1Input>,
{
    pub fn build(self) -> OperationRegistry<Op0, In0, Op1, In1> {
        OperationRegistry {
            operation0: self.operation0,
            operation1: self.operation1,
        }
    }
}
```

The customer will now get a compile time error rather than a runtime error when they fail to specify a handler.

## Switch `From<OperationRegistry> for Router` to a `OperationRegistry::build` method

To construct a `Router`, the customer must either give a type ascription

```rust
let app: Router = /* Service builder */.into();
```

or be explicit about the `Router` namespace

```rust
let app = Router::from(/* Service builder */);
```

If we switch from a `From<OperationRegistry> for Router` to a `build` method on `OperationRegistry` the customer may simply

```rust
let app = /* Service builder */.build();
```

## Operations as Middleware

As mentioned in [Comparison to Axum: Routing](#routing) and [Handlers](#handlers), the `smithy-rs` service builder accepts handlers and only converts them into a `tower::Service` during the final conversion into a `Router`. There are downsides to this:

1. The customer has no opportunity to apply middleware to a specific operation before they are all collected into `Router`. The `Router` does have a `layer` method, described in [Router](#router), but this applies the middleware uniformly across all operations.
2. The builder has no way to apply middleware around customer applied middleware. A concrete example of where this would be useful is described in the [Middleware Position](rfc0018_logging_sensitive.md#middleware-position) section of [RFC: Logging in the Presence of Sensitive Data](rfc0018_logging_sensitive.md).
3. The customer has no way of expressing readiness of the underlying operation - all handlers are converted to services with [Service::poll_ready](https://docs.rs/tower/latest/tower/trait.Service.html#tymethod.poll_ready) returning `Poll::Ready(Ok(()))`.

The three use cases described above are supported by `axum` by virtue of the [Router::route](https://docs.rs/axum/latest/axum/routing/struct.Router.html#method.route) method accepting a `tower::Service`. The reader should consider a similar approach where the service builder setters accept a `tower::Service<http::Request, Response = http::Response>` rather than the `Handler`.

The smallest changeset to make progress towards these problems would require the customer eagerly uses `OperationHandler::new` rather than it being applied internally within `From<OperationRegistry> for Router` (see [Handlers](#handlers)). The usage would become

```rust
async fn handler0(input: Operation0Input) -> Operation0Output {
    todo!()
}

async fn handler1(input: Operation1Input, state: State) -> Operation1Output {
    todo!()
}

// Create a `Service<http::Request, Response = http::Response, Error = Infallible>` eagerly
let svc = OperationHandler::new(handler0);

// Middleware can be applied at this point
let operation0 = http_layer.layer(op1_svc);

let operation1 = OperationHandler::new(handler1);

OperationRegistryBuilder::default()
    .operation0(operation0)
    .operation1(operation1)
    /* ... */
```

Note that this requires that the `OperationRegistryBuilder` stores services, rather than `Handler`s. An unintended and superficial benefit of this is that we are able to drop `In{n}` from the `OperationRegistryBuilder<Op0, In0, Op1, In1>`, only `Op{n}` remains and it parametrizes each operations `tower::Service`.

It is still possible to retain the original API which accepts `Handler` by introducing the following setters:

```rust
impl<Op1, Op2> OperationRegistry<Op1, Op2> {
    fn operation0_handler<H: Handler>(self, handler: H) -> OperationRegistryBuilder<OperationHandler<H>, Op2> {
        self.operation0(OperationHandler::new(handler))
    }
}
```

There are two points at which the customer might want to apply middleware: around `tower::Service<{Operation}Input, Response = {Operation}Output>` and `tower::Service<http::Response, Response = http::Response>`, that is, before and after the model aware serialization/deserialization is performed. The change described only succeeds in the later, and therefore is only a partial solution to (1).

This solves (2), the service builder may apply additional middleware around the service.

This does not solve (3), as the customer is not permitted to provide a `Service`.

<!-- In this way the we separate the building of the entire service from the building of individual operations, this leads us to one of the central tenets of this proposal: operations as middleware. More concretely, the customer is provided with the following generated code

```rust
struct Operation0<S> {
    inner: S,
}

impl<S> Service<http::Request> for Operation0<S>
where
    S: Service<Operation0Input, Response = Operation0Output, Error = Infallible>
{
    type Response = http::Response;
    type Error = Infallible;

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // We defer to the inner service for readiness
        self.inner.poll_ready(cx)
    }

    async fn call(&mut self, request: http::Request) -> Result<Self::Response, Self::Error> {
        let input = /* Create `Operation0Input` from `request: http::Request` */;

        self.inner.call(input).await;

        let response = /* Create `http::Response` from `output: Operation0Output` */
        response
    }
}

fn make_operation_0<S>(inner: S) -> Operation0<S> {
    Operation0 {
        inner
    }
}
```

which is concerned with all the same model aware serialization/deserialization we noted in [Handlers](#handlers). -->

## Protocol specific routers

## Protocol specific errors

## Type erasure with the name of the Smithy service
