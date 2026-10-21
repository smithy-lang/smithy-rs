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
It also implements `IntoResponseWithProtocol`, which schema upgrades require for extractor
rejections. Making the extractor generic while keeping the protocol-specific error is insufficient:
that error's static response conversion only supports its original protocol.

Schema upgrades call `IntoResponseWithProtocol::into_response_with_protocol`, passing the protocol selected
for the request. The shared error uses that protocol to frame the rejection. If you use your own
rejection type instead, implement `IntoResponseWithProtocol` to return its complete custom response
or to use the supplied protocol. Keep its `IntoResponse<P>` implementation as well if generated
serde callers need it. Modeled errors implementing `HttpModeledError` receive the contextual
implementation automatically.

## `IntoResponse<RestJson1>` implementations on your own types

**What changes.** `impl IntoResponse<RestJson1> for MyError` (or for `AwsJson1_0`, `RestXml`, and so
on) still compiles, including for types implementing `HttpModeledError`, but the schema path never
calls it for handler errors. The static and contextual response traits are independent.
Handler errors are serialized
through the generated operation error types, so a custom `IntoResponse` impl is only used where your
code calls it directly, or by a generated serde extractor (see above), for example:

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

Schema operation errors implement `HttpModeledError`, which supplies
`IntoResponseWithProtocol`. They do not need a static `IntoResponse<P>`
implementation. A middleware library can support both modes through one conversion
bound, without requiring callers to choose a different plugin.

The runtime defines the unit markers `response::GeneratedSerde` and
`response::SchemaSerde`. `Plugin<Ser, Op, T, SerdeMode = GeneratedSerde>` defaults to
generated serde, so older generated SDKs and existing three-argument plugin
implementations retain their original meaning. Schema codegen explicitly selects
`SchemaSerde` in HTTP plugin bounds. Model plugins keep their existing mode.

A middleware plugin that serializes errors forwards `SerdeMode` from its `Plugin`
implementation to its layer and service, and replaces static response bounds with:

```rust,ignore
Op::Error: IntoResponseWithRequest<P, SerdeMode>,
InternalFailureException: IntoResponseWithRequest<P, SerdeMode>,
```

Its response conversion can then use the same helper in both modes:

```rust
use http::Request;
use aws_smithy_http_server::response::{IntoResponseWithRequest, Response};

fn error_response<P, SerdeMode, E, B>(request: &Request<B>, error: E) -> Response
where
    E: IntoResponseWithRequest<P, SerdeMode>,
{
    error.into_response_with_request(request)
}
```

| SerdeMode | Conversion |
| --- | --- |
| `GeneratedSerde` | Calls the existing `IntoResponse<P>::into_response`; does not require a routed selection. |
| `SchemaSerde` | Reads `SelectedOperation` from request extensions and calls `IntoResponseWithProtocol` with its protocol. |

`P` remains the service's protocol type for the generated serde implementation.
In schema mode it may be a single protocol or a tuple; the request's selected
protocol determines the response in either case. The middleware does not choose
a tuple member or invent a default protocol.

Preserve request extensions while collecting or replacing the body. When body
collection fails after `request.into_parts()`, reconstruct a metadata-only request
from the retained parts before converting the error:

```rust,ignore
let request = http::Request::from_parts(parts, ());
return Ok(InternalFailureException.into_response_with_request(&request));
```

When schema selection is missing, conversion logs the missing context and returns
the existing empty 500 response. It cannot fall back to a static serializer whose
bounds are unavailable for schema errors. A custom contextual implementation's
status, headers, body, and extensions are returned without being serialized again.

### Existing HTTP plugins

Built-in plugin stacks, layers, filters, scopes, and references forward the mode.
A serde-independent plugin written with the original three-argument `Plugin`
implementation can be adapted when registering it:

```rust,ignore
let plugins = HttpPlugins::new().push_legacy(existing_plugin);
```

`LegacyHttpPlugin` delegates to that plugin's original generated-mode implementation.
It does not remove that implementation's bounds. A plugin that converts modeled
errors must adopt `IntoResponseWithRequest<P, SerdeMode>` so schema errors can satisfy
its bounds. Logging and header manipulation plugins can use the adapter without
changing their implementation.

### Authentication middleware and older runtimes

`SmithyRsServerSdkAwsAuthMiddleware` keeps the same `.authenticate(...)` and
`.authenticate_with_request_id(...)` entry points. Its HTTP 1.x plugin forwards
the service-selected mode through its layer and service. Authentication failures
and request-body collection failures both use `IntoResponseWithRequest`; normal
requests retain their request extensions and request-ID handling.

