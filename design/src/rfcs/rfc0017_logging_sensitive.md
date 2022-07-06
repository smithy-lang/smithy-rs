# RFC: Logging in the Presence of Sensitive Data

> Status: RFC

Smithy provides a [sensitive trait](https://awslabs.github.io/smithy/1.0/spec/core/documentation-traits.html#sensitive-trait) which exists as a `@sensitive` field annotation syntactically and has the following semantics:

> Sensitive data MUST NOT be exposed in things like exception messages or log output. Application of this trait SHOULD NOT affect wire logging (i.e., logging of all data transmitted to and from servers or clients).

This RFC is concerned with solving the problem of honouring this specification.

Progress has been made towards this goal in the form of the [Sensitive Trait PR](https://github.com/awslabs/smithy-rs/pull/229), which uses code generation to remove sensitive fields from `Debug` implementations.

The problem remains open due to the existence of HTTP binding traits and a lack of clearly defined user guidelines which customers may follow, in the context of `smithy-rs`, to honour the specification.

This RFC proposes a new logging `Layer` to be applied to the `Routes` `Service`, and internal and external developer guidelines on how to avoid violating the specification.

## Terminology

- **Runtime crate**: A crate existing within the `/rust-runtime/` folder.
- **Service**: A trait defined in the [`tower-service` crate][tower_service::Service]. The lowest level of abstraction we deal with when making HTTP requests. Services act directly on data to transform and modify that data. A Service is what eventually turns a request into a response.
- **Layer**: Layers are a higher-order abstraction over services that is used to compose multiple services together, creating a new service from that combination. Nothing prevents us from manually wrapping services within services, but Layers allow us to do it in a flexible and generic manner. Layers don't directly act on data but instead can wrap an existing service with additional functionality, creating a new service. Layers can be thought of as middleware. *NOTE: The use of [Layers can produce compiler errors] that are difficult to interpret and defining a layer requires a large amount of boilerplate code.*

## Background

### HTTP Binding Traits

Smithy provides various [HTTP binding traits](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html). These allow protocols to configure a HTTP request by way of binding fields to parts of the request. For this reason sensitive data might be unintentionally leaked through logging of a bound request.

| Trait                                                                                                        | Configurable        |
|--------------------------------------------------------------------------------------------------------------|---------------------|
| [http](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#http-trait)                           | URI and Status Code |
| [httpHeader](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpheader-trait)               | Headers             |
| [httpPrefixHeaders](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpprefixheaders-trait) | Headers             |
| [httpLabel](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httplabel-trait)                 | URI                 |
| [httpQuery](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpquery-trait)                 | Query Parameters    |
| [httpResponseCode](https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpresponsecode-trait)   | Status Code

Each of these configurable parts must therefore be logged cautiously.

### Scope and Guidelines

Making the logging of sensitive data forbidden an type theoretic invariant is unfeasible. With the current API, the customer will always have an opportunity to log a request containing sensitive data before it enters the `Service<Request<B>>` that we provide to them.

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

### Runtime Crates

The crates existing in `rust-runtime` are non-generated - their source code is agnostic to the specific smithy models in use. For this reason, if such a crate wanted to log data which could be sensitive then there must be a way to conditionally toggle that log without manipulation of the source code. Any proposed solution must acknowledge this concern.

## Proposal

<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [x] Create new struct `NewFeature`
