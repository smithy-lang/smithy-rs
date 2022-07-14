# RFC: Logging in the Presence of Sensitive Data

> Status: RFC

Smithy provides a [sensitive trait](https://awslabs.github.io/smithy/1.0/spec/core/documentation-traits.html#sensitive-trait) which exists as a `@sensitive` field annotation syntactically and has the following semantics:

> Sensitive data MUST NOT be exposed in things like exception messages or log output. Application of this trait SHOULD NOT affect wire logging (i.e., logging of all data transmitted to and from servers or clients).

This RFC is concerned with solving the problem of honouring this specification.

Progress has been made towards this goal in the form of the [Sensitive Trait PR](https://github.com/awslabs/smithy-rs/pull/229), which uses code generation to remove sensitive fields from `Debug` implementations.

The problem remains open due to the existence of HTTP binding traits and a lack of clearly defined user guidelines which customers may follow to honour the specification.

This RFC proposes a new logging `Layer` to be generated and applied to each `OperationHandler` `Service`, and internal and external developer guidelines on how to avoid violating the specification.

## Terminology

- **Model**: A [Smithy Model](https://awslabs.github.io/smithy/1.0/spec/core/model.html), usually pertaining to the one in use by the customer.
- **Runtime crate**: A crate existing within the `rust-runtime/` folder, used to implement shared functionalities that do not have to be code-generated.
- **Service**: The [tower::Service](https://docs.rs/tower-service/latest/tower_service/trait.Service.html) trait. The lowest level of abstraction we deal with when making HTTP requests. Services act directly on data to transform and modify that data. A Service is what eventually turns a request into a response.
- **Layer**: Layers are a higher-order abstraction over services that is used to compose multiple services together, creating a new service from that combination. Nothing prevents us from manually wrapping services within services, but Layers allow us to do it in a flexible and generic manner. Layers don't directly act on data but instead can wrap an existing service with additional functionality, creating a new service. Layers can be thought of as middleware. *NOTE: The use of [Layers can produce compiler errors] that are difficult to interpret and defining a layer requires a large amount of boilerplate code.*
- **Potentially sensitive**: Data that _could_ be bound to a sensitive field of a structure, for example [HTTP Binding Traits](#http-binding-traits).

## Background

### HTTP Binding Traits

Smithy provides various [HTTP binding traits](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html). These allow protocols to configure a HTTP request by way of binding fields to parts of the request. For this reason sensitive data might be unintentionally leaked through logging of a bound request.

| Trait                                                                                                        | Configurable     |
| ------------------------------------------------------------------------------------------------------------ | ---------------- |
| [httpHeader](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpheader-trait)               | Headers          |
| [httpPrefixHeaders](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpprefixheaders-trait) | Headers          |
| [httpLabel](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httplabel-trait)                 | URI              |
| [httpQuery](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpquery-trait)                 | Query Parameters |
| [httpResponseCode](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpresponsecode-trait)   | Status Code      |

Each of these configurable parts must therefore be logged cautiously.

### Scope and Guidelines

It is unfeasible to make the logging of sensitive data forbidden a type theoretic invariant. With the current API, the customer will always have an opportunity to log a request containing sensitive data before it enters the `Service<Request<B>>` that we provide to them.

```rust
// The API provides us with a `Service<Request<B>>`
let app: Router = OperationRegistryBuilder::default().build().expect("unable to build operation registry").into();

// We can `ServiceExt::map_request` log a request with potentially sensitive data
let app = app.map_request(|request| {
        info!(?request);
        request
    });
```

A more subtle violation of the specification may occur when the customer enables verbose logging - a third-party dependency might simply log data marked as sensitive, for example `tokio` or `hyper`.

These two cases illustrate that `smithy-rs` can only prevent violation of the specification in a restricted scope - logs emitted from generated code and the runtime crates. A `smithy-rs` specific guideline should be available to the customer which outlines how to avoid violating the specification in areas outside of our control.

### Routing

The sensitivity and HTTP bindings are declared within specific structures/operations. For this reason, in the general case, it's unknowable whether or not any given part of a request is sensitive until we determine which operation is tasked with handling the request and hence which fields are bound. Implementation wise, this means that any `Layer` applied _before_ routing has taken place cannot log anything sensitive without performing routing logic itself.

Note that:
- We are not required to deserialize the entire request before we can make judgments on what data is sensitive or not - only which operation it has been routed to.
- We are permitted to emit logs prior to routing when:
  - they contain no potentially sensitive data, or
  - the request failed to route, in which case it's subject to the constraints of an operation.

### Runtime Crates

The crates existing in `rust-runtime` are not code generated - their source code is agnostic to the specific model in use. For this reason, if such a crate wanted to log potentially sensitive data then there must be a way to conditionally toggle that log without manipulation of the source code. Any proposed solution must acknowledge this concern.

## Proposal

This proposal serves to honor the sensitivity specification via code generation of a logging `Layer`s which is aware of the sensitivity, together with a developer contract disallowing logging potentially sensitive data in the runtime crates. An internal and external guideline should be provided in addition to the `Layer`s.

All data known to be sensitive should be replaced with `"{redacted}"` when logged. Implementation wise this means that [tracing::Event](https://docs.rs/tracing/latest/tracing/struct.Event.html)s and [tracing::Span](https://docs.rs/tracing/latest/tracing/struct.Span.html)s of the form `debug!(field = "sensitive data")` and `span!(..., field = "sensitive data")` must become `debug!(field = "{redacted}")` and `span!(..., field = "{redacted}")`.

### Debug Logging

Developers might want to observe sensitive data for debugging purposes. It should be possible to opt-out of the redactions by enabling a feature flag `unredacted-logging` (which is disabled by default).

To prevent excessive branches such as

```rust
if cfg!(feature = "unredacted-logging") {
    debug!(%data, "logging here");
} else {
    debug!(data = "{redacted}", "logging here");
}
```

the following wrapper should be provided

```rust
pub struct Sensitive<T>(T);

impl<T> Debug for Sensitive<T>
where
    T: Debug
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        if cfg!(feature = "unredacted-logging") {
            self.0.fmt(f)
        } else {
            "{redacted}".fmt(f)
        }
    }
}

impl<T> Display for Sensitive<T>
where
    T: Display
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        if cfg!(feature = "unredacted-logging") {
            self.0.fmt(f)
        } else {
            "{redacted}".fmt(f)
        }
    }
}
```

In which case the branch above becomes

```rust
debug!(sensitive_data = %Sensitive(data));
```

### Code Generated Logging Layer

Using the smithy model, for each operation, a logging `Layer` should be generated. Through the model, the code generation knows which fields are sensitive and which HTTP bindings exist, therefore the logging `Layer` can be careful crafted to avoid leaking sensitive data.

This `Layer` should be applied to the [OperationHandler](https://github.com/awslabs/smithy-rs/blob/cd0563020abcde866a741fa123e3f2e18e1be1c9/rust-runtime/inlineable/src/server_operation_handler_trait.rs#L17-L21) directly after its construction in the generated `operation_registry.rs`. The `Layer` should preserve the associated types of the `OperationHandler` (`Response = Response<BoxBody>`, `Error = Infallible`) so cause minimal breakage.

This is illustrated before as the introduction of `Logging{Operation}::new`.

```rust
let empty_operation = LoggingEmptyOperation::new(operation(registry.empty_operation));
let get_pokemon_species = LoggingPokemonSpecies::new(operation(registry.get_pokemon_species));
let get_server_statistics = LoggingServerStatistics::new(operation(registry.get_server_statistics));
let routes = vec![
    (BoxCloneService::new(empty_operation), empty_operation_request_spec),
    (BoxCloneService::new(get_pokemon_species), get_pokemon_species_request_spec),
    (BoxCloneService::new(get_server_statistics), get_server_statistics_request_spec),
];
let router = aws_smithy_http_server::routing::Router::new_rest_json_router(routes);
```

As a request enters this layer it should record the HTTP headers, status code, and URI.

The following model

```smithy
@readonly
@http(uri: "/inventory/{name}", method: "GET")
operation Inventory {
    input: Product,
    output: Stocked
}

@input
structure Product {
    @required
    @sensitive
    @httpLabel
    name: String
}

@output
structure Stocked {
    @sensitive
    @httpResponseCode
    code: String,
}
```

should generate the following

```rust
// NOTE: This code is intended to show behavior - it does not compile

const SENSITIVE_MARKER: &str = "{redacted}";

pub struct InventoryLogging<S> {
    inner: S
}

impl<S> InventoryLogging<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner
        }
    }
}

impl<B, S> Service<Request<B>> for InventoryLogging<S>
where
    S: Service<Request<B>>
{
    type Response = Response<BoxBody>;
    type Error = S::Error;
    type Future = /* Implementation detail */;

    fn call(&mut self, request: Request<B>) -> Self::Future {
        // Remove sensitive data from path
        let mut uri = redact(request.uri());

        let fut = async {
            let response = self.inner.call(request).await;

            debug!(status_code = %Sensitive(request.status()), "sending response");

            response
        };

        // Instrument the future with a span
        let span = span!(%method, %uri, headers = request.headers(), "received request");
        fut.instrument(span)
    }
}
```

As this path is latency-sensitive, careful implementation is required to avoid excess allocations during redaction of sensitive data. Wrapping `Uri` and `HeaderMap` then providing a new `Display` implementation which skips over the sensitive data is preferable over allocating a new `String`/`HeaderMap` and mutating it.

### Internal Guideline

A guideline should be made available to internal smithy developers to outline the following:

- The [HTTP bindings traits](#http-binding-traits) and why they are of concern.
- The [Debug implementation](https://github.com/awslabs/smithy-rs/pull/229) on structures.
- How to use the `Sensitive` struct and the `unredacted-logging` feature flag described in [Debug Logging](#unredacted-logging).

### Public Guideline

A guideline should be made available to customers to outline the following:

- The [HTTP bindings traits](#http-binding-traits) and why they are of concern.
- Warn against the two potential leaks described in [Scope and Guidelines](#scope-and-guidelines):
  - Sensitive data leaking from third-party dependencies.
  - Sensitive data leaking from middleware applied to the `Router`.
- How to use the `Sensitive` struct and the `unredacted-logging` feature flag described in [Debug Logging](#unredacted-logging).

## Alternative Proposals

All of the following proposals are compatible with, and benefit from, [Debug Logging](#unredacted-logging), [Internal Guideline](#internal-guideline)/[External Guideline](#external-guideline) portions of the main proposal.

The main proposal disallows the logging of potentially sensitive data in the runtime crates, instead opting for a dedicated code generated logging layer. In contrast, the following proposals all seek ways to accommodate logging of potentially sensitive data in the runtime crates.

### Use Request Extensions

Request extensions can be used to adjoin data to a Request as it passes through the middleware layers. Concretely, they exist as the type map [http::Extensions](https://docs.rs/http/latest/http/struct.Extensions.html) accessed via [http::extensions](https://docs.rs/http/latest/http/request/struct.Request.html#method.extensions) and [http::extensions_mut](https://docs.rs/http/latest/http/request/struct.Request.html#method.extensions_mut).

These can be used to provide data to middleware interested in logging potentially sensitive data.

```rust
struct Sensitivity {
    /* Data concerning which parts of the request are sensitive */
}

struct Middleware<S> {
    inner: S
}

impl<B, S> Service<Request<B>> for Middleware<S> {
    /* ... */

    fn call(&mut self, request: Request<B>) -> Self::Future {
        if let Some(sensitivity) = request.extensions().get::<Sensitivity>() {
            if sensitivity.is_method_sensitive() {
                debug!(method = %request.method());
            }
        }

        /* ... */

        self.inner.call(request)
    }
}
```

A middleware layer must be code generated (much in the same way as the logging layer) which is dedicated to inserting the `Sensitivity` struct into the extensions of each incoming request.

```rust
impl<B, S> Service<Request<B>> for SensitivityInserter<S>
where
    S: Service<Request<B>>
{
    /* ... */

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let sensitivity = Sensitivity {
            /* .. */
        };
        request.extensions_mut().insert(sensitivity);

        self.inner.call(request)
    }
}
```

#### Advantages

- Applicable to _all_ middleware which takes `http::Request<B>`.
- Does not pollute the API of the middleware - code internal to middleware simply inspects the request's extensions and performs logic based on its value.

#### Disadvantages

- The sensitivity and HTTP bindings are known at compile time whereas the insertion/retrieval of the extension data is done at runtime.
  - [http::Extensions](https://docs.rs/http/latest/http/struct.Extensions.html) is approximately a `HashMap<u64, Box<dyn Any>>` so lookup/insertion involves indirection/cache misses/heap allocation.

### Accommodate the Sensitivity in Middleware API

It is possible that sensitivity is a parameter passed to middleware during construction. This is similar in nature to [Use Request Extensions](#use-request-extensions) except that the `Sensitivity` is passed to middleware at startup.

```rust
struct Middleware<S> {
    inner: S,
    sensitivity: Sensitivity
}

impl Middleware<S> {
    pub fn new(inner: S) -> Self { /* ... */ }

    pub fn new_with_sensitivity(inner: S, sensitivity: Sensitivity) -> Self { /* ... */ }
}

impl<B, S> Service<Request<B>> for Middleware<S> {
    /* ... */

    fn call(&mut self, request: Request<B>) -> Self::Future {
        if self.sensitivity.is_method_sensitive() {
            debug!(method = %Sensitive(request.method()));
        }

        /* ... */

        self.inner.call(request)
    }
}
```

It would then be required that the code generation responsible constructing a `Sensitivity` for each operation. Additionally, if any middleware is being applied to a operation then the code generation would be responsible for passing that middleware the appropriate `Sensitivity` before applying it.

#### Advantages

- Applicable to _all_ middleware.
- As the `Sensitivity` struct will be known statically, the compiler will remove branches, making it potentially free.

#### Disadvantages

- Pollutes the API of middleware.

### Redact values using a tracing Layer

Distinct from `tower::Layer`, a [tracing::Layer](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/trait.Layer.html) is a "composable handler for `tracing` events". It would be possible to write an implementation which would filter out events which contain sensitive data.

Examples of filtering `tracing::Layer`s already exist in the form of the [EnvFilter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) and [Targets](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/targets/struct.Targets.html). It is unlikely that we'll be able to leverage them for our use, but the underlying principle remains the same - the `tracing::Layer` inspects `tracing::Event`s/`tracing::Span`s filtering them based on some criteria.

Code generation would be need to be used in order to produce the filtering criteria from the models. Internal developers would need to adhere to a common set of field names in order for them to be subject to the filtering. Spans would need to be opened after routing occurs in order for the `tracing::Layer` to know which operation `Event`s are being produced within and hence which filtering rules to apply.

#### Advantages

- Applicable to _all_ middleware.
- Good separation of concerns:
  - Does not pollute the API of the middleware
  - No specific logic required within middleware.

#### Disadvantages

- Complex implementation.
- Not necessarily fast.
- `tracing::Layer`s seem to only support filtering entire `Event`s, rather than more fine grained removal of fields.
