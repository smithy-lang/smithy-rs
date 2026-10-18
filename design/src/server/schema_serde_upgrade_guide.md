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

**Caveat.** A generic rejection cannot produce a response formatted for the protocol of the
request being served (for example a restJson1 `X-Amzn-Errortype` header versus an rpcv2Cbor CBOR
body). Every protocol gets the same response. If clients depend on a protocol-specific rejection
response, raise it with the smithy-rs team before adding a second protocol.

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
