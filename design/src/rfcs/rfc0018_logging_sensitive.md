# RFC: Logging in the Presence of Sensitive Data

> Status: Accepted

Smithy provides a [sensitive trait](https://awslabs.github.io/smithy/1.0/spec/core/documentation-traits.html#sensitive-trait) which exists as a `@sensitive` field annotation syntactically and has the following semantics:

> Sensitive data MUST NOT be exposed in things like exception messages or log output. Application of this trait SHOULD NOT affect wire logging (i.e., logging of all data transmitted to and from servers or clients).

This RFC is concerned with solving the problem of honouring this specification in the context of logging.

Progress has been made towards this goal in the form of the [Sensitive Trait PR](https://github.com/awslabs/smithy-rs/pull/229), which uses code generation to remove sensitive fields from `Debug` implementations.

The problem remains open due to the existence of HTTP binding traits and a lack of clearly defined user guidelines which customers may follow to honour the specification.

This RFC proposes:

- A new logging middleware is generated and applied to each `OperationHandler` `Service`.
- A developer guideline is provided on how to avoid violating the specification.

## Terminology

- **Model**: A [Smithy Model](https://awslabs.github.io/smithy/1.0/spec/core/model.html), usually pertaining to the one in use by the customer.
- **Runtime crate**: A crate existing within the `rust-runtime` folder, used to implement shared functionalities that do not have to be code-generated.
- **Service**: The [tower::Service](https://docs.rs/tower-service/latest/tower_service/trait.Service.html) trait. The lowest level of abstraction we deal with when making HTTP requests. Services act directly on data to transform and modify that data. A Service is what eventually turns a request into a response.
- **Middleware**: Broadly speaking, middleware modify requests and responses. Concretely, these are exist as implementations of [Layer](https://docs.rs/tower/latest/tower/layer/trait.Layer.html)/a `Service` wrapping an inner `Service`.
- **Potentially sensitive**: Data that _could_ be bound to a sensitive field of a structure, for example via the [HTTP Binding Traits](#http-binding-traits).

## Background

### HTTP Binding Traits

Smithy provides various HTTP binding traits. These allow protocols to configure a HTTP request by way of binding fields to parts of the request. For this reason sensitive data might be unintentionally leaked through logging of a bound request.

| Trait                                                                                                        | Configurable     |
|--------------------------------------------------------------------------------------------------------------|------------------|
| [httpHeader](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpheader-trait)               | Headers          |
| [httpPrefixHeaders](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpprefixheaders-trait) | Headers          |
| [httpLabel](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httplabel-trait)                 | URI              |
| [httpPayload](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httppayload-trait)             | Payload          |
| [httpQuery](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpquery-trait)                 | Query Parameters |
| [httpResponseCode](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpresponsecode-trait)   | Status Code      |

Each of these configurable parts must therefore be logged cautiously.

### Scope and Guidelines

It would be unfeasible to forbid the logging of sensitive data all together using the type system. With the current API, the customer will always have an opportunity to log a request containing sensitive data before it enters the `Service<Request<B>>` that we provide to them.

```rust,ignore
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

The sensitivity and HTTP bindings are declared within specific structures/operations. For this reason, in the general case, it's unknowable whether or not any given part of a request is sensitive until we determine which operation is tasked with handling the request and hence which fields are bound. Implementation wise, this means that any middleware applied _before_ routing has taken place cannot log anything potentially sensitive without performing routing logic itself.

Note that:

- We are not required to deserialize the entire request before we can make judgments on what data is sensitive or not - only which operation it has been routed to.
- We are permitted to emit logs prior to routing when:
  - they contain no potentially sensitive data, or
  - the request failed to route, in which case it's not subject to the constraints of an operation.

### Runtime Crates

The crates existing in `rust-runtime` are not code generated - their source code is agnostic to the specific model in use. For this reason, if such a crate wanted to log potentially sensitive data then there must be a way to conditionally toggle that log without manipulation of the source code. Any proposed solution must acknowledge this concern.

## Proposal

This proposal serves to honor the sensitivity specification via code generation of a logging middleware which is aware of the sensitivity, together with a developer contract disallowing logging potentially sensitive data in the runtime crates. A developer guideline should be provided in addition to the middleware.

All data known to be sensitive should be replaced with `"{redacted}"` when logged. Implementation wise this means that [tracing::Event](https://docs.rs/tracing/latest/tracing/struct.Event.html)s and [tracing::Span](https://docs.rs/tracing/latest/tracing/struct.Span.html)s of the form `debug!(field = "sensitive data")` and `span!(..., field = "sensitive data")` must become `debug!(field = "{redacted}")` and `span!(..., field = "{redacted}")`.

### Debug Logging

Developers might want to observe sensitive data for debugging purposes. It should be possible to opt-out of the redactions by enabling a feature flag `unredacted-logging` (which is disabled by default).

To prevent excessive branches such as

```rust,ignore
if cfg!(feature = "unredacted-logging") {
    debug!(%data, "logging here");
} else {
    debug!(data = "{redacted}", "logging here");
}
```

the following wrapper should be provided from a runtime crate:

```rust,ignore
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

```rust,ignore
debug!(sensitive_data = %Sensitive(data));
```

### Code Generated Logging Middleware

Using the smithy model, for each operation, a logging middleware should be generated. Through the model, the code generation knows which fields are sensitive and which HTTP bindings exist, therefore the logging middleware can be carefully crafted to avoid leaking sensitive data.

As a request enters this middleware it should record the method, HTTP headers, status code, and URI in a `tracing::span`. As a response leaves this middleware it should record the HTTP headers and status code in a `tracing::debug`.

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

```rust,ignore
// NOTE: This code is intended to show behavior - it does not compile

pub struct InventoryLogging<S> {
    inner: S,
    operation_name: &'static str
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
        // Remove sensitive data from parts of the HTTP
        let uri = /* redact {name} from URI */;
        let headers = /* no redactions */;

        let fut = async {
            let response = self.inner.call(request).await;
            let status_code = /* redact status code */;
            let headers = /* no redactions */;

            debug!(%status_code, ?headers, "response");

            response
        };

        // Instrument the future with a span
        let span = debug_span!("request", operation = %self.operation_name, method = %request.method(), %uri, ?headers);
        fut.instrument(span)
    }
}
```

### HTTP Debug/Display Wrappers

The `Service::call` path, seen in [Code Generated Logging Middleware](#code-generated-logging-middleware), is latency-sensitive. Careful implementation is required to avoid excess allocations during redaction of sensitive data. Wrapping [Uri](https://docs.rs/http/latest/http/uri/struct.Uri.html) and [HeaderMap](https://docs.rs/http/latest/http/header/struct.HeaderMap.html) then providing a new [Display](https://doc.rust-lang.org/std/fmt/trait.Display.html)/[Debug](https://doc.rust-lang.org/std/fmt/trait.Debug.html) implementation which skips over the sensitive data is preferable over allocating a new `String`/`HeaderMap` and then mutating it.

These wrappers should be provided alongside the `Sensitive` struct described in [Debug Logging](#debug-logging). If they are implemented on top of `Sensitive`, they will inherit the same behavior - allowing redactions to be toggled using `unredacted-logging` feature flag.

### Middleware Position

This logging middleware should be applied outside of the [OperationHandler](https://github.com/awslabs/smithy-rs/blob/cd0563020abcde866a741fa123e3f2e18e1be1c9/rust-runtime/inlineable/src/server_operation_handler_trait.rs#L17-L21) after its construction in the (generated) `operation_registry.rs` file. The middleware should preserve the associated types of the `OperationHandler` (`Response = Response<BoxBody>`, `Error = Infallible`) to cause minimal disruption.

An easy position to apply the logging middleware is illustrated below in the form of `Logging{Operation}::new`:

```rust,ignore
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

Although an acceptable first step, putting logging middleware here is suboptimal - the `Router` allows a `tower::Layer` to be applied to the operation by using the [Router::layer](https://github.com/awslabs/smithy-rs/blob/main/rust-runtime/aws-smithy-http-server/src/routing/mod.rs#L146) method. This middleware will be applied _outside_ of the logging middleware and, as a result, will not be subject to the span of any middleware. Therefore, the `Router` must be changed to allow for middleware to be applied within the logging middleware rather than outside of it.

This is a general problem, not specific to this proposal. For example, [Use Request Extensions](#use-request-extensions) must also solve this problem.

Fortunately, this problem is separable from the actual implementation of the logging middleware and we can get immediate benefit by application of it in the suboptimal position described above.

### Logging within the Router

There is need for logging within the `Router` implementation - this is a crucial area of business logic. As mentioned in the [Routing](#routing) section, we are permitted to log potentially sensitive data in cases where requests fail to get routed to an operation.

In the case of AWS JSON 1.0 and 1.1 protocols, the request URI is always `/`, putting it outside of the reach of the `@sensitive` trait. We therefore have the option to log it before routing occurs. We make a choice not to do this in order to remove the special case - relying on the logging layer to log URIs when appropriate.

### Developer Guideline

A guideline should be made available, which includes:

- The [HTTP bindings traits](#http-binding-traits) and why they are of concern in the presence of `@sensitive`.
- The [Debug implementation](https://github.com/awslabs/smithy-rs/pull/229) on structures.
- How to use the `Sensitive` struct, HTTP wrappers, and the `unredacted-logging` feature flag described in [Debug Logging](#debug-logging) and [HTTP Debug/Display Wrappers](#http-debugdisplay-wrappers).
- A warning against the two potential leaks described in [Scope and Guidelines](#scope-and-guidelines):
  - Sensitive data leaking from third-party dependencies.
  - Sensitive data leaking from middleware applied to the `Router`.

## Alternative Proposals

All of the following proposals are compatible with, and benefit from, [Debug Logging](#debug-logging), [HTTP Debug/Display Wrappers](#http-debugdisplay-wrappers), and [Developer Guideline](#developer-guideline) portions of the main proposal.

The main proposal disallows the logging of potentially sensitive data in the runtime crates, instead opting for a dedicated code generated logging middleware. In contrast, the following proposals all seek ways to accommodate logging of potentially sensitive data in the runtime crates.

### Use Request Extensions

Request extensions can be used to adjoin data to a Request as it passes through the middleware. Concretely, they exist as the type map [http::Extensions](https://docs.rs/http/latest/http/struct.Extensions.html) accessed via [http::extensions](https://docs.rs/http/latest/http/request/struct.Request.html#method.extensions) and [http::extensions_mut](https://docs.rs/http/latest/http/request/struct.Request.html#method.extensions_mut).

These can be used to provide data to middleware interested in logging potentially sensitive data.

```rust,ignore
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

A middleware layer must be code generated (much in the same way as the logging middleware) which is dedicated to inserting the `Sensitivity` struct into the extensions of each incoming request.

```rust,ignore
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

It is possible that sensitivity is a parameter passed to middleware during construction. This is similar in nature to [Use Request Extensions](#use-request-extensions) except that the `Sensitivity` is passed to middleware during construction.

```rust,ignore
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
- As the `Sensitivity` struct will be known statically, the compiler will remove branches, making it cheap.

#### Disadvantages

- Pollutes the API of middleware.

### Redact values using a tracing Layer

Distinct from `tower::Layer`, a [tracing::Layer](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/trait.Layer.html) is a "composable handler for `tracing` events". It would be possible to write an implementation which would filter out events which contain sensitive data.

Examples of filtering `tracing::Layer`s already exist in the form of the [EnvFilter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) and [Targets](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/targets/struct.Targets.html). It is unlikely that we'll be able to leverage them for our use, but the underlying principle remains the same - the `tracing::Layer` inspects `tracing::Event`s/`tracing::Span`s, filtering them based on some criteria.

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

## Changes Checklist

- [x] Implement and integrate code generated logging middleware.
  - <https://github.com/awslabs/smithy-rs/pull/1550>
- [x] Add logging to `Router` implementation.
  - <https://github.com/awslabs/smithy-rs/issues/1666>
- [x] Write developer guideline.
  - <https://github.com/awslabs/smithy-rs/pull/1772>
- [x] Refactor `Router` to allow for better positioning described in [Middleware Position](#middleware-position).
  - <https://github.com/awslabs/smithy-rs/pull/1620>
  - <https://github.com/awslabs/smithy-rs/pull/1679>
  - <https://github.com/awslabs/smithy-rs/pull/1693>
