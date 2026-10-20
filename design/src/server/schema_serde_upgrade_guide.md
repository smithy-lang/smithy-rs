# Upgrading a Server to Schema-Driven Serde

This guide lists the source changes a service needs when it turns on `codegen.schemaSerde: true`.
Each entry describes what breaks or changes, how you notice it, and what to change.

Services that do not enable `schemaSerde` are unaffected.

This guide calls `schemaSerde: false` **generated serde** and `schemaSerde: true`
**schema serde**. Generated serde emits protocol serializers and deserializers;
schema serde uses runtime schema walkers. Serde mode is separate from HTTP version
selection: `aws-smithy-legacy-http-server` is the HTTP 0.x runtime, while
`aws-smithy-http-server` supports both serde modes over HTTP 1.x.

## RPC v2 CBOR streaming blob inputs

Schema serde preserves `@streaming` on blob shapes, so RPC v2 CBOR handlers receive
`ByteStream` fields where generated serde CBOR codegen generated `Blob` fields. To access all
bytes in a handler, use `input.data.collect().await?.into_bytes()` instead of
`input.data.as_ref()`.

For CBOR requests, the server collects the complete document under the configured
body limit and timeout, decodes the blob and other fields with the existing codec,
and validates the input before invoking the handler. The handler's `ByteStream`
contains the decoded blob bytes, including when fields follow the blob in the
document. It does not contain CBOR framing. Nullable blob members that are absent
are represented by an empty `ByteStream` in the shared non-optional Rust field.

REST streaming blob uploads still pass the live body to the same handler after
decoding and validating HTTP-bound metadata. Incremental CBOR input decoding is
deferred until the handler contract for trailing fields and validation is settled.

## The service's protocol type

**What changes.** `ServiceShape::Protocol` names the protocols the service serves. For a service
with one protocol it is that protocol, as before (`RestJson1`, `RpcV2Cbor`, ...). For a service with
several protocols it is a tuple of them, for example `(RestJson1, RpcV2Cbor)`.

**How you notice.** Code that is generic over the service's protocol keeps compiling. Code written
for one protocol (a plugin, an extractor, an `IntoResponse` impl) stops compiling when it is used
with a multi-protocol service, instead of silently not applying.

## FromParts for multiprotocol

Request-part extractors resolve against the service's protocol type. An extractor implemented
only for `RestJson1` works on a restJson1 service, but does not satisfy the handler's bounds when
the service serves several protocols, such as `(RestJson1, RpcV2Cbor)`. The builder call that
registers a handler taking the extractor then fails to compile with an unsatisfied `FromParts`
bound.

For example, a single-protocol caller-info extractor may use the restJson1 runtime error:

```rust
use http::request::Parts;
use sdk::server::{
    protocol::rest_json_1::{runtime_error::RuntimeError, RestJson1},
    request::FromParts,
};

impl FromParts<RestJson1> for CallerInfo {
    type Rejection = RuntimeError;

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        extract_caller_info_from_parts(parts).ok_or_else(|| {
            tracing::warn!("Rejecting request: caller identity could not be resolved");
            RuntimeError::Validation("Caller identity could not be resolved".to_string())
        })
    }
}
```

Change both the implementation to `impl<P> FromParts<P>` and the error import to the shared
`sdk::server::runtime_error::RuntimeError` so the extractor works with any protocol:

```rust
use http::request::Parts;
use sdk::server::{request::FromParts, runtime_error::RuntimeError};

impl<P> FromParts<P> for CallerInfo {
    type Rejection = sdk::server::runtime_error::RuntimeError;

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        extract_caller_info_from_parts(parts).ok_or_else(|| {
            tracing::warn!("Rejecting request: caller identity could not be resolved");
            RuntimeError::Validation("Caller identity could not be resolved".to_string())
        })
    }
}
```

Here, `sdk` is your generated service SDK crate. The examples use your existing `CallerInfo`
and `extract_caller_info_from_parts` helper; identity extraction and logging stay the same.
The shared runtime error implements `IntoResponse<P>` for every `P`, including protocol tuples.
Making the extractor generic while keeping the protocol-specific error is insufficient, because
that error's response conversion only supports its original protocol.

Schema upgrades call `IntoResponse<P>::into_response_with_protocol`, passing the protocol selected
for the request. The shared error uses that protocol to frame the rejection. If you use your own
rejection type instead, implement `IntoResponse<P>` for every `P`; its default context-aware
conversion preserves the response returned by its existing `into_response()` implementation.