The internal `AwsSmithyHttpServer` facade inspects the resolved runtime's API.
For a runtime with modes it re-exports the mode markers and response converter,
and emits the four-argument plugin implementation. For an older HTTP 1.x runtime
it supplies a generated-only compatibility converter and emits the original
three-argument plugin implementation. The older branch does not refer to schema
APIs. HTTP 0.2 middleware retains its existing response conversions.

Update the facade and middleware packages together. Older generated SDKs do not
need regeneration merely to use the updated middleware. Schema SDKs must be
regenerated with the codegen that selects `SchemaSerde` for HTTP plugins.

## Extractor rejections through the selected protocol

The runtime API and regression tests are implemented. The Pokemon example will demonstrate
the API in a later change.

An extractor can reject a request before the operation handler runs. A service-owned rejection
may deliberately return a custom HTTP response. A runtime-owned error such as
`RuntimeError::Validation("Caller identity could not be resolved".to_string())` is instead
framed by the protocol that routing selected for that request. Both behaviors must work in
single-protocol and multi-protocol schema services.

### Response conversion contract

`IntoResponse<P>` retains its static `into_response()` method. A separate trait handles schema
response conversion:

```rust
use aws_smithy_http_server::{response::Response, schema::protocol::ServerProtocol};

pub trait IntoResponseWithProtocol {
    fn into_response_with_protocol(self, protocol: &dyn ServerProtocol) -> Response;
}
```

Modeled schema rejections need only `HttpModeledError`, which includes `SerializableStruct`:
the error supplies its schema, field values, and HTTP status, and receives the contextual conversion
automatically. The extractor can return that error without implementing either response trait.
The runtime chooses the protocol instance for each request, including on services with protocol tuples.

Custom HTTP rejections implement this trait explicitly. To preserve a complete custom response,
the implementation can delegate to the rejection's existing static conversion. To frame a rejection
through the selected protocol, use the supplied instance. The framework must not inspect an
arbitrary rejection's type or try to serialize its completed HTTP response again.

Both `DynUpgrade` and `DynStreamingUpgrade` call this method on extractor failure, passing
the protocol from the validated `SelectedOperation` already obtained before extraction:

```rust
Err(err) => return Ok(err.into_response_with_protocol(&**protocol)),
```

The `any_rejections::Two` through `Eight` wrappers forward the method and the protocol handle
to the contained rejection. This is required for handlers with multiple extractors: calling only
`into_response` inside a wrapper would lose the selected protocol. Single-element tuples
already use the contained extractor's rejection type directly.

`FromParts<P>` no longer imposes a response trait on `Rejection`; the consuming upgrade imposes
the static or contextual bound. Both upgrades require `Display` because they log the rejection's diagnostic
before returning its response; modeled errors satisfy this through their `std::error::Error` contract.
Generic helpers that previously relied on `FromParts<P>` implying `Rejection: IntoResponse<P>` must
state that static bound explicitly when they perform legacy response conversion.
`ServiceShape::Protocol` is unchanged. A multi-protocol service still uses
a tuple of its served protocols. Implementing `FromParts` separately for each protocol does not
satisfy `FromParts<(RestJson1, RpcV2Cbor)>`. Extractors intended for either service configuration
should implement `impl<P> FromParts<P>` with a contextual rejection. Generated serde callers
additionally need its static conversion for their protocol.

### Shared runtime error

The new protocol-neutral `aws_smithy_http_server::runtime_error::RuntimeError` is also
available through the generated SDK's existing `server` re-export. Its semantic variants cover
`Validation(String)`, `Serialization(crate::Error)`, `InternalFailure(crate::Error)`,
`NotAcceptable`, and `UnsupportedMediaType`. It implements `Display` and `IntoResponse<P>`
for every `P`, including protocol tuples.

Its `IntoResponseWithProtocol` implementation delegates to
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

### External protocol example: Coral RPC v1

An external protocol adapter implements response framing on `ServerProtocol`. It can reuse its
existing static runtime-error conversions while accepting protocol-independent extractor rejections.
Coral RPC v1 demonstrates this in `amzn-smithy-http-server-internal`.

Keep the existing `IntoResponse<CoralRpc1>` implementations for legacy callers. In the selected
`CoralRpc1Protocol`, convert the shared runtime error to Coral's existing runtime-error type and
delegate to that static conversion:

