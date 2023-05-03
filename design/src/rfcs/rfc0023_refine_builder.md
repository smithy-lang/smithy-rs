RFC: Evolving the new service builder API
========================

> Status: Accepted
>
> Applies to: Server

[RFC 20] introduced a new service builder API.
It supports fine-grained configuration at multiple levels (per-handler middlewares, router middlewares, plugins) while trying to prevent some misconfiguration issues at compile-time (i.e. missing operation handlers).
There is consensus that the new API is an improvement over the pre-existing `OperationRegistryBuilder`/`OperationRegistry`, which is now on its way to deprecation in one of the next releases.

This RFC builds on top of [RFC 20] to explore an alternative API design prior to its stabilisation.
The API proposed in this RFC has been manually implemented for the Pokemon service. You can find the code [here](https://github.com/LukeMathWalker/builder-experiments).

## Overview

Type-heavy builders can lead to a poor developer experience when it comes to writing function signatures, conditional branches and clarity of error messages.
This RFC provides examples for the issues we are trying to mitigate and showcases an alternative design for the service builder, cutting generic parameters from 2*(N+1) to 2, where `N` is the number of operations on the service.
We rely on eagerly upgrading the registered handlers and operations to `Route<B>` to achieve this reduction.

Goals:

- Maximise API ergonomics, with a particular focus on the developer experience for Rust beginners.

Strategy:

- Reduce type complexity, exposing a less generic API;
- Provide clearer errors when the service builder is misconfigured.

Trade-offs:

- Reduce compile-time safety. Missing handlers will be detected at runtime instead of compile-time.

Constraints:

- There should be no significant degradation in runtime performance (i.e. startup time for applications).

## Handling missing operations

Let's start by reviewing the API proposed in [RFC 20]. We will use the [Pokemon service] as our driving example throughout the RFC.
This is what the startup code looks like:

```rust,ignore
#[tokio::main]
pub async fn main() {
    // [...]
    let app = PokemonService::builder()
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing)
        .check_health(check_health)
        .build();

    // Setup shared state and middlewares.
    let shared_state = Arc::new(State::default());
    let app = app.layer(&AddExtensionLayer::new(shared_state));

    // Start the [`hyper::Server`].
    let bind: SocketAddr = /* */;
    let server = hyper::Server::bind(&bind).serve(app.into_make_service());
    // [...]
}
```

The builder is infallible: we are able to verify at compile-time that all handlers have been provided using the [typestate builder pattern].

### Compiler errors cannot be tuned

What happens if we stray away from the happy path? We might forget, for example, to add the `check_health` handler.
The compiler greets us with this error:

```text
error[E0277]: the trait bound `MissingOperation: Upgradable<AwsRestJson1, CheckHealth, (), _, IdentityPlugin>` is not satisfied
  --> pokemon-service/src/bin/pokemon-service.rs:38:10
   |
38 |         .build();
   |          ^^^^^ the trait `Upgradable<AwsRestJson1, CheckHealth, (), _, IdentityPlugin>` is not implemented for `MissingOperation`
   |
   = help: the following other types implement trait `Upgradable<Protocol, Operation, Exts, B, Plugin>`:
             FailOnMissingOperation
             Operation<S, L>
```

The compiler complains that `MissingOperation` does not implement the `Upgradable` trait. Neither `MissingOperation` nor `Upgradable` appear in the startup code we looked at. This is likely to be the first time the developer sees those traits, assuming they haven't spent time getting familiar with `aws-smithy-http-server`'s internals.
The `help` section is unhelpful, if not actively misdirecting.
How can the developer figure out that the issue lies with `check_health`?
They need to inspect the generic parameters attached to `Upgradable` in the code label or the top-level error message - we see, among other things, a `CheckHealth` parameter. That is the hint they need to follow to move forward.

We unfortunately do not have agency on the compiler error we just examined. Rust does not expose hooks for crate authors to tweak the errors returned when a type does not implement a trait we defined.
All implementations of the [typestate builder pattern] accept this shortcoming in exchange for compile-time safety.

Is it a good tradeoff in our case?

### The cost of a runtime error

If `build` returns an error, the HTTP server is never launched. The application fails to start.

Let's examine the cost of this runtime error along two dimensions:
- Impact on developer productivity;
- Impact on end users.

We'd love for this issue to be caught on the developer machine - it provides the shortest feedback loop.
The issue won't be surfaced by a `cargo check` or `cargo build` invocation, as it happens with the typestate builder approach.
It should be surfaced by executing the application test suite, assuming that the developer has written at least a single integration test - e.g. a test that passes a request to the `call` method exposed by `PokemonService` or launches a full-blown instance of the application which is then probed via an HTTP client.

If there are no integration tests, the issue won't be detected on the developer machine nor in CI.
Nonetheless, it is unlikely to cause any end-user impact even if it manages to escape detection and reach production. The deployment will never complete if they are using a progressive rollout strategy: instances of the new version will crash as soon as they are launched, never getting a chance to mark themselves as healthy; all traffic will keep being handled by the old version, with no visible impact on end users of the application.

Given the above, we think that the impact of a runtime error is low enough to be worth exploring designs that do not guarantee compile-safety for the builder API[^further-dev-productivity-improvements].

### Providing clear feedback

Moving from a compile-time error to a runtime error does not require extensive refactoring.
The definition of `PokemonServiceBuilder` goes from:

```rust,ignore
pub struct PokemonServiceBuilder<
    Op1,
    Op2,
    Op3,
    Op4,
    Op5,
    Op6,
    Exts1 = (),
    Exts2 = (),
    Exts3 = (),
    Exts4 = (),
    Exts5 = (),
    Exts6 = (),
    Pl = aws_smithy_http_server::plugin::IdentityPlugin,
> {
    check_health: Op1,
    do_nothing: Op2,
    get_pokemon_species: Op3,
    get_server_statistics: Op4,
    capture_pokemon: Op5,
    get_storage: Op6,
    #[allow(unused_parens)]
    _exts: std::marker::PhantomData<(Exts1, Exts2, Exts3, Exts4, Exts5, Exts6)>,
    plugin: Pl,
}
```

to:

```rust,ignore
pub struct PokemonServiceBuilder<
    Op1,
    Op2,
    Op3,
    Op4,
    Op5,
    Op6,
    Exts1 = (),
    Exts2 = (),
    Exts3 = (),
    Exts4 = (),
    Exts5 = (),
    Exts6 = (),
    Pl = aws_smithy_http_server::plugin::IdentityPlugin,
> {
    check_health: Option<Op1>,
    do_nothing: Option<Op2>,
    get_pokemon_species: Option<Op3>,
    get_server_statistics: Option<Op4>,
    capture_pokemon: Option<Op5>,
    get_storage: Option<Op6>,
    #[allow(unused_parens)]
    _exts: std::marker::PhantomData<(Exts1, Exts2, Exts3, Exts4, Exts5, Exts6)>,
    plugin: Pl,
}
```

All operation fields are now `Option`-wrapped.
We introduce a new `MissingOperationsError` error to hold the names of the missing operations and their respective setter methods:

```rust,ignore
#[derive(Debug)]
pub struct MissingOperationsError {
    service_name: &'static str,
    operation_names2setter_methods: HashMap<&'static str, &'static str>,
}

impl Display for MissingOperationsError { /* */ }
impl std::error::Error for MissingOperationsError {}
```

which is then used in `build` as error type _(not shown here for brevity)_.
We can now try again to stray away from the happy path by forgetting to register a handler for the `CheckHealth` operation.
The code compiles just fine this time, but the application fails when launched via `cargo run`:

```text
<timestamp> ERROR pokemon_service: You must specify a handler for all operations attached to the `Pokemon` service.
We are missing handlers for the following operations:
- com.aws.example#CheckHealth

Use the dedicated methods on `PokemonServiceBuilder` to register the missing handlers:
- PokemonServiceBuilder::check_health
```

The error speaks the language of the domain, Smithy's interface definition language: it mentions operations, services, handlers.
Understanding the error requires no familiarity with `smithy-rs`' internal type machinery or advanced trait patterns in Rust.
We can also provide actionable suggestions: Rust beginners should be able to easily process the information, rectify the mistake and move on quickly.

## Simplifying `PokemonServiceBuilder`'s signature

Let's take a second look at the (updated) definition of `PokemonServiceBuilder`:

```rust,ignore
pub struct PokemonServiceBuilder<
    Op1,
    Op2,
    Op3,
    Op4,
    Op5,
    Op6,
    Exts1 = (),
    Exts2 = (),
    Exts3 = (),
    Exts4 = (),
    Exts5 = (),
    Exts6 = (),
    Pl = aws_smithy_http_server::plugin::IdentityPlugin,
> {
    check_health: Option<Op1>,
    do_nothing: Option<Op2>,
    get_pokemon_species: Option<Op3>,
    get_server_statistics: Option<Op4>,
    capture_pokemon: Option<Op5>,
    get_storage: Option<Op6>,
    #[allow(unused_parens)]
    _exts: std::marker::PhantomData<(Exts1, Exts2, Exts3, Exts4, Exts5, Exts6)>,
    plugin: Pl,
}
```

We have 13 generic parameters:
- 1 for plugins (`Pl`);
- 2 for each operation (`OpX` and `ExtsX`);

All those generic parameters were necessary when we were using the [typestate builder pattern]. They kept track of which operation handlers were missing: if any `OpX` was set to `MissingOperation` when calling `build` -> compilation error!

Do we still need all those generic parameters if we move forward with this RFC?
You might be asking yourselves: why do those generics bother us? Is there any harm in keeping them around?
We'll look at the impact of those generic parameters on two scenarios:
- Branching in startup logic;
- Breaking down a monolithic startup function into multiple smaller functions.

### Branching -> "Incompatible types"

Conditional statements appear quite often in the startup logic for an application (or in the setup code for its integration tests).
Let's consider a toy example: if a `check_database` flag is set to `true`, we want to register a different `check_health` handler - one that takes care of pinging the database to make sure it's up.

The "obvious" solution would look somewhat like this:

```rust,ignore
let check_database: bool = /* */;
let app = if check_database {
    app.check_health(check_health)
} else {
    app.check_health(check_health_with_database)
};
app.build();
```

The compiler is not pleased:

```text
error[E0308]: `if` and `else` have incompatible types
  --> pokemon-service/src/bin/pokemon-service.rs:39:9
   |
36 |       let app = if check_database {
   |  _______________-
37 | |         app.check_health(check_health)
   | |         ------------------------------ expected because of this
38 | |     } else {
39 | |         app.check_health(check_health_with_database)
   | |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected fn item, found a different fn item
40 | |     };
   | |_____- `if` and `else` have incompatible types
   |
   = note: expected struct `PokemonServiceBuilder<Operation<IntoService<_, fn(CheckHealthInput) -> impl Future<Output =
    CheckHealthOutput> {check_health}>>, _, _, _, _, _, _, _, _, _, _, _>`
              found struct `PokemonServiceBuilder<Operation<IntoService<_, fn(CheckHealthInput) -> impl Future<Output =
    CheckHealthOutput> {check_health_with_database}>>, _, _, _, _, _, _, _, _, _, _, _>`
```

The developer must be aware of the following facts to unpack the error message:
1. The two branches of an `if`/`else` statement need to return the same type.
2. Each function closure has a new unique type (represented as `fn(CheckHealthInput) -> impl Future<Output =
    CheckHealthOutput> {check_health}` for `check_health`);
3. The handler function type becomes part of the overall `PokemonServiceBuilder` type, a cog in the larger `Op1` generic parameter used to hold the handler for the `CheckHealth` operation (i.e. `Operation<IntoService<_, fn(CheckHealthInput) -> impl Future<Output =
    CheckHealthOutput> {check_health}>>`);

The second fact requires an intermediate understanding of Rust's closures and opaque types (`impl Trait`). It's quite likely to confuse Rust beginners.

The developer has three options to move forward:
1. Convert `check_health` and `check_health_with_database` into a common type that can be passed as a handler to `PokemonServiceBuilder::check_health`;
2. Invoke the `build` method inside the two branches in order to return a "plain" `PokemonService<Route<B>>` from both branches.
3. Embed the configuration parameter (`check_database`) in the application state, retrieve it inside `check_health` and perform the branching there.

I can't easily see a way to accomplish 1) using the current API. Pursuing 2) is straight-forward with a single conditional:

```rust,ignore
let check_database: bool = /* */;
let app = if check_database {
    app.check_health(check_health).build()
} else {
    app.check_health(check_health_with_database).build()
};
```

It becomes more cumbersome when we have more than a single conditional:

```rust,ignore
let check_database: bool = /* */;
let include_cpu_statics: bool = /* */;
match (check_database, include_cpu_statics) {
    (true, true) => app
        .check_health(check_health_with_database)
        .get_server_statistics(get_server_statistics_with_cpu)
        .build(),
    (true, false) => app
        .check_health(check_health_with_database)
        .get_server_statistics(get_server_statistics)
        .build(),
    (false, true) => app
        .check_health(check_health)
        .get_server_statistics(get_server_statistics_with_cpu())
        .build(),
    (false, false) => app
        .check_health(check_health)
        .get_server_statistics(get_server_statistics)
        .build(),
}
```

A lot of repetition compared to the code for the "obvious" approach:

```rust,ignore
let check_database: bool = /* */;
let include_cpu_statics: bool = /* */;
let app = if check_database {
    app.check_health(check_health)
} else {
    app.check_health(check_health_with_database)
};
let app = if include_cpu_statistics {
    app.get_server_statistics(get_server_statistics_with_cpu)
} else {
    app.get_server_statistics(get_server_statistics)
};
app.build();
```

The obvious approach becomes viable if we stop embedding the handler function type in `PokemonServiceBuilder`'s overall type.

### Refactoring into smaller functions -> Prepare for some type juggling!

Services with a high number of routes can lead to fairly long startup routines.
Developers might be tempted to break down the startup routine into smaller functions, grouping together operations with common requirements (similar domain, same middlewares, etc.).

What does the signature of those smaller functions look like?
The service builder must be one of the arguments if we want to register handlers. We must also return it to allow the orchestrating function to finish the application setup (our setters take ownership of `self`).

A first sketch:

```rust,ignore
fn partial_setup(builder: PokemonServiceBuilder) -> PokemonServiceBuilder {
    /* */
}
```

The compiler demands to see those generic parameters in the signature:

```text
error[E0107]: missing generics for struct `PokemonServiceBuilder`
  --> pokemon-service/src/bin/pokemon-service.rs:28:27
   |
28 | fn partial_setup(builder: PokemonServiceBuilder) -> PokemonServiceBuilder {
   |                           ^^^^^^^^^^^^^^^^^^^^^ expected at least 6 generic arguments
   |
note: struct defined here, with at least 6 generic parameters: `Op1`, `Op2`, `Op3`, `Op4`, `Op5`, `Op6`

error[E0107]: missing generics for struct `PokemonServiceBuilder`
  --> pokemon-service/src/bin/pokemon-service.rs:28:53
   |
28 | fn partial_setup(builder: PokemonServiceBuilder) -> PokemonServiceBuilder {
   |                                                     ^^^^^^^^^^^^^^^^^^^^^ expected at least 6 generic arguments
   |
note: struct defined here, with at least 6 generic parameters: `Op1`, `Op2`, `Op3`, `Op4`, `Op5`, `Op6`
```

We could try to nudge the compiler into inferring them:

```rust,ignore
fn partial_setup(
    builder: PokemonServiceBuilder<_, _, _, _, _, _>,
) -> PokemonServiceBuilder<_, _, _, _, _, _> {
    /* */
}
```

but that won't fly either:

```text
error[E0121]: the placeholder `_` is not allowed within types on item signatures for return types
  --> pokemon-service/src/bin/pokemon-service.rs:30:28
   |
30 | ) -> PokemonServiceBuilder<_, _, _, _, _, _> {
   |                            ^  ^  ^  ^  ^  ^ not allowed in type signatures
   |                            |  |  |  |  |
   |                            |  |  |  |  not allowed in type signatures
   |                            |  |  |  not allowed in type signatures
   |                            |  |  not allowed in type signatures
   |                            |  not allowed in type signatures
   |                            not allowed in type signatures
```

We must type it all out:

```rust,ignore
fn partial_setup<Op1, Op2, Op3, Op4, Op5, Op6>(
    builder: PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6>,
) -> PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6> {
    builder
}
```

That compiles, at last.
Let's try to register an operation handler now:

```rust,ignore
fn partial_setup<Op1, Op2, Op3, Op4, Op5, Op6>(
    builder: PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6>,
) -> PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6> {
    builder.get_server_statistics(get_server_statistics)
}
```

That looks innocent, but it doesn't fly:

```text
error[E0308]: mismatched types
  --> pokemon-service/src/bin/pokemon-service.rs:31:5
   |
28 | fn partial_setup<Op1, Op2, Op3, Op4, Op5, Op6>(
   |                                 --- this type parameter
29 |     builder: PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6>,
30 | ) -> PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6> {
   |      --------------------------------------------------- expected `PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6>` because of return type
31 |     builder.get_server_statistics(get_server_statistics)
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected type parameter `Op4`, found struct `Operation`
   |
   = note: expected struct `PokemonServiceBuilder<_, _, _, Op4, _, _, _>`
              found struct `PokemonServiceBuilder<_, _, _, Operation<IntoService<GetServerStatistics, fn(GetServerStatisticsInput, Extension<Arc<State>>) -> impl Future<Output = GetServerStatisticsOutput> {get_server_statistics}>>, _, _, _>
```

By registering a handler we have changed the corresponding `OpX` generic parameter.
Fixing this error requires some non-trivial type gymnastic - I gave up after trying for ~15 minutes.

### Cut them down: going from 2N+1 to 2 generic parameters

The previous two examples should have convinced you that the 2N+1 generic parameters on `PokemonServiceBuilder` harm the ergonomics of our API.
Can we get rid of them?

Yes! Let's look at one possible approach:

```rust,ignore
pub struct PokemonServiceBuilder<Body, Plugin> {
    check_health: Option<Route<Body>>,
    do_nothing: Option<Route<Body>>,
    get_pokemon_species: Option<Route<Body>>,
    get_server_statistics: Option<Route<Body>>,
    capture_pokemon: Option<Route<Body>>,
    get_storage: Option<Route<Body>>,
    plugin: Plugin,
}
```

We no longer store the raw handlers inside `PokemonServiceBuilder`.
We eagerly upgrade the operation handlers to a `Route` instance when they are registered with the builder.

```rust,ignore
impl<Body, Plugin> PokemonServiceBuilder<Body, Plugin> {
    pub fn get_pokemon_species<Handler, Extensions>(mut self, handler: Handler) -> Self
    /* Complex trait bounds */
    {
        let route = Route::new(Operation::from_handler(handler).upgrade(&self.plugin));
        self.get_pokemon_species = Some(route);
        self
    }

    /* other setters and methods */
}
```

The existing API performs the upgrade when `build` is called, forcing `PokemonServiceBuilder` to store the raw handlers and keep two generic parameters around (`OpX` and `ExtsX`) for each operation.
The proposed API requires plugins to be specified upfront, when creating an instance of the builder. They cannot be modified after a `PokemonServiceBuilder` instance has been built:

```rust,ignore
impl PokemonService<()> {
    /// Constructs a builder for [`PokemonService`].
    pub fn builder<Body, Plugin>(plugin: Plugin) -> PokemonServiceBuilder<Body, Plugin> {
        PokemonServiceBuilder {
            check_health: None,
            do_nothing: None,
            get_pokemon_species: None,
            get_server_statistics: None,
            capture_pokemon: None,
            get_storage: None,
            plugin,
        }
    }
}
```

This constraint guarantees that all operation handlers are upgraded to a `Route` using the same set of plugins.

Having to specify all plugins upfront is unlikely to have a negative impact on developers currently using `smithy-rs`.
We have seen how cumbersome it is to break the startup logic into different functions using the current service builder API. Developers are most likely specifying all plugins and routes in the same function even if the current API allows them to intersperse route registrations and plugin registrations: they would simply have to re-order their registration statements to adopt the API proposed in this RFC.

### Alternatives: allow new plugins to be registered after builder creation

The new design prohibits the following invocation style:

```rust,ignore
let plugin = ColorPlugin::new();
PokemonService::builder(plugin)
    // [...]
    .get_pokemon_species(get_pokemon_species)
    // Add PrintPlugin
    .print()
    .get_storage(get_storage)
    .build()
```

We could choose to remove this limitation and allow handlers to be upgraded using a different set of plugins depending on where they were registered.
In the snippet above, for example, we would have:

- `get_pokemon_species` is upgraded using just the `ColorPlugin`;
- `get_storage` is upgraded using both the `ColorPlugin` and the `PrintPlugin`.

There are no technical obstacles preventing us from implementing this API, but I believe it could easily lead to confusion and runtime surprises due to a mismatch between what the developer might expect `PrintPlugin` to apply to (all handlers) and what it actually applies to (handlers registered after `.print()`).

We can provide developers with other mechanisms to register plugins for a single operation or a subset of operations without introducing ambiguity.
For attaching additional plugins to a single operation, we could introduce a blanket `Pluggable` implementation for all operations in `aws-smithy-http-server`:

```rust,ignore
impl<P, Op, Pl, S, L> Pluggable<Pl> for Operation<S, L> where Pl: Plugin<P, Op, S, L> {
    type Output = Operation<Pl::Service, Pl::Layer>;

    fn apply(self, new_plugin: Pl) -> Self::Output {
       new_plugin.map(self)
   }
}
```

which would allow developers to invoke `op.apply(MyPlugin)` or call extensions methods such as `op.print()` where `op` is an `Operation`.
For attaching additional plugins to a subgroup of operations, instead, we could introduce nested builders:

```rust,ignore
let initial_plugins = ColorPlugin;
let mut builder = PokemonService::builder(initial_plugins)
    .get_pokemon_species(get_pokemon_species);
let additional_plugins = PrintPlugin;
// PrintPlugin will be applied to all handlers registered on the scoped builder returned by `scope`.
let nested_builder = builder.scoped(additional_plugins)
    .get_storage(get_storage)
    .capture_pokemon(capture_pokemon)
    // Register all the routes on the scoped builder with the parent builder.
    // API names are definitely provisional and bikesheddable.
    .attach(builder);
let app = builder.build();
```

Both proposals are outside the scope of this RFC, but they are shown here for illustrative purposes.

### Alternatives: lazy and eager-on-demand type erasure

A lot of our issues stem from type mismatch errors: we are encoding the type of our handlers into the overall type of the service builder and, as a consequence, we end up modifying that type every time we set a handler or modify its state.
Type erasure is a common approach for mitigating these issues - reduce those generic parameters to a common type to avoid the mismatch errors.
This whole RFC can be seen as a type erasure proposal - done eagerly, as soon as the handler is registered, using `Option<Route<B>>` as our "common type" after erasure.

We could try to strike a different balance - i.e. avoid performing type erasure eagerly, but allow developers to erase types on demand.
Based on my analysis, this could happen in two ways:

1. We cast handlers into a `Box<dyn Upgradable<Protocol, Operation, Exts, Body, Plugin>>` to which we can later apply plugins (lazy type erasure);
2. We upgrade registered handlers to `Route<B>` and apply plugins in the process (eager type erasure on-demand).

Let's ignore these implementation issues for the time being to focus on what the ergonomics would look like assuming we can actually perform type erasure.
In practice, we are going to assume that:

- In approach 1), we can call `.boxed()` on a registered operation and get a `Box<dyn Upgradable>` back;
- In approach 2), we can call `.erase()` on the entire service builder and convert all registered operations to `Route<B>` while keeping the `MissingOperation` entries as they are. After `erase` has been called, you can no longer register plugins (or, alternatively, the plugins you register will only apply new handlers).

We are going to explore both approaches under the assumption that we want to preserve compile-time verification for missing handlers. If we are willing to abandon compile-time verification, we get better ergonomics since all `OpX` and `ExtsX` generic parameters can be erased (i.e. we no longer need to worry about `MissingOperation`).

#### On `Box<dyn Upgradable<Protocol, Operation, Exts, Body, Plugin>>`

This is the current definition of the `Upgradable` trait:

```rust,ignore
/// Provides an interface to convert a representation of an operation to a HTTP [`Service`](tower::Service) with
/// canonical associated types.
pub trait Upgradable<Protocol, Operation, Exts, Body, Plugin> {
    type Service: Service<http::Request<Body>, Response = http::Response<BoxBody>>;

    /// Performs an upgrade from a representation of an operation to a HTTP [`Service`](tower::Service).
    fn upgrade(self, plugin: &Plugin) -> Self::Service;
}
```

In order to perform type erasure, we need to determine:

- what type parameters we are going to pass as generic arguments to `Upgradable`;
- what type we are going to use for the associated type `Service`.

We have:

- there is a single known protocol for a service, therefore we can set `Protocol` to its concrete type (e.g. `AwsRestJson1`);
- each handler refers to a different operation, therefore we cannot erase the `Operation` and the `Exts` parameters;
- both `Body` and `Plugin` appear as generic parameters on the service builder itself, therefore we can set them to the same type;
- we can use `Route<B>` to normalize the `Service` associated type.

The above leaves us with two unconstrained type parameters, `Operation` and `Exts`, for each operation. Those unconstrained type parameters leak into the type signature of the service builder itself. We therefore find ourselves having, again, 2N+2 type parameters.

#### Branching

Going back to the branching example:

```rust,ignore
let check_database: bool = /* */;
let builder = if check_database {
    builder.check_health(check_health)
} else {
    builder.check_health(check_health_with_database)
};
let app = builder.build();
```

In approach 1), we could leverage the `.boxed()` method to convert the actual `OpX` type into a `Box<dyn Upgradable>`, thus ensuring that both branches return the same type:

```rust,ignore
let check_database: bool = /* */;
let builder = if check_database {
    builder.check_health_operation(Operation::from_handler(check_health).boxed())
} else {
    builder.check_health_operation(Operation::from_handler(check_health_with_database).boxed())
};
let app = builder.build();
```

The same cannot be done when conditionally registering a route, because on the `else` branch we cannot convert `MissingOperation` into a `Box<dyn Upgradable>` since `MissingOperation` doesn't implement `Upgradable` - the pillar on which we built all our compile-time safety story.

```rust,ignore
// This won't compile!
let builder = if check_database {
    builder.check_health_operation(Operation::from_handler(check_health).boxed())
} else {
    builder
};
```

In approach 2), we can erase the whole builder in both branches when they both register a route:

```rust,ignore
let check_database: bool = /* */;
let boxed_builder = if check_database {
    builder.check_health(check_health).erase()
} else {
    builder.check_health(check_health_with_database).erase()
};
let app = boxed_builder.build();
```

but, like in approach 1), we will still get a type mismatch error if one of the two branches leaves the route unset.

#### Refactoring into smaller functions

Developers would still have to spell out all generic parameters when writing a function that takes in a builder as a parameter:

```rust,ignore
fn partial_setup<Op1, Op2, Op3, Op4, Op5, Op6, Body, Plugin>(
    builder: PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6, Body, Plugin>,
) -> PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6, Body, Plugin> {
    builder
}
```

Writing the signature after having modified the builder becomes easier though.
In approach 1), they can explicitly change the touched operation parameters to the boxed variant:

```rust,ignore
fn partial_setup<Op1, Op2, Op3, Op4, Op5, Op6, Exts4, Body, Plugin>(
    builder: PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6, Body, Plugin, Exts4=Exts4>,
) -> PokemonServiceBuilder<
        Op1, Op2, Op3, Box<dyn Upgradable<AwsRestJson1, GetServerStatistics, Exts4, Body, Plugin>>,
        Op5, Op6, Body, Plugin, Body, Plugin, Exts4=Exts
    > {
    builder.get_server_statistics(get_server_statistics)
}
```

It becomes trickier in approach 2), since to retain compile-time safety on the builder we expect `erase` to map `MissingOperation` into `MissingOperation`. Therefore, we can't write something like this:

```rust,ignore
fn partial_setup<Body, Op1, Op2, Op3, Op4, Op5, Op6>(
    builder: PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6>,
) -> PokemonServiceBuilder<Route<B>, Route<B>, Route<B>, Route<B>, Route<B>, Route<B>> {
    builder.get_server_statistics(get_server_statistics).()
}
```

The compiler would reject it since it can't guarantee that all other operations can be erased to a `Route<B>`. This is likely to require something along the lines of:

```rust,ignore
fn partial_setup<Body, Op1, Op2, Op3, Op4, Op5, Op6>(
    builder: PokemonServiceBuilder<Op1, Op2, Op3, Op4, Op5, Op6>,
) -> PokemonServiceBuilder<<Op1 as TypeErase>::Erased, <Op2 as TypeErase>::Erased, <Op3 as TypeErase>::Erased, <Op4 as TypeErase>::Erased, <Op5 as TypeErase>::Erased, <Op6 as TypeErase>::Erased>
where
    // Omitting a bunch of likely needed additional generic parameters and bounds here
    Op1: TypeErase,
    Op2: TypeErase,
    Op3: TypeErase,
    Op4: TypeErase,
    Op5: TypeErase,
    Op6: TypeErase,
{
    builder.get_server_statistics(get_server_statistics).()
}
```

#### Summary

Both approaches force us to have a number of generic parameters that scales linearly with the number of operations on the service, affecting the ergonomics of the resulting API in both the branching and the refactoring scenarios.
We believe that the ergonomics advantages of the proposal advanced by this RFC outweigh the limitation of having to specify your plugins upfront, when creating the builder instance.

### Builder extensions: what now?

The `Pluggable` trait was an interesting development out of [RFC 20]: it allows you to attach methods to a service builder using an extension trait.

```rust,ignore
/// An extension to service builders to add the `print()` function.
pub trait PrintExt: aws_smithy_http_server::plugin::Pluggable<PrintPlugin> {
    /// Causes all operations to print the operation name when called.
    ///
    /// This works by applying the [`PrintPlugin`].
    fn print(self) -> Self::Output
        where
            Self: Sized,
    {
        self.apply(PrintPlugin)
    }
}
```

This pattern needs to be revisited if we want to move forward with this RFC, since new plugins cannot be registered after the builder has been instantiated.
My recommendation would be to implement `Pluggable` for `PluginStack`, providing the same pattern ahead of the creation of the builder:

```rust,ignore
// Currently you'd have to go for `PluginStack::new(IdentityPlugin, IdentityPlugin)`,
// but that can be smoothed out even if this RFC isn't approved.
let plugin_stack = PluginStack::default()
    // Use the extension method
    .print();
let app = PokemonService::builder(plugin_stack)
    .get_pokemon_species(get_pokemon_species)
    .get_storage(get_storage)
    .get_server_statistics(get_server_statistics)
    .capture_pokemon(capture_pokemon)
    .do_nothing(do_nothing)
    .build()?;
```

## Playing around with the design

The API proposed in this RFC has been manually implemented for the Pokemon service. You can find the code [here](https://github.com/LukeMathWalker/builder-experiments).

## Changes checklist

- [x] Update `codegen-server` to generate the proposed service builder API
  - <https://github.com/awslabs/smithy-rs/pull/1954>
- [x] Implement `Pluggable` for `PluginStack`
  - <https://github.com/awslabs/smithy-rs/pull/1954>
- [x] Evaluate the introduction of a `PluginBuilder` as the primary API to compose multiple plugins (instead of `PluginStack::new(IdentityPlugin, IdentityPlugin).apply(...)`)
  - <https://github.com/awslabs/smithy-rs/pull/1971>

[RFC 20]: rfc0020_service_builder.md
[Pokemon service]: https://github.com/awslabs/smithy-rs/blob/c7ddb164b28b920313432789cfe05d8112a035cc/codegen-core/common-test-models/pokemon.smithy
[typestate builder pattern]: https://www.greyblake.com/blog/builder-with-typestate-in-rust/
[^further-dev-productivity-improvements]: The impact of a runtime error on developer productivity can be further minimised by encouraging adoption of integration testing; this can be achieved, among other options, by authoring guides that highlight its benefits and provide implementation guidance.
