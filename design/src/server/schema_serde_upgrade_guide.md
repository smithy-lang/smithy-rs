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

Schema upgrades call `IntoResponseWithProtocol::into_response_with_protocol`, passing the protocol
selected for the request. The shared error uses that protocol to frame the rejection. If you use
your own rejection type instead, implement `IntoResponseWithProtocol`; it may return a complete
custom response (the upgrade does not re-serialize it) or use the selected protocol. Add an
`IntoResponse<P>` implementation only when the same rejection also serves generated serde
services.

## `IntoResponse<RestJson1>` implementations on your own types

**What changes.** `impl IntoResponse<RestJson1> for MyError` (or for `AwsJson1_0`, `RestXml`, and so
on) still compiles, but the schema path never calls it for handler errors. The two response traits
are independent: a type may implement `IntoResponse<P>`, `IntoResponseWithProtocol`, or both —
wrapper errors that forward `HttpModeledError` from an inner error and keep their own static
conversion are supported; see [Runtime support for one middleware API](#runtime-support-for-one-middleware-api).
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

**What changes.** Middleware such as an authentication plugin used to require
`Op::Error: IntoResponse<P>` and `InternalFailureException: IntoResponse<P>` to return early
when authentication or request-body collection fails. Those bounds cannot be satisfied by schema
services: their errors are serialized by the protocol selected per request and deliberately never
implement the context-free conversion, and `P` may be a protocol tuple. The runtime instead
provides `ServiceSerde`/`ErrorConverter` dispatch (see
[Runtime support for one middleware API](#runtime-support-for-one-middleware-api)): the generated
service names its converter, and one middleware implementation converts errors for both
serialization systems without a serde-mode parameter.

**How you notice.** Registering a plugin whose bounds demand `IntoResponse<(...)>` on a schema
service's errors fails to compile. Replacing those bounds with
`Ser::SerdeKind: ErrorConverter<...>` makes the same plugin compile against both SDK kinds.

**What to change.** Bound the plugin on `Ser: ServiceSerde` and convert through `Ser::SerdeKind`.
Schema-specific middleware can alternatively use shared `runtime_error::RuntimeError` for
framework failures and require `Op::Error: schema::HttpModeledError` to serialize mapped
authentication errors directly, as the following helpers illustrate.

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

A middleware library keeps one public entry point, such as
`plugins.authenticate(client, config)`, for generated serde, single-protocol schema, and
multi-protocol schema services. Callers never choose a serialization mode; the generated service
does, through one trait and one associated type:

```rust
// service.rs — implemented by every generated HTTP 1.x service.
pub trait ServiceSerde {
    type SerdeKind; // GeneratedSerde<Protocol> or SchemaSerde
}

// response.rs — the two converter markers, with exactly two implementations.
pub struct GeneratedSerde<P>(PhantomData<P>);
pub struct SchemaSerde;

pub trait ErrorConverter<E> {
    fn convert<B>(error: E, request: &http::Request<B>) -> Response;
}

impl<P, E: IntoResponse<P>> ErrorConverter<E> for GeneratedSerde<P> { /* context-free */ }
impl<E: IntoResponseWithProtocol> ErrorConverter<E> for SchemaSerde { /* via SelectedOperation */ }
```

Codegen emits `impl<S> ServiceSerde for MyService<S> { type SerdeKind = GeneratedSerde<RestJson1>; }`
for generated serde, or `type SerdeKind = SchemaSerde;` for schema serde. Middleware generic over the
service replaces its old `Op::Error: IntoResponse<Ser::Protocol>` and
`InternalFailureException: IntoResponse<Ser::Protocol>` bounds with:

```rust
impl<Ser, Op, S> Plugin<Ser, Op, S> for AuthenticatePlugin
where
    Ser: ServiceShape + ServiceSerde,
    Ser::SerdeKind: ErrorConverter<Op::Error> + ErrorConverter<InternalFailureException>,
    /* ... */
```

threading `Ser::SerdeKind` into its tower service as a phantom type parameter, and converting with
`<Converter as ErrorConverter<Op::Error>>::convert(error, &request)`. The `Plugin` trait keeps
its three parameters; dispatch rides on the service's associated type, and the two
`ErrorConverter` implementations live on disjoint marker types, so no blanket overlap arises.
`GeneratedSerde<P>` ignores the request (the protocol is the type); `SchemaSerde` reads the
`SelectedOperation` from the request extensions and answers an empty `500` if middleware
discarded them. On a body-collection failure, rebuild the request from its parts before
converting, so the extensions remain readable.

`ServiceSerde` exists only in the updated HTTP 1.x runtime; the HTTP 0.2 runtime and SDKs from
older code generators keep the context-free `IntoResponse<P>` middleware path. A middleware
library that must support both selects its implementation at build time from the runtime version
in the dependency graph (the `amzn-aws-smithy-http-server` facade publishes it as `links`
metadata).

`Infallible` implements both response traits, so error-free operations satisfy either converter.

## Extractor rejections through the selected protocol

The runtime API and regression tests are implemented. The Pokemon example will demonstrate
the API in a later change.

An extractor can reject a request before the operation handler runs. A service-owned rejection
may deliberately return a custom HTTP response. A runtime-owned error such as
`RuntimeError::Validation("Caller identity could not be resolved".to_string())` is instead
framed by the protocol that routing selected for that request. Both behaviors must work in
single-protocol and multi-protocol schema services.

### Response conversion contract

Response conversion is split into two traits, one per serialization system:

```rust
use aws_smithy_http_server::{response::Response, schema::ServerProtocol};

/// Context-free conversion for a statically known protocol. Generated serde.
pub trait IntoResponse<Protocol> {
    fn into_response(self) -> Response;
}

/// Conversion through the protocol instance selected for this request. Schema serde.
pub trait IntoResponseWithProtocol {
    fn into_response_with_protocol(self, protocol: &dyn ServerProtocol) -> Response;
}
```

There is no blanket implementation of `IntoResponse<P>` for modeled errors and no defaulted
method bridging the two: a schema error cannot be converted without the routed protocol, and a
blanket would conflict with wrapper errors that forward both traits. Instead, one runtime blanket
covers every modeled error on the contextual side:
`impl<E: HttpModeledError> IntoResponseWithProtocol for E` delegates to
`protocol.serialize_error(&self)`. A custom rejection implements `IntoResponseWithProtocol` to
return its own complete HTTP response (which the upgrade must not re-serialize) or to use the
selected protocol; it can independently implement `IntoResponse<P>` for generated serde callers.

**Where the requirement is enforced.** `FromParts::Rejection` carries no trait bound. An
extractor promises only that it can fail; what its failure must convert *to* depends on the
service it is attached to, so the bound lives on the pipeline that performs the conversion:

- the generated serde handler path requires `Rejection: IntoResponse<P>` (its `FromRequest`
  tuple implementations and operation upgrade);
- the schema path requires `Rejection: IntoResponseWithProtocol` (`DynUpgrade` and
  `DynStreamingUpgrade` bounds), and on extractor failure calls:

```rust
Err(err) => return Ok(err.into_response_with_protocol(&**protocol)),
```

passing the protocol from the validated `SelectedOperation` obtained before extraction.
Registering a handler whose extractor rejection lacks the required conversion fails to compile
at the builder method — the same place extractor mistakes have always surfaced. The loosened
associated type is therefore not a loss of enforcement; it is the same enforcement moved to the
only place that knows which conversion applies.

The `any_rejections::Two` through `Eight` wrappers implement each trait independently, when
every variant implements that trait. Single-element tuples already use the contained extractor's
rejection type directly.

Keep `FromParts<P>` and `ServiceShape::Protocol` as they are. A multi-protocol service still uses
a tuple of its served protocols. Implementing `FromParts` separately for each protocol does not
satisfy `FromParts<(RestJson1, RpcV2Cbor)>`. Extractors intended for either service configuration
should implement `impl<P> FromParts<P>`, with `IntoResponse<P>` and/or `IntoResponseWithProtocol`
on the rejection according to where the extractor is used.

### Shared runtime error

The new protocol-neutral `aws_smithy_http_server::runtime_error::RuntimeError` is also
available through the generated SDK's existing `server` re-export. Its semantic variants cover
`Validation(String)`, `Serialization(crate::Error)`, `InternalFailure(crate::Error)`,
`NotAcceptable`, and `UnsupportedMediaType`. It implements `Display`, `IntoResponse<P>` for
every `P` (including protocol tuples), and `IntoResponseWithProtocol`.

Its `IntoResponseWithProtocol` implementation delegates to the
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

A service-owned rejection used on schema services implements `IntoResponseWithProtocol`; it can
return the same complete custom response for every protocol (the upgrade does not re-serialize
it) or use the supplied protocol for request-specific framing. A rejection also used on
generated serde services keeps its `impl<P> IntoResponse<P>` alongside — the traits are
independent. A rejection implemented only for one concrete protocol still needs migration before
use with a protocol tuple.

`MissingExtension` already implements `IntoResponse<P>` generically and returns an empty 500.
Keep that behavior through its explicit `IntoResponseWithProtocol` implementation, which
returns the same response as its context-free conversion. Delegating a required field to `Extension<T>` can
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