```rust
use aws_smithy_http_server::{
    response::{IntoResponse, Response},
    runtime_error::RuntimeError as SharedRuntimeError,
    schema::{HttpModeledError, ServerProtocol},
};
use crate::proto::coral_rpc_1::{
    CoralRpc1,
    http_1x::runtime_error::RuntimeError as CoralRuntimeError,
};

impl ServerProtocol for CoralRpc1Protocol {
    // Other protocol methods and the adapter's serialization helpers are omitted.
    fn serialize_runtime_error(&self, error: SharedRuntimeError) -> Response {
        IntoResponse::<CoralRpc1>::into_response(CoralRuntimeError::from(error))
    }

    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
        let schema = error.schema();
        serialize_modeled_error_response(
            self.inner.codec(),
            schema,
            &frame(schema, error),
            error.status_code(),
            ResponseBindings::BodyOnly,
            CONTENT_TYPE,
        )
        .map(|response| stamp_error_extension(response, schema.shape_id().shape_name()))
        .unwrap_or_else(serialization_failure)
    }
}
```

Here `frame` is Coral's existing helper that adds the full-shape-ID `__type` member.
`serialize_modeled_error_response` serializes the error's fields using the adapter's codec and
binding rules. The adapter retains its existing serialization-failure handling and stamps the
modeled-error extension. The runtime-error method retains Coral's existing status, headers,
body, and runtime-error extension through the legacy conversion.

An extractor intended for single-protocol and tuple services can implement `impl<P> FromParts<P>`
and return `aws_smithy_http_server::runtime_error::RuntimeError`, rather than Coral's
protocol-specific runtime-error alias. For example, it can return
`SharedRuntimeError::Validation("Caller identity could not be resolved".to_owned())`.
The upstream contextual implementation then calls the selected protocol's
`serialize_runtime_error`; when Coral is selected, the first method above supplies its framing.
The extractor does not choose `CoralRpc1` or construct an HTTP response.

For a modeled rejection, the extractor instead returns an error implementing `HttpModeledError`.
That contract already includes `SerializableStruct`, supplying the schema and field values.
The upstream blanket `impl<E: HttpModeledError> IntoResponseWithProtocol for E` calls the selected
protocol's `serialize_error`, reaching the second method above when Coral is selected.
The error needs no explicit contextual implementation or static `IntoResponse<P>` implementation
for schema callers.

The adapter therefore needs no additional `IntoResponseWithProtocol` implementations for Coral's
aliased upstream framework errors. A non-generic foreign trait cannot be implemented downstream
for a foreign type; use the shared runtime error or an owned modeled rejection. Register the adapter
in the service builder's protocol registry so routing can select it, including when the service's
protocol marker is a tuple. Verify its emitted responses against generated legacy services before
changing framing; direct adapter comparisons alone do not establish generated-service parity.

### Existing custom and built-in rejections

A service-owned error can keep `impl<P> IntoResponse<P>` for generated serde callers. Schema callers
need an explicit `IntoResponseWithProtocol` implementation unless the error implements
`HttpModeledError`. For an existing protocol-independent custom response, delegate to the static
conversion without changing its status, headers, body or extensions:

```rust
impl sdk::server::response::IntoResponseWithProtocol for MyRejection {
    fn into_response_with_protocol(
        self,
        _: &dyn sdk::server::schema::ServerProtocol,
    ) -> sdk::server::response::Response {
        sdk::server::response::IntoResponse::<()>::into_response(self)
    }
}
```

`MissingExtension` already implements `IntoResponse<P>` generically and returns an empty 500.
Its explicit contextual implementation preserves that response. Delegating a required field to `Extension<T>` can
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
| Single-protocol generated serde extractor and custom rejection | Existing `FromParts<RestJson1>` and `IntoResponse<RestJson1>` compile unchanged for generated serde; after adding the contextual conversion, the schema response matches the recorded generated serde response. |
| Shared runtime error on each single protocol | Each variant matches the corresponding generated serde protocol-local runtime error's status, headers, body bytes, and response extensions. |
| One multi-protocol service, generic extractor, shared validation error | Requests selecting restJson1 and rpcv2Cbor receive their respective generated serde runtime-validation framing; the selected protocol controls conversion. |
| Repeated requests selecting different protocols | Selection does not leak across requests or depend on protocol tuple ordering. |
| Multiple extractors | Failure at different tuple positions forwards the selected protocol through `any_rejections`; cover every generated wrapper arity from two through eight. |
| Existing custom response | An explicit contextual implementation delegating to the old conversion keeps its chosen status, custom headers, exact body, and response extensions on both protocols. |
| Custom context-aware rejection | Its contextual implementation receives the selected protocol without requiring any static conversion. |
| Modeled rejection without response implementations | `HttpModeledError` and `SerializableStruct` suffice for tuple extractors in ordinary and streaming upgrades; each selected protocol emits its error framing and modeled-error extension. |
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
an explicit contextual conversion preserving its custom response. The example change is deferred.
