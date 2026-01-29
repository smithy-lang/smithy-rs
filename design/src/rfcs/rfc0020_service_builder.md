# RFC: Service Builder Improvements

> Status: Accepted

One might characterize `smithy-rs` as a tool for transforming a [Smithy service](https://awslabs.github.io/smithy/1.0/spec/core/model.html#service) into a [tower::Service](https://docs.rs/tower-service/latest/tower_service/trait.Service.html) builder. A Smithy model defines behavior of the generated service partially - handlers must be passed to the builder before the `tower::Service` is fully specified. This builder structure is the primary API surface we provide to the customer, as a result, it is important that it meets their needs.

This RFC proposes a new builder, deprecating the existing one, which addresses API deficiencies and takes steps to improve performance.

## Terminology

- **Model**: A [Smithy Model](https://awslabs.github.io/smithy/1.0/spec/core/model.html), usually pertaining to the one in use by the customer.
- **Smithy Service**: The entry point of an API that aggregates [resources](https://awslabs.github.io/smithy/1.0/spec/core/model.html#resource) and [operations](https://awslabs.github.io/smithy/1.0/spec/core/model.html#operation) together within a Smithy model. Described in detail [here](https://awslabs.github.io/smithy/1.0/spec/core/model.html#service).
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

@restJson1
service Service0 {
    operations: [
        Operation0,
        Operation1,
    ]
}
```

We have purposely omitted details from the model that are unimportant to describing the proposal. We also omit distracting details from the Rust snippets. Code generation is linear in the sense that, code snippets can be assumed to extend to multiple operations in a predictable way. In the case where we do want to speak generally about an operation and its associated types, we use `{Operation}`, for example `{Operation}Input` is the input type of an unspecified operation.

Here is a quick example of what a customer might write when using the service builder:

```rust,ignore
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

During the survey we touch on the major mechanisms used to achieve this API.

### Handlers

A core concept in the service builder is the `Handler` trait:

```rust,ignore
pub trait Handler<T, Input> {
    async fn call(self, req: http::Request) -> http::Response;
}
```

Its purpose is to provide an even interface over closures of the form `FnOnce({Operation}Input) -> impl Future<Output = {Operation}Output>` and `FnOnce({Operation}Input, State) -> impl Future<Output = {Operation}Output>`. It's this abstraction which allows the customers to supply both `async fn handler(input: {Operation}Input) -> {Operation}Output` and `async fn handler(input: {Operation}Input, state: Extension<S>) -> {Operation}Output` to the service builder.

We generate `Handler` implementations for said closures in [ServerOperationHandlerGenerator.kt](https://github.com/smithy-lang/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationHandlerGenerator.kt):

```rust,ignore
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

Creating `{Operation}Input` from a `http::Request` and `http::Response` from a `{Operation}Output` involves protocol aware serialization/deserialization, for example, it can involve the [HTTP binding traits](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html). The [RuntimeError](https://github.com/smithy-lang/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/rust-runtime/aws-smithy-http-server/src/runtime_error.rs#L53-L5) enumerates error cases such as serialization/deserialization failures, `extensions().get::<T>()` failures, etc. We omit error handling in the snippet above, but, in full, it also involves protocol aware conversions from the `RuntimeError` to `http::Response`. The reader should make note of the influence of the model on the different sections of this procedure.

The `request.extensions().get::<T>()` present in the `Fun: FnOnce(Operation0Input, Extension<S>) -> Fut` implementation is the current approach to injecting state into handlers. The customer is required to apply a [AddExtensionLayer](https://docs.rs/tower-http/latest/tower_http/add_extension/struct.AddExtensionLayer.html) to the output of the service builder so that, when the request reaches the handler, the `extensions().get::<T>()` will succeed.

To convert the closures described above into a `Service` an `OperationHandler` is used:

```rust,ignore
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

The service builder we provide to the customer is the `OperationRegistryBuilder`, generated from [ServerOperationRegistryGenerator.kt](https://github.com/smithy-lang/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerOperationRegistryGenerator.kt).

Currently, the reference model would generate the following `OperationRegistryBuilder` and `OperationRegistry`:

```rust,ignore
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

```rust,ignore
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

```rust,ignore
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

The [aws_smithy_http::routing::Router](https://github.com/smithy-lang/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/rust-runtime/aws-smithy-http-server/src/routing/mod.rs#L58-L60) provides the protocol aware routing of requests to their target , it exists as

```rust,ignore
pub struct Route {
    service: Box<dyn Service<http::Request, Response = http::Response>>,
}

enum Routes {
    RestXml(Vec<(Route, RequestSpec)>),
    RestJson1(Vec<(Route, RequestSpec)>),
    AwsJson1_0(TinyMap<String, Route>),
    AwsJson11(TinyMap<String, Route>),
}

pub struct Router {
    routes: Routes,
}
```

and has the following `Service<http::Request>` implementation:

```rust,ignore
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

```rust,ignore
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

- Reliance on `Handler` trait to abstract over different closure signatures:
  - [axum::handler::Handler](https://docs.rs/axum/latest/axum/handler/trait.Handler.html)
  - [Handlers](#handlers)
- A mechanism for turning `H: Handler` into a `tower::Service`:
  - [axum::handler::IntoService](https://docs.rs/axum/latest/axum/handler/struct.IntoService.html)
  - [OperationHandler](#handlers)
- A `Router` to route requests to various handlers:
  - [axum::Router](https://docs.rs/axum/latest/axum/struct.Router.html)
  - [aws_smithy_http_server::routing::Router](#router)

To identify where the implementations should differ we should classify in what ways the use cases differ. There are two primary areas which we describe below.

#### Extractors and Responses

In `axum` there is a notion of [Extractor](https://docs.rs/axum/latest/axum/extract/index.html), which allows the customer to easily define a decomposition of an incoming `http::Request` by specifying the arguments to the handlers. For example,

```rust,ignore
async fn request(Json(payload): Json<Value>, Query(params): Query<HashMap<String, String>>, headers: HeaderMap) {
    todo!()
}
```

is a valid handler - each argument satisfies the [axum::extract::FromRequest](https://docs.rs/axum/latest/axum/extract/trait.FromRequest.html) trait, therefore satisfies one of `axum`s blanket `Handler` implementations:

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

The implementations of `Handler` in `axum` and `smithy-rs` follow a similar pattern - convert `http::Request` into the closure's input, run the closure, convert the output of the closure to `http::Response`.

In `smithy-rs` we do not need a general notion of "extractor" - the `http::Request` decomposition is specified by the Smithy model, whereas in `axum` it's defined by the handlers signature. Despite the Smithy specification the customer may still want an "escape hatch" to allow them access to data outside of the Smithy service inputs, for this reason we should continue to support a restricted notion of extractor. This will help support use cases such as passing [lambda_http::Context](https://docs.rs/lambda_http/latest/lambda_http/struct.Context.html) through to the handler despite it not being modeled in the Smithy model.

Dual to `FromRequest` is the [axum::response::IntoResponse](https://docs.rs/axum/latest/axum/response/trait.IntoResponse.html) trait. This plays the role of converting the output of the handler to `http::Response`. Again, the difference between `axum` and `smithy-rs` is that `smithy-rs` has the conversion from `{Operation}Output` to `http::Response` specified by the Smithy model, whereas in `axum` the customer is free to specify a return type which implements `axum::response::IntoResponse`.

#### Routing

The Smithy model not only specifies the `http::Request` decomposition and `http::Response` composition for a given service, it also determines the routing. The `From<OperationRegistry>` implementation, described in [Builder](#builder), yields a fully formed router based on the protocol and [http traits](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#http-trait) specified.

This is in contrast to `axum`, where the user specifies the routing by use of various combinators included on the `axum::Router`, applied to other `tower::Service`s. In an `axum` application one might encounter the following code:

```rust,ignore
let user_routes = Router::new().route("/:id", /* service */);

let team_routes = Router::new().route("/", /* service */);

let api_routes = Router::new()
    .nest("/users", user_routes)
    .nest("/teams", team_routes);

let app = Router::new().nest("/api", api_routes);
```

Note that, in `axum` handlers are eagerly converted to a `tower::Service` (via `IntoService`) before they are passed into the `Router`. In contrast, in `smithy-rs`, handlers are passed into a builder and then the conversion to `tower::Service` is performed (via `OperationHandler`).

Introducing state to handlers in `axum` is done in the same way as `smithy-rs`, described briefly in [Handlers](#handlers) - a layer is used to insert state into incoming `http::Request`s and the `Handler` implementation pops it out of the type map layer. In `axum`, if a customer wanted to scope state to all routes within `/users/` they are able to do the following:

```rust,ignore
async fn handler(Extension(state): Extension</* State */>) -> /* Return Type */ {}

let api_routes = Router::new()
    .nest("/users", user_routes.layer(Extension(/* state */)))
    .nest("/teams", team_routes);
```

In `smithy-rs` a customer is only able to apply a layer around the `aws_smithy_http::routing::Router` or around every route via the [layer method](#router) described above.

## Proposal

The proposal is presented as a series of compatible transforms to the existing service builder, each paired with a motivation. Most of these can be independently implemented, and it is stated in the cases where an interdependency exists.

Although presented as a mutation to the existing service builder, the actual implementation should exist as an entirely separate builder, living in a separate namespace, reusing code generation from the old builder, while exposing a new Rust API. Preserving the old API surface will prevent breakage and make it easier to perform comparative benchmarks and testing.

### Remove two-step build procedure

As described in [Builder](#builder), the customer is required to perform two conversions. One from `OperationRegistryBuilder` via `OperationRegistryBuilder::build`, the second from `OperationRegistryBuilder` to `Router` via the `From<OperationRegistry> for Router` implementation. The intermediary stop at `OperationRegistry` is not required and can be removed.

### Statically check for missing Handlers

As described in [Builder](#builder), the `OperationRegistryBuilder::build` method is fallible - it yields a runtime error when one of the handlers has not been set.

```rust,ignore
    pub fn build(
        self,
    ) -> Result<OperationRegistry<Op0, In0, Op1, In1>, OperationRegistryBuilderError> {
        Ok(OperationRegistry {
            operation0: self.operation0.ok_or(/* OperationRegistryBuilderError */)?,
            operation1: self.operation1.ok_or(/* OperationRegistryBuilderError */)?,
        })
    }
```

We can do away with fallibility if we allow for on `Op0`, `Op1` to switch types during build and remove the `Option` from around the fields. The `OperationRegistryBuilder` then becomes

```rust,ignore
struct OperationRegistryBuilder<Op0, Op1> {
    operation_0: Op0,
    operation_1: Op1
}

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

### Switch `From<OperationRegistry> for Router` to an `OperationRegistry::build` method

To construct a `Router`, the customer must either give a type ascription

```rust,ignore
let app: Router = /* Service builder */.into();
```

or be explicit about the `Router` namespace

```rust,ignore
let app = Router::from(/* Service builder */);
```

If we switch from a `From<OperationRegistry> for Router` to a `build` method on `OperationRegistry` the customer may simply

```rust,ignore
let app = /* Service builder */.build();
```

There already exists a `build` method taking `OperationRegistryBuilder` to `OperationRegistry`, this is removed in [Remove two-step build procedure](#remove-two-step-build-procedure). These two transforms pair well together for this reason.

### Operations as Middleware Constructors

As mentioned in [Comparison to Axum: Routing](#routing) and [Handlers](#handlers), the `smithy-rs` service builder accepts handlers and only converts them into a `tower::Service` during the final conversion into a `Router`. There are downsides to this:

1. The customer has no opportunity to apply middleware to a specific operation before they are all collected into `Router`. The `Router` does have a `layer` method, described in [Router](#router), but this applies the middleware uniformly across all operations.
2. The builder has no way to apply middleware around customer applied middleware. A concrete example of where this would be useful is described in the [Middleware Position](rfc0018_logging_sensitive.md#middleware-position) section of [RFC: Logging in the Presence of Sensitive Data](rfc0018_logging_sensitive.md).
3. The customer has no way of expressing readiness of the underlying operation - all handlers are converted to services with [Service::poll_ready](https://docs.rs/tower/latest/tower/trait.Service.html#tymethod.poll_ready) returning `Poll::Ready(Ok(()))`.

The three use cases described above are supported by `axum` by virtue of the [Router::route](https://docs.rs/axum/latest/axum/routing/struct.Router.html#method.route) method accepting a `tower::Service`. The reader should consider a similar approach where the service builder setters accept a `tower::Service<http::Request, Response = http::Response>` rather than the `Handler`.

Throughout this section we purposely ignore the existence of handlers accepting state alongside the `{Operation}Input`, this class of handlers serve as a distraction and can be accommodated with small perturbations from each approach.

#### Approach A: Customer uses `OperationHandler::new`

It's possible to make progress with a small changeset, by requiring the customer eagerly uses `OperationHandler::new` rather than it being applied internally within `From<OperationRegistry> for Router` (see [Handlers](#handlers)). The setter would then become:

```rust,ignore
pub struct OperationRegistryBuilder<Op0, Op1> {
    operation1: Option<Op0>,
    operation2: Option<Op1>
}

impl<Op0, Op1> OperationRegistryBuilder<Op0, Op1> {
    pub fn operation0(self, value: Op0) -> Self {
        self.operation1 = Some(value);
        self
    }
}
```

The API usage would then become

```rust,ignore
async fn handler0(input: Operation0Input) -> Operation0Output {
    todo!()
}

// Create a `Service<http::Request, Response = http::Response, Error = Infallible>` eagerly
let svc = OperationHandler::new(handler0);

// Middleware can be applied at this point
let operation0 = /* A HTTP `tower::Layer` */.layer(op1_svc);

OperationRegistryBuilder::default()
    .operation0(operation0)
    /* ... */
```

Note that this requires that the `OperationRegistryBuilder` stores services, rather than `Handler`s. An unintended and superficial benefit of this is that we are able to drop `In{n}` from the `OperationRegistryBuilder<Op0, In0, Op1, In1>` - only `Op{n}` remains and it parametrizes each operation's `tower::Service`.

It is still possible to retain the original API which accepts `Handler` by introducing the following setters:

```rust,ignore
impl<Op1, Op2> OperationRegistryBuilder<Op1, Op2> {
    fn operation0_handler<H: Handler>(self, handler: H) -> OperationRegistryBuilder<OperationHandler<H>, Op2> {
        OperationRegistryBuilder {
            operation0: OperationHandler::new(handler),
            operation1: self.operation1
        }
    }
}
```

There are two points at which the customer might want to apply middleware: around `tower::Service<{Operation}Input, Response = {Operation}Output>` and `tower::Service<http::Request, Response = http::Response>`, that is, before and after the serialization/deserialization is performed. The change described only succeeds in the latter, and therefore is only a partial solution to (1).

This solves (2), the service builder may apply additional middleware around the service.

This does not solve (3), as the customer is not able to provide a `tower::Service<{Operation}Input, Response = {Operation}Output>`.

#### Approach B: Operations as Middleware

In order to achieve all three we model operations as middleware:

```rust,ignore
pub struct Operation0<S> {
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
```

Notice the similarity between this and the `OperationHandler`, the only real difference being that we hold an inner service rather than a closure. In this way we have separated all model aware serialization/deserialization, we noted in [Handlers](#handlers), into this middleware.

A consequence of this is that the user `Operation0` must have two constructors:

- `from_service`, which takes a `tower::Service<Operation0Input, Response = Operation0Output>`.
- `from_handler`, which takes an async `Operation0Input -> Operation0Output`.

A brief example of how this might look:

```rust,ignore
use tower::util::{ServiceFn, service_fn};

impl<S> Operation0<S> {
    pub fn from_service(inner: S) -> Self {
        Self {
            inner,
        }
    }
}

impl<F> Operation0<ServiceFn<F>> {
    pub fn from_handler(inner: F) -> Self {
        // Using `service_fn` here isn't strictly correct - there is slight misalignment of closure signatures. This
        // still serves to illustrate the proposal.
        Operation0::from_service(service_fn(inner))
    }
}
```

The API usage then becomes:

```rust,ignore
async fn handler(input: Operation0Input) -> Operation0Output {
    todo!()
}

// These are both `tower::Service` and hence can have middleware applied to them
let operation_0 = Operation0::from_handler(handler);
let operation_1 = Operation1::from_service(/* some service */);

OperationRegistryBuilder::default()
    .operation0(operation_0)
    .operation1(operation_1)
    /* ... */
```

#### Approach C: Operations as Middleware Constructors

While [Attempt B](#approach-b-operations-as-middleware) solves all three problems, it fails to adequately model the Smithy semantics. An operation cannot uniquely define a `tower::Service` without reference to a parent Smithy service - information concerning the serialization/deserialization, error modes are all inherited from the Smithy service an operation is used within. In this way, `Operation0` should not be a standalone middleware, but become middleware once accepted by the service builder.

Any solution which provides an `{Operation}` structure and wishes it to be accepted by multiple service builders must deal with this problem. We currently build one library per service and hence have duplicate structures when [service closures](https://awslabs.github.io/smithy/1.0/spec/core/model.html#service-closure) overlap. This means we wouldn't run into this problem today, but it would be a future obstruction if we wanted to reduce the amount of generated code.

```rust,ignore
use tower::layer::util::{Stack, Identity};
use tower::util::{ServiceFn, service_fn};

// This takes the same form as `Operation0` defined in the previous attempt. The difference being that this is now
// private.
struct Service0Operation0<S> {
    inner: S
}

impl<S> Service<http::Request> for ServiceOperation0<S>
where
    S: Service<Operation0Input, Response = Operation0Output, Error = Infallible>
{
    /* Same as above */
}

pub struct Operation0<S, L> {
    inner: S,
    layer: L
}

impl<S> Operation0<S, Identity> {
    pub fn from_service(inner: S) -> Self {
        Self {
            inner,
            layer: Identity
        }
    }
}

impl<F> Operation0<ServiceFn<F>, Identity> {
    pub fn from_handler(inner: F) -> Self {
        Operation0::from_service(service_fn(inner))
    }
}

impl<S, L> Operation0<S, L> {
    pub fn layer<NewL>(self, layer: L) -> Operation0<S, Stack<L, NewL>> {
        Operation0 {
            inner: self.inner,
            layer: Stack::new(self.layer, layer)
        }
    }

    pub fn logging(self, /* args */) -> Operation0<S, Stack<L, LoggingLayer>> {
        Operation0 {
            inner: self.inner,
            layer: Stack::new(self.layer, LoggingLayer::new(/* args */))
        }
    }

    pub fn auth(self, /* args */) -> Operation0<S, Stack<L, AuthLayer>> {
        Operation0 {
            inner: self.inner,
            layer: Stack::new(self.layer, /* Construct auth middleware */)
        }

    }
}

impl<Op1, Op2> OperationRegistryBuilder<Op1, Op2> {
    pub fn operation0<S, L>(self, operation: Operation0<S, L>) -> OperationRegistryBuilder<<L as Layer<Service0Operation0<S>>::Service, Op2>
    where
        L: Layer<Service0Operation0<S>>
    {
        // Convert `Operation0` to a `tower::Service`.
        let http_svc = Service0Operation0 { inner: operation.inner };
        // Apply the layers
        operation.layer(http_svc)
    }
}
```

Notice that we get some additional type safety here when compared to [Approach A](#approach-a-customer-uses-operationhandlernew) and [Approach B](#approach-b-operations-as-middleware) - `operation0` accepts a `Operation0` rather than a general `tower::Service`. We also get a namespace to include utility methods - notice the `logging` and `auth` methods.

The RFC favours this approach out of all those presented.

#### Approach D: Add more methods to the Service Builder

An alternative to [Approach C](#approach-c-operations-as-middleware-constructors) is to simply add more methods to the service builder while internally storing a `tower::Service`:

- `operation0_from_service`, accepts a `tower::Service<Operation0Input, Response = Operation0Output>`.
- `operation0_from_handler`, accepts an async `Fn(Operation0Input) -> Operation0Output`.
- `operation0_layer`, accepts a `tower::Layer<Op0>`.

This is functionally similar to [Attempt C](#approach-c-operations-as-middleware-constructors) except that all composition is done internal to the service builder and the namespace exists in the method name, rather than the `{Operation}` struct.

### Service parameterized Routers

Currently the `Router` stores `Box<dyn tower::Service<http::Request, Response = http::Response>`. As a result the `Router::layer` method, seen in [Router](#router), must re-box a service after every `tower::Layer` applied. The heap allocation `Box::new` itself is not cause for concern because `Router`s are typically constructed once at startup, however one might expect the indirection to regress performance when the server is running.

Having the service type parameterized as `Router<S>`, allows us to write:

```rust,ignore
impl<S> Router<S> {
    fn layer<L>(self, layer: &L) -> Router<L::Service>
    where
        L: Layer<S>
    {
        /* Same internal implementation without boxing */
    }
}
```

### Protocol specific Routers

Currently there is a single `Router` structure, described in [Router](#router), situated in the `rust-runtime/aws-smithy-http-server` crate, which is output by the service builder. This, roughly, takes the form of an `enum` listing the different protocols.

```rust
#[derive(Debug)]
enum Routes {
    RestXml(/* Container */),
    RestJson1(/* Container */),
    AwsJson1_0(/* Container */),
    AwsJson1_1(/* Container */),
}
```

Recall the form of the `Service::call` method, given in [Router](#router), which involved matching on the protocol and then performing protocol specific logic.

Two downsides of modelling `Router` in this way are:

- `Router` is larger and has more branches than a protocol specific implementation.
- If a third-party wanted to extend `smithy-rs` to additional protocols `Routes` would have to be extended. A synopsis of this obstruction is presented in [Should we generate the `Router` type](https://github.com/smithy-lang/smithy-rs/issues/1606) issue.

After taking the [Switch `From<OperationRegistry> for Router` to an `OperationRegistry::build` method](#switch-fromoperationregistry-for-router-to-an-operationregistrybuild-method) transform, code generation is free to switch between return types based on the model. This allows for a scenario where a `@restJson1` causes the service builder to output a specific `RestJson1Router`.

### Protocol specific Errors

Currently, protocol specific routing errors are either:

- Converted to `RuntimeError`s and then `http::Response` (see [unknown_operation](https://github.com/smithy-lang/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/rust-runtime/aws-smithy-http-server/src/routing/mod.rs#L106-L118)).
- Converted directly to a `http::Response` (see [method_not_allowed](https://github.com/smithy-lang/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/rust-runtime/aws-smithy-http-server/src/routing/mod.rs#L121-L127)). This is an outlier to the common pattern.

The `from_request` functions yield protocol specific errors which are converted to `RequestRejection`s then `RuntimeError`s (see [ServerHttpBoundProtocolGenerator.kt](https://github.com/smithy-lang/smithy-rs/blob/458eeb63b95e6e1e26de0858457adbc0b39cbe4e/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/protocols/ServerHttpBoundProtocolGenerator.kt#L194-L210)).

In these scenarios protocol specific errors are converted into `RuntimeError` before being converted to a `http::Response` via `into_response` method.

Two downsides of this are:

- `RuntimeError` enumerates all possible errors across all existing protocols, so is larger than modelling the errors for a specific protocol.
- If a third-party wanted to extend `smithy-rs` to additional protocols with differing failure modes `RuntimeError` would have to be extended. As in [Protocol specific Errors](#protocol-specific-errors), a synopsis of this obstruction is presented in [Should we generate the `Router` type](https://github.com/smithy-lang/smithy-rs/issues/1606) issue.

Switching from using `RuntimeError` to protocol specific errors which satisfy a common interface, `IntoResponse`, would resolve these problem.

### Type erasure with the name of the Smithy service

Currently the service builder is named `OperationRegistryBuilder`. Despite the name being model agnostic, the `OperationRegistryBuilder` mutates when the associated service mutates. Renaming `OperationRegistryBuilder` to `{Service}Builder` would reflect the relationship between the builder and the Smithy service and prevent naming conflicts if multiple service builders are to exist in the same namespace.

Similarly, the output of the service builder is `Router`. This ties the output of the service builder to a structure in `rust-runtime`. Introducing a type erasure here around `Router` using a newtype named `{Service}` would:

- Ensure we are free to change the implementation of `{Service}` without changing the `Router` implementation.
- Hide the router type, which is determined by the protocol specified in the model.
- Allow us to put a `builder` method on `{Service}` which returns `{Service}Builder`.

This is compatible with [Protocol specific Routers](#protocol-specific-routers), we simply newtype the protocol specific router rather than `Router`.

With both of these changes the API would take the form:

```rust,ignore
let service_0: Service0 = Service0::builder()
    /* use the setters */
    .build()
    .unwrap()
    .into();
```

With [Remove two-step build procedure](#remove-two-step-build-procedure), [Switch `From<OperationRegistry> for Router` to a `OperationRegistry::build` method](#switch-fromoperationregistry-for-router-to-an-operationregistrybuild-method), and [Statically check for missing Handlers](#statically-check-for-missing-handlers) we obtain the following API:

```rust,ignore
let service_0: Service0 = Service0::builder()
    /* use the setters */
    .build();
```

### Combined Proposal

A combination of all the proposed transformations results in the following API:

```rust,ignore
struct Context {
    /* fields */
}

async fn handler(input: Operation0Input) -> Operation0Output {
    todo!()
}

async fn handler_with_ext(input: Operation0Input, extension: Extension<Context>) -> Operation0Output {
    todo!()
}

struct Operation1Service {
    /* fields */
}

impl Service<Operation1Input> for Operation1Service {
    type Response = Operation1Output;

    /* implementation */
}

struct Operation1ServiceWithExt {
    /* fields */
}

impl Service<(Operation1Input, Extension<Context>)> for Operation1Service {
    type Response = Operation1Output;

    /* implementation */
}

// Create an operation from a handler
let operation_0 = Operation0::from_handler(handler);

// Create an operation from a handler with extension
let operation_0 = Operation::from_handler(handler_with_ext);

// Create an operation from a `tower::Service`
let operation_1_svc = Operation1Service { /* initialize */ };
let operation_1 = Operation::from_service(operation_1_svc);

// Create an operation from a `tower::Service` with extension
let operation_1_svc = Operation1ServiceWithExtension { /* initialize */ };
let operation_1 = Operation::from_service(operation_1_svc);

// Apply a layer
let operation_0 = operation_0.layer(/* layer */);

// Use the service builder
let service_0 = Service0::builder()
    .operation_0(operation_0)
    .operation_1(operation_1)
    .build();
```

A toy implementation of the combined proposal is presented in [this PR](https://github.com/hlbarber/service-builder/pull/1).

## Changes Checklist

- [x] Add protocol specific routers to `rust-runtime/aws-smithy-http-server`.
  - <https://github.com/smithy-lang/smithy-rs/pull/1666>
- [x] Add middleware primitives and error types to `rust-runtime/aws-smithy-http-server`.
  - <https://github.com/smithy-lang/smithy-rs/pull/1679>
- [x] Add code generation which outputs new service builder.
  - <https://github.com/smithy-lang/smithy-rs/pull/1693>
- [x] Deprecate `OperationRegistryBuilder`, `OperationRegistry` and `Router`.
  - <https://github.com/smithy-lang/smithy-rs/pull/1886>
  - <https://github.com/smithy-lang/smithy-rs/pull/2161>
