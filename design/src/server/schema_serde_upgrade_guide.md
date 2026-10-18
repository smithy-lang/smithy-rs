# Upgrading a Server to Schema-Driven Serde

This guide lists the source changes a service needs when it turns on `codegen.schemaSerde: true`.
Each entry describes what breaks or changes, how you notice it, and what to change.

Services that do not enable `schemaSerde` are unaffected.

## The service's protocol type

**What changes.** `ServiceShape::Protocol` names the protocols the service serves. For a service
with one protocol it is that protocol, as before (`RestJson1`, `RpcV2Cbor`, ...). For a service with
several protocols it is a tuple of them, for example `(RestJson1, RpcV2Cbor)`.

**How you notice.** Code that is generic over the service's protocol keeps compiling. Code written
for one protocol (a plugin, an extractor, an `IntoResponse` impl) stops compiling when it is used
with a multi-protocol service, instead of silently not applying.

## Custom `FromParts` extractors written for one protocol

**What changes.** Request-part extractors resolve against the service's protocol type. On a service
with one protocol, an extractor implemented for that protocol keeps working, and its rejection is
rendered by its `IntoResponse<RestJson1>` impl exactly as before:

```rust
impl FromParts<RestJson1> for CallerInfo {
    type Rejection = CallerInfoError;
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> { /* ... */ }
}
```

On a service with several protocols the protocol type is a tuple, so the same extractor no longer
satisfies the handler's bounds.

**How you notice.** A multi-protocol service fails to compile at the builder call that registers a
handler taking `CallerInfo`, with an unsatisfied `FromParts<(...)>` bound.

**What to change.** Make the extractor generic over the protocol:

```rust
impl<P> FromParts<P> for CallerInfo {
    type Rejection = CallerInfoError;
    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> { /* ... */ }
}
```

and implement `IntoResponse<P>` for the rejection type for every `P`.

**Rejection framing.** Schema upgrades call `IntoResponse<P>::into_response_with_protocol`,
passing the protocol selected for the request. The default preserves an existing rejection's
`into_response()` response. Use the shared `server::runtime_error::RuntimeError` to have the runtime
frame errors through that selected protocol, or override the method on your own rejection type.
See the migration example below.

## `IntoResponse<RestJson1>` implementations on your own types

**What changes.** `impl IntoResponse<RestJson1> for MyError` (or for `AwsJson1_0`, `RestXml`, and so
on) still compiles, but the schema path never calls it for handler errors. Handler errors are serialized
through the generated operation error types, so a custom `IntoResponse` impl is only used where your
code calls it directly, or as the rejection of a `FromParts` extractor (see above), for example:

```rust
let response = IntoResponse::<RestJson1>::into_response(err);
```

**How you notice.** You don't, at compile time. If you relied on the framework calling your impl,
the response your clients receive changes silently.

**What to change.** Search your code for `impl IntoResponse<` on protocol marker types and check
each one:

* If your code calls it directly (in middleware or a custom service), it keeps working as before.
* If you expected the framework to call it, return a modeled error from the handler instead, or
  build the response in a layer, so the error is serialized by the protocol that served the request.

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
Switching an extractor to the shared error is an explicit import change:

```rust
use aws_smithy_http_server::{request::FromParts, runtime_error::RuntimeError};
use http::request::Parts;

impl<P> FromParts<P> for CallerInfo {
    type Rejection = RuntimeError;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        extract_caller_info_from_parts(parts).ok_or_else(|| {
            RuntimeError::Validation("Caller identity could not be resolved".to_string())
        })
    }
}
```

This is a migration sketch using the service's existing `CallerInfo` and identity-extraction helper.
Extraction remains independent of the protocol; the runtime handles rejection framing.

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
legacy service with schema serde disabled. Use the same model, protocol, configuration, and inputs
for the schema comparison. Record observed status, headers, exact body bytes, and relevant response
extensions for extractor success and rejection, including streaming operations. Report the baseline
and proposed differences before implementation; any difference from legacy requires explicit user
direction. Source inspection is not a substitute for that reproduction.

Tests must exercise generated services and actual upgrade paths, not only direct trait calls:

| Case | Required assertions |
| --- | --- |
| Single-protocol legacy extractor and custom rejection | Existing `FromParts<RestJson1>` and `IntoResponse<RestJson1>` compile unchanged; schema response matches the recorded legacy response. |
| Shared runtime error on each single protocol | Each variant matches the corresponding legacy protocol-local runtime error's status, headers, body bytes, and response extensions. |
| One multi-protocol service, generic extractor, shared validation error | Requests selecting restJson1 and rpcv2Cbor receive their respective legacy runtime-validation framing; the selected protocol controls conversion. |
| Repeated requests selecting different protocols | Selection does not leak across requests or depend on protocol tuple ordering. |
| Multiple extractors | Failure at different tuple positions forwards the selected protocol through `any_rejections`; cover every generated wrapper arity from two through eight. |
| Existing custom response | A type implementing only the old conversion keeps its chosen status, custom headers, exact body, and response extensions on both protocols. |
| Custom context-aware rejection | Its override receives the selected protocol; its old conversion is not used by the schema upgrade. |
| Required and optional extensions | Required absence keeps the empty 500; optional absence reaches the handler with `None`. |
| Successful extraction | Handler receives the extracted value and runs once; every rejection skips the handler. |
| Streaming operations | Run shared and custom rejection cases through `DynStreamingUpgrade`; extraction failure must not poll the live request body. |
| Direct conversions | Old protocol-local conversions remain compatible; the new shared error's context-free fallback is pinned separately. |
| Custom protocol implementation | Its serializer is invoked for the shared error through routing, including a protocol registration supplied by the service. |

Keep legacy observations explicit in comparison tests. Do not rewrite expectations to fit a new
serializer. In particular, assert the existing protocol behavior even when content types or body
bytes look surprising; wire-format cleanup is a separate compatibility decision.

A later change will update the multi-protocol Pokemon example to
show a generic custom extractor using the shared runtime error and a service-owned rejection using
the preserved default conversion. The example change is deferred.