## `IntoResponse<RestJson1>` implementations on your own types

**What changes.** `impl IntoResponse<RestJson1> for MyError` (or for `AwsJson1_0`, `RestXml`, and so
on) still compiles for types that do not also implement `HttpModeledError`, but the schema path never
calls it for handler errors. Types implementing both traits must remove their explicit response impl
or use a separate wrapper; see [Runtime support for one middleware API](#runtime-support-for-one-middleware-api).
Handler errors are serialized
through the generated operation error types, so a custom `IntoResponse` impl is only used where your
code calls it directly, or as the rejection of a `FromParts` extractor (see above), for example:

```rust
let response = IntoResponse::<RestJson1>::into_response(err);
```

**How you notice.** You don't, at compile time. If you relied on the framework calling your impl,
the response your clients receive changes silently.

**What to change.** Search your code for `impl IntoResponse<` on protocol marker types and check
each one:

* Direct conversions on types that still implement the trait keep working. Middleware requiring
  generated operation errors to implement `IntoResponse<P>` needs the migration below.
* If you expected the framework to call it, return a modeled error from the handler instead, or
  build the response in a layer, so the error is serialized by the protocol that served the request.

## HTTP middleware that returns errors

**What changes.** Middleware such as an authentication plugin may require
`Op::Error: IntoResponse<P>` and `InternalFailureException: IntoResponse<P>` to return early
when authentication or request-body collection fails. Before the runtime adapters described below,
these bounds failed for multi-protocol services: `P` is a protocol tuple, while the existing
conversions supported concrete protocol markers. Schema codegen generates `HttpModeledError`
for operation errors, and the updated runtime now supplies their `IntoResponse<P>` blanket
implementation. Middleware must also pass the selected protocol to obtain a framed response.

**How you notice.** With earlier schema runtimes, registering the plugin fails with an unsatisfied
`IntoResponse<(...)>` bound for an operation error or `InternalFailureException`. With the updated
runtime, the bounds compile, but calling only `into_response()` on a schema error returns an empty,
unframed response. A tuple conversion that chooses one fixed protocol would also produce the wrong
framing for requests selecting another protocol.

**What to change.** Obtain the protocol selected for each request and use the context-aware
conversion when it is available. The runtime adapters below let middleware retain its existing
`IntoResponse<P>` bounds. Schema-specific middleware can alternatively use shared
`runtime_error::RuntimeError` for framework failures and require `Op::Error: schema::HttpModeledError`
to serialize mapped authentication errors directly, as the following helpers illustrate.

### One implementation for single- and multi-protocol schema services

The distinction is schema versus generated serde serialization, not single versus multiple protocols.
With `schemaSerde: true`, routing supplies `SelectedOperation` for single-protocol services too.
The same middleware implementation can remain generic over `P`, require
`Op::Error: HttpModeledError`, and use the selected protocol for both kinds of service:

| Service configuration | Middleware error serialization |
| --- | --- |
| Schema, single protocol (`P = RestJson1`, for example) | Selected protocol's `serialize_runtime_error` and `serialize_error`. |
| Schema, multiple protocols (`P = (RestJson1, RpcV2Cbor)`, for example) | The same calls, using the protocol selected for each request. |
| Generated serde, schema serde disabled | Existing `IntoResponse<P>` conversions for its concrete protocol marker. |

No single-protocol schema fallback or tuple-specific implementation is needed. Even when `P`
is a concrete marker, the schema middleware obtains framing from the request's selected handle.

The following helpers illustrate the two conversions:

```rust
use sdk::server::{
    response::{IntoResponse, Response},
    runtime_error::RuntimeError,
    schema::{HttpModeledError, SharedServerProtocol},
};

fn runtime_failure(protocol: Option<&SharedServerProtocol>, error: RuntimeError) -> Response {
    match protocol {
        Some(protocol) => protocol.serialize_runtime_error(error),
        None => IntoResponse::<()>::into_response(error),
    }
}

fn authentication_failure<E: HttpModeledError>(
    protocol: &SharedServerProtocol,
    error: E,
) -> Response {
    protocol.serialize_error(&error)
}
```

At the start of the plugin's request handling, capture the handle with
`let protocol = SharedServerProtocol::get_from_request(&request).cloned();` before consuming
the request or moving it into an async task. For body-collection failure, pass
`RuntimeError::InternalFailure(sdk::server::error::Error::new(err))` to `runtime_failure`. For
authentication failure, pass the modeled operation error returned by `config.map_error(err)`
to `authentication_failure` with the selected handle. `serialize_runtime_error` is equivalent
to the shared error's `into_response_with_protocol` override and does not require naming `P`.
Keep authentication errors modeled; converting them to runtime validation errors changes their
status, bindings, and response body.

Schema routing stores the selection in a private request-extension carrier before invoking the
selected operation's route, including its HTTP plugins. `SelectedOperation` is the public
read-only view. Per-operation middleware reads it with:

```rust
use sdk::server::schema::SelectedOperation;

let selected = SelectedOperation::get_from_request(&request).unwrap();
let operation = selected.operation();
let protocol = selected.protocol();
```

`FromParts` extractors read the same selection from their request parts:

```rust
let selected = SelectedOperation::get_from_extensions(&parts.extensions).unwrap();
```

`SharedServerProtocol::get_from_request` remains the convenience accessor when only the
protocol is needed. Direct `extensions.get::<SelectedOperation>()` calls do not read the
private carrier. Inserting or removing a public `SelectedOperation` value does not alter the
authoritative selection. Middleware cannot replace the selected operation, protocol, or
body limits; protocol replacement through `with_protocol` is no longer supported.

Middleware wrapping the whole router runs before selection, so the accessors return `None`
there. Routing rejections do not reach per-operation plugins. Middleware that forwards
requests must preserve their extensions, including when reconstructing a request.
Clearing or replacing all extensions discards the private carrier. The schema upgrade and
missing-handler fallback then return an empty `500` with no headers, without polling the
request body or invoking the handler. A body-routed protocol may already have collected
the input before middleware runs. This fallback intentionally differs from generated serde services,
which do not depend on request selection metadata: registered handlers still process requests,
and missing handlers return their protocol's internal-failure response. Missing selection in
a schema per-operation plugin indicates a wiring failure; handle it explicitly rather than
guessing a protocol from the tuple.

The shared runtime error's context-free fallback returns an empty body, its semantic status,
and a `RuntimeErrorExtension`, without protocol headers. It can provide a defensive response
when selection is missing, but it cannot preserve a modeled authentication error's framing.

### Future work: routing context held by upgrade services

To support middleware that discards extensions, a future design could build an upgrade stack
for each supported protocol-and-operation pair. Each upgrade would hold its immutable protocol,
operation, and body-limit context in service state. The router would select that stack, while
request extensions would provide the selection for middleware visibility. Tower services could
continue accepting `Request<B>`; the context would not need to become part of the request type.

The current router shares one operation route across protocols, and generated code constructs
the upgrade and applies HTTP plugins when a handler is registered. Binding context to upgrade
services would require deferring that construction until the selected protocol's context is
available. Before adopting this design, evaluate middleware state sharing, stack construction
and allocation costs, and parity for success, error, streaming, and missing-handler responses
when extensions are discarded. This is follow-up work; the current middleware contract requires
preserving extensions.

### Runtime support for one middleware API

A middleware library can keep one public entry point, such as
`plugins.authenticate(client, config)`, for generated serde, single-protocol schema, and multi-protocol
schema services. Callers do not inherently need to choose a serialization mode. The updated
runtime provides the following adapters for middleware retaining its existing `IntoResponse<P>`
bounds:

* One runtime blanket implementation, `impl<P, E: HttpModeledError> IntoResponse<P> for E`.
  Its context-aware method calls `protocol.serialize_error(&self)`. Schema codegen already
  supplies `HttpModeledError`, so no new per-error generated implementation is needed. Its
  context-free conversion returns an empty body with the modeled HTTP status, no protocol
  headers, and a `ModeledErrorExtension` containing the error's shape name. An invalid status
  falls back to 500. Generated serde error implementations remain unchanged.
* Context-aware `IntoResponse` implementations for `InternalFailureException` on protocol
  tuples of two through ten elements. They serialize shared `RuntimeError::InternalFailure`
  through the supplied protocol. A direct tuple conversion returns an empty 500 with no protocol
  headers and a `RuntimeErrorExtension`; it does not choose a tuple member. Existing
  concrete-marker conversions preserve their direct responses, and their context-aware methods
  frame through the supplied protocol.

Middleware must call the context-aware method when routing supplied a protocol, and the
existing direct conversion otherwise. Its shared conversion helper is:

The helper below requires the updated HTTP 1.x runtime. HTTP 1.x alone does not guarantee the
context-aware method exists: older HTTP 1.x runtimes expose only `into_response()`. The separate
HTTP 0.2 runtime also lacks the context-aware method.

```rust
use sdk::server::{
    response::{IntoResponse, Response},
    schema::ServerProtocol,
};

fn middleware_response<P, E: IntoResponse<P>>(
    error: E,
    protocol: Option<&dyn ServerProtocol>,
) -> Response {
    match protocol {
        Some(protocol) => error.into_response_with_protocol(protocol),
        None => error.into_response(),
    }
}
```

Both authentication failure and body-collection failure use this helper.
The existing `Op::Error: IntoResponse<P>` and `InternalFailureException: IntoResponse<P>` bounds
are satisfied automatically. Generated serde requests retain their static conversions; schema requests
use their selected protocol, whether `P` is one marker or a tuple. Existing generated serde and schema SDKs
need no new generated error implementation for this bridge.

`Infallible` retains its response support through the blanket implementation. A service-owned
type with both `HttpModeledError` and an explicit `IntoResponse<P>` implementation now conflicts
with the blanket. Remove the explicit implementation to use the selected protocol's modeled
serializer. If a custom HTTP response is required, use a distinct rejection or response wrapper
that does not implement `HttpModeledError`.

A runtime check alone cannot make an unsatisfied trait bound compile: the missing implementations
must be supplied first. Conversely, a tuple implementation is valid when its context-aware method
uses the selected protocol; the incorrect implementation is one that guesses a fixed framing
from the tuple.

## Extractor rejections through the selected protocol

The runtime API and regression tests are implemented. The Pokemon example will demonstrate
the API in a later change.

An extractor can reject a request before the operation handler runs. A service-owned rejection
may deliberately return a custom HTTP response. A runtime-owned error such as
`RuntimeError::Validation("Caller identity could not be resolved".to_string())` is instead
framed by the protocol that routing selected for that request. Both behaviors must work in
single-protocol and multi-protocol schema services.

### Response conversion contract

The existing `IntoResponse<P>` trait has a new default method:

```rust
use aws_smithy_http_server::{response::Response, schema::protocol::ServerProtocol};

pub trait IntoResponse<P> {
    fn into_response(self) -> Response;

    fn into_response_with_protocol(self, _protocol: &dyn ServerProtocol) -> Response
    where
        Self: Sized,
    {
        self.into_response()
    }
}
```

Existing implementations need no new method. The default calls their existing response conversion,
preserving the status, headers, body, and response extensions they choose. Custom errors can override
the new method when they want to use the selected protocol. The framework must not inspect an
arbitrary rejection's type or try to serialize its completed HTTP response again.

Both `DynUpgrade` and `DynStreamingUpgrade` call this method on extractor failure, passing
the protocol from the validated `SelectedOperation` already obtained before extraction:

```rust
Err(err) => return Ok(err.into_response_with_protocol(&**protocol)),
```

The `any_rejections::Two` through `Eight` wrappers forward the method and the protocol handle
to the contained rejection. This is required for handlers with multiple extractors: calling only
`into_response` inside a wrapper would lose the runtime error's override. Single-element tuples
already use the contained extractor's rejection type directly.

Keep `FromParts<P>` and `ServiceShape::Protocol` as they are. A multi-protocol service still uses
a tuple of its served protocols. Implementing `FromParts` separately for each protocol does not
satisfy `FromParts<(RestJson1, RpcV2Cbor)>`. Extractors intended for either service configuration
should implement `impl<P> FromParts<P>` with a rejection that supports every `P`.

### Shared runtime error

The new protocol-neutral `aws_smithy_http_server::runtime_error::RuntimeError` is also
available through the generated SDK's existing `server` re-export. Its semantic variants cover
`Validation(String)`, `Serialization(crate::Error)`, `InternalFailure(crate::Error)`,
`NotAcceptable`, and `UnsupportedMediaType`. It implements `Display` and `IntoResponse<P>`
for every `P`, including protocol tuples.

Its `into_response_with_protocol` override delegates to a new
`ServerProtocol::serialize_runtime_error` method. Built-in protocols render the shared error
using their corresponding existing protocol-specific runtime-error conversion, including response
extensions and existing wire quirks. An extractor's `Validation` must retain runtime-validation
semantics; do not reinterpret it as a modeled constraint violation through
`DeserializeError::ConstraintViolation`.

Keep existing protocol-specific `RuntimeError` types, imports, variants, and direct
`IntoResponse<RestJson1>` (etc.) conversions compatible. They remain explicitly protocol-specific.
Switching an extractor to the shared error requires an explicit import change.
See [FromParts for multiprotocol](#fromparts-for-multiprotocol) for the complete before-and-after
example using the generated SDK's re-export. Extraction remains independent of the protocol;
the runtime handles rejection framing.

The new shared error's direct `into_response()` has no selected-protocol context. Define its
fallback: an empty response with the semantic HTTP status (400 for validation or
serialization, 500 for internal failure, 406 for not acceptable, 415 for unsupported media type),
no protocol-specific headers, and a `RuntimeErrorExtension` carrying the error name. This fallback
is for the new shared type only. Schema upgrades must use the context-aware method; callers needing
protocol framing outside an upgrade must supply a protocol to that method. Existing protocol-local
error conversions retain their established responses.

Custom `ServerProtocol` implementations must add the required `serialize_runtime_error` method
to supply their framing for the shared error. The registered external-protocol regression test
exercises this method through routing into a generated operation's upgrade.

### Existing custom and built-in rejections

A service-owned error can keep `impl<P> IntoResponse<P>` and return the same custom response for
every protocol. It can alternatively override `into_response_with_protocol` to use request-specific
framing. Both receive the same hook from the upgrade; the error's implementation selects the behavior.
A rejection implemented only for one protocol still needs migration before use with a protocol tuple.

`MissingExtension` already implements `IntoResponse<P>` generically and returns an empty 500.
Keep that behavior through the default hook. Delegating a required field to `Extension<T>` can
propagate this rejection with `?`; `Option<Extension<T>>` swallows a missing-extension rejection
and returns `None`. A custom generic extractor bundling these fields continues to work unchanged.
Changing a validation failure to `MissingExtension` would change its HTTP semantics and is not a
substitute for adopting the shared runtime error. Preserve the existing generic responses of
missing connection-info, request-ID, and metrics extractors as well.

### Compatibility verification and regression tests

Before editing behavior-changing runtime or codegen implementation, generate and run a focused
generated serde service with schema serde disabled. Use the same model, protocol, configuration, and inputs
for the schema comparison. Record observed status, headers, exact body bytes, and relevant response
extensions for extractor success and rejection, including streaming operations. Report the baseline
and proposed differences before implementation; any difference from generated serde requires explicit user
direction. Source inspection is not a substitute for that reproduction.

Tests must exercise generated services and actual upgrade paths, not only direct trait calls:

| Case | Required assertions |
| --- | --- |
| Single-protocol generated serde extractor and custom rejection | Existing `FromParts<RestJson1>` and `IntoResponse<RestJson1>` compile unchanged; schema response matches the recorded generated serde response. |
| Shared runtime error on each single protocol | Each variant matches the corresponding generated serde protocol-local runtime error's status, headers, body bytes, and response extensions. |
| One multi-protocol service, generic extractor, shared validation error | Requests selecting restJson1 and rpcv2Cbor receive their respective generated serde runtime-validation framing; the selected protocol controls conversion. |
| Repeated requests selecting different protocols | Selection does not leak across requests or depend on protocol tuple ordering. |
| Multiple extractors | Failure at different tuple positions forwards the selected protocol through `any_rejections`; cover every generated wrapper arity from two through eight. |
| Existing custom response | A type implementing only the old conversion keeps its chosen status, custom headers, exact body, and response extensions on both protocols. |
| Custom context-aware rejection | Its override receives the selected protocol; its old conversion is not used by the schema upgrade. |
| Required and optional extensions | Required absence keeps the empty 500; optional absence reaches the handler with `None`. |
| Successful extraction | Handler receives the extracted value and runs once; every rejection skips the handler. |
| Streaming operations | Run shared and custom rejection cases through `DynStreamingUpgrade`; extraction failure must not poll the live request body. |
| Direct conversions | Old protocol-local conversions remain compatible; the new shared error's context-free fallback is pinned separately. |
| Custom protocol implementation | Its serializer is invoked for the shared error through routing, including a protocol registration supplied by the service. |

Keep generated serde observations explicit in comparison tests. Do not rewrite expectations to fit a new
serializer. In particular, assert the existing protocol behavior even when content types or body
bytes look surprising; wire-format cleanup is a separate compatibility decision.

A later change will update the multi-protocol Pokemon example to
show a generic custom extractor using the shared runtime error and a service-owned rejection using
the preserved default conversion. The example change is deferred.
