# Upgrading a Server to Schema-Driven Serde

This guide covers enabling schema serde, adding protocols, and upgrading extractors
and middleware. In the examples, `sdk` means your generated server SDK crate.
Names such as `CallerInfo`, `authenticate`, and `map_stream_error` stand for your
application's types and helpers.

`schemaSerde: false` uses **generated serde**, which emits protocol serializers
and deserializers. `schemaSerde: true` uses **schema serde**, where the runtime
serializes typed values through their schemas and member walkers. Schema serde
supports single-protocol and multi-protocol services.

This guide describes the updated HTTP 1.x runtime and generator, including
`response::ModeledRejection`. Generated serde retains its existing response
conversions. Runtime upgrades and adding protocols can require dependency and
protocol-bound changes independently of serde mode.

## 1. Upgrade the runtime and regenerate your server SDK

Schema serde uses HTTP 1.x. Align your application and middleware with
`aws-smithy-http-server`, HTTP 1.x, and Tower 0.5. The HTTP 0.x runtime,
`aws-smithy-legacy-http-server`, retains its generated-serde API.

Put these settings inside your existing server codegen plugin configuration:

```json
{
  "codegen": {
    "http-1x": true,
    "schemaSerde": true
  }
}
```

Use `"codegen"` as the configuration key. Regenerate the SDK with the updated
generator, then rebuild its consumers. Schema serde enables HTTP 1.x when the
HTTP setting is omitted; explicitly setting `"http-1x": false` is invalid.

Code that constructs a protocol marker as a unit struct changes to:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::protocol::rest_json_1::RestJson1;
let protocol = RestJson1::default();
```

For request-specific serialization, use the routed protocol instance. It carries
both the request's selected protocol and that instance's configuration.
`P::default()` cannot replace this selection: a protocol tuple is not a
`ServerProtocol`, and a concrete default can lose configured behavior.

## 2. Declare the protocols your service supports

For example, add both protocol traits to your existing service:

```smithy
use aws.protocols#restJson1
use smithy.protocols#rpcv2Cbor

@restJson1
@rpcv2Cbor
service MyService {
    version: "2026-01-01"
    operations: [GetThing]
}
```

Register each operation handler once. This Rust example uses the generated
Pokémon SDK; substitute your service's names and handlers:

```rust,no_run
# extern crate pokemon_service_server_schema_sdk as sdk;
# async fn get_pokemon_species(
#     input: sdk::input::GetPokemonSpeciesInput,
# ) -> Result<sdk::output::GetPokemonSpeciesOutput, sdk::error::GetPokemonSpeciesError> {
#     Err(sdk::error::ResourceNotFoundException {
#         message: format!("No species named {}", input.name),
#     }.into())
# }
# async fn unused_handler<Op: sdk::server::operation::OperationShape>(
#     _: Op::Input,
# ) -> Result<Op::Output, Op::Error> {
#     std::future::pending().await
# }
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let config = sdk::PokemonServiceConfig::builder().build();

let service = sdk::PokemonService::builder(config)
    .get_pokemon_species(get_pokemon_species)
#     .capture_pokemon(unused_handler::<sdk::operation_shape::CapturePokemon>)
#     .do_nothing(unused_handler::<sdk::operation_shape::DoNothing>)
#     .get_server_statistics(unused_handler::<sdk::operation_shape::GetServerStatistics>)
#     .get_storage(unused_handler::<sdk::operation_shape::GetStorage>)
#     .stream_pokemon_radio(unused_handler::<sdk::operation_shape::StreamPokemonRadio>)
    .build()?;
# Ok(())
# }
```

Routing selects the protocol for each request. Your handler receives the same
modeled Rust input and returns the same modeled Rust output regardless of which
protocol served the request.

Most handler signatures can remain unchanged. The CBOR streaming exception is
covered in [step 9](#9-review-streaming-handlers-and-compatibility-settings).

## 3. Treat the service's protocol type as its supported protocol set

The generated associated type changes with the service:

| Service | `ServiceShape::Protocol` |
| --- | --- |
| REST JSON only | `RestJson1` |
| REST JSON and CBOR | `(RestJson1, RpcV2Cbor)` |

Review code that explicitly requires `RestJson1`, including extractors, plugins,
and response implementations. A concrete-protocol implementation can still
compile on its own while failing the bounds when attached to a multi-protocol
service.

Two separate implementations of `FromParts<RestJson1>` and
`FromParts<RpcV2Cbor>` do not satisfy `FromParts<(RestJson1, RpcV2Cbor)>`.
A generic implementation usually handles both service configurations.

## 4. Make request-part extractors generic and choose their rejection

Your old extractor might look like:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# use sdk::server::http;
# #[derive(Clone, Debug)]
# struct CallerInfo;
# fn extract_caller_info_from_parts(parts: &http::request::Parts) -> Option<CallerInfo> {
#     parts.extensions.get::<CallerInfo>().cloned()
# }
use sdk::server::{
    protocol::rest_json_1::{RestJson1, runtime_error::RuntimeError},
    request::FromParts,
};

impl FromParts<RestJson1> for CallerInfo {
    type Rejection = RuntimeError;

    fn from_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        extract_caller_info_from_parts(parts).ok_or_else(|| {
            RuntimeError::Validation("Caller identity could not be resolved".to_owned())
        })
    }
}
```

For a protocol-independent extractor returning a framework error, change both
the trait implementation and the rejection type:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# use sdk::server::http;
# #[derive(Clone, Debug)]
# struct CallerInfo;
# fn extract_caller_info_from_parts(parts: &http::request::Parts) -> Option<CallerInfo> {
#     parts.extensions.get::<CallerInfo>().cloned()
# }
use http::request::Parts;
use sdk::server::{request::FromParts, runtime_error::RuntimeError};

impl<P> FromParts<P> for CallerInfo {
    type Rejection = RuntimeError;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        extract_caller_info_from_parts(parts).ok_or_else(|| {
            RuntimeError::Validation("Caller identity could not be resolved".to_owned())
        })
    }
}
```

Making only `FromParts` generic is insufficient if its rejection still implements
response conversion for just one concrete protocol. The shared `RuntimeError`
implements `IntoResponse<P>` for every `P`, including protocol tuples. The schema
pipeline passes the selected protocol to its contextual conversion method.

The original associated-type guarantee remains intact:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::{request::FromParts, response::{IntoResponse, Response}};

fn rejection_response<P, T: FromParts<P>>(error: T::Rejection) -> Response {
    // FromParts already guarantees that its rejection implements IntoResponse<P>.
    error.into_response()
}
```

Existing generic callers relying on that bound need no additional constraint.

For a new handwritten modeled extractor error, prefer `ModeledRejection<MyError>`.
Implement the schema, member walker, and `HttpModeledError` on `MyError`; the
wrapper supplies response conversion. `HttpModeledError` alone has no blanket
`IntoResponse<P>` implementation. Generated errors and errors with an existing
conversion can still be used directly.

Choose a rejection that preserves the error's semantics:

| Rejection | Contracts | When to use it |
| --- | --- | --- |
| `ModeledRejection<MyError>` | `MyError: HttpModeledError`; the wrapper supplies `IntoResponse<P>` | Recommended for new handwritten modeled extractor errors; the runtime handles response conversion. |
| A modeled error directly | `HttpModeledError` and `IntoResponse<P>` | Generated modeled errors, errors with an existing conversion, or a handwritten error whose conversion you explicitly choose to implement. |
| Shared `RuntimeError` | Already implements `IntoResponse<P>` | Framework failures and runtime validation, without a modeled error shape. |
| A custom response rejection | Your `IntoResponse<P>` implementation | A complete HTTP response whose status, headers, body, and extensions you control. |

The Pokémon example has a concrete implementation in
[`PreferredLanguage`](https://github.com/smithy-lang/smithy-rs/blob/main/examples/pokemon-service-common/src/language.rs).
It reads `Accept-Language` without changing the Smithy operation input and
returns `ModeledRejection<InvalidAcceptLanguage>` for malformed preferences.
`InvalidAcceptLanguage` is handwritten: it defines its schema and member walker
and implements `HttpModeledError` with status `400`. The wrapper supplies
`IntoResponse<P>` and lets the selected protocol serialize the message and error
identity. The error needs no generated shape or handwritten response conversion.
`GetPokemonSpecies` takes this extractor as an additional handler argument,
returns the selected translation, and retains all translations when the header
is absent. Its HTTP plugin adds `Vary: Accept-Language` to the response.
The same extractor works with the example's five-protocol tuple; its integration
tests exercise translation selection and rejection framing for each protocol.

### Recommended: let the runtime provide conversion with `ModeledRejection`

For a new handwritten modeled extractor error, define its schema, member
walker, error traits, and `HttpModeledError` status as in the handwritten example
below. Omit the error's `IntoResponse<P>` implementation: the runtime wrapper
supplies that conversion:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# extern crate aws_smithy_schema;
# use sdk::server::http;
# use std::sync::LazyLock;
# use aws_smithy_schema::{
#     serde::{SerdeError, SerializableStruct, ShapeSerializer},
#     shape_id, Schema, ShapeType, StringTrait, TraitMap,
# };
# use sdk::server::{
#     body,
#     extension::ModeledErrorExtension,
#     response::{IntoResponse, Response},
#     schema::{HttpModeledError, ServerProtocol},
# };
#
# #[derive(Debug)]
# struct MyError {
#     message: String,
# }
#
# static MESSAGE: Schema<'static> = Schema::new_member(
#     shape_id!("my_service.auth", "MyError", "message"),
#     ShapeType::String,
#     "message",
#     0,
# );
# static ERROR_TRAITS: LazyLock<TraitMap> = LazyLock::new(|| {
#     let mut traits = TraitMap::new();
#     traits.insert(Box::new(StringTrait::new(
#         shape_id!("smithy.api", "error"),
#         "client",
#     )));
#     traits
# });
# static ERROR_SCHEMA: Schema<'static> = Schema::new_struct(
#     shape_id!("my_service.auth", "MyError"),
#     ShapeType::Structure,
#     &[&MESSAGE],
# )
# .with_traits(&ERROR_TRAITS);
#
# impl std::fmt::Display for MyError {
#     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
#         f.write_str(&self.message)
#     }
# }
#
# impl std::error::Error for MyError {}
#
# impl SerializableStruct for MyError {
#     fn schema(&self) -> &Schema<'_> {
#         &ERROR_SCHEMA
#     }
#
#     fn serialize_members(
#         &self,
#         serializer: &mut dyn ShapeSerializer,
#     ) -> Result<(), SerdeError> {
#         serializer.write_string(&MESSAGE, &self.message)
#     }
# }
#
# impl HttpModeledError for MyError {
#     fn status_code(&self) -> u16 {
#         401
#     }
# }
# #[derive(Clone, Debug)]
# struct CallerInfo;
# fn authenticate(parts: &http::request::Parts) -> Result<CallerInfo, MyError> {
#     parts.extensions.get::<CallerInfo>().cloned().ok_or_else(|| MyError {
#         message: "Caller identity could not be resolved".to_owned(),
#     })
# }
use http::request::Parts;
use sdk::server::{
    request::FromParts,
    response::ModeledRejection,
};

impl<P> FromParts<P> for CallerInfo {
    type Rejection = ModeledRejection<MyError>;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        // Your function returns Result<CallerInfo, MyError>.
        authenticate(parts).map_err(Into::into)
    }
}
```

`ModeledRejection<E>` supplies `IntoResponse<P>` for every `P`. You write no
response-conversion implementation. This is the recommended path for new
handwritten errors. Generated errors and existing conversion implementations can
still be used directly. Both direct modeled rejections and wrapped rejections
reach `into_response_with_protocol` with the selected protocol through ordinary
and streaming schema upgrades. Tuple rejection wrappers forward that call to
the failing extractor. For the adapter, the path is:

```text
Router selects protocol
    → FromParts::from_parts returns Err(ModeledRejection(error))
    → Schema upgrade calls into_response_with_protocol(selected_protocol)
    → Wrapper calls selected_protocol.serialize_error(&error)
```

`error.into()` and `ModeledRejection::new(error)` construct the wrapper.
`into_inner()` returns the wrapped error. Its `Display` delegates to the error,
preserving diagnostics in runtime logs.

Existing generic callers can still invoke the wrapper's `into_response()`.
That static fallback returns the modeled status and `ModeledErrorExtension` with
an empty body and no protocol-specific headers; invalid statuses become `500`.
Use this adapter for schema serde, where the upgrade supplies the selected
protocol. A generated-serde upgrade calls its static method and receives that
fallback.

### Alternative: use a modeled error directly

For an application rejection with a modeled error shape, retain that error's
schema and status. You can use the error directly as `type Rejection` when it
implements both `HttpModeledError` and `IntoResponse<P>`. The error can be generated
from a Smithy model or written entirely by hand.

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# extern crate aws_smithy_schema;
# use sdk::server::http;
# use std::sync::LazyLock;
# use aws_smithy_schema::{
#     serde::{SerdeError, SerializableStruct, ShapeSerializer},
#     shape_id, Schema, ShapeType, StringTrait, TraitMap,
# };
# use sdk::server::{
#     body,
#     extension::ModeledErrorExtension,
#     response::{IntoResponse, Response},
#     schema::{HttpModeledError, ServerProtocol},
# };
#
# #[derive(Debug)]
# struct MyError {
#     message: String,
# }
#
# static MESSAGE: Schema<'static> = Schema::new_member(
#     shape_id!("my_service.auth", "MyError", "message"),
#     ShapeType::String,
#     "message",
#     0,
# );
# static ERROR_TRAITS: LazyLock<TraitMap> = LazyLock::new(|| {
#     let mut traits = TraitMap::new();
#     traits.insert(Box::new(StringTrait::new(
#         shape_id!("smithy.api", "error"),
#         "client",
#     )));
#     traits
# });
# static ERROR_SCHEMA: Schema<'static> = Schema::new_struct(
#     shape_id!("my_service.auth", "MyError"),
#     ShapeType::Structure,
#     &[&MESSAGE],
# )
# .with_traits(&ERROR_TRAITS);
#
# impl std::fmt::Display for MyError {
#     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
#         f.write_str(&self.message)
#     }
# }
#
# impl std::error::Error for MyError {}
#
# impl SerializableStruct for MyError {
#     fn schema(&self) -> &Schema<'_> {
#         &ERROR_SCHEMA
#     }
#
#     fn serialize_members(
#         &self,
#         serializer: &mut dyn ShapeSerializer,
#     ) -> Result<(), SerdeError> {
#         serializer.write_string(&MESSAGE, &self.message)
#     }
# }
#
# impl HttpModeledError for MyError {
#     fn status_code(&self) -> u16 {
#         401
#     }
# }
#
# impl<P> IntoResponse<P> for MyError {
#     fn into_response(self) -> Response {
#         // Define the behavior for callers without a selected protocol.
#         let mut response = Response::new(body::empty());
#         *response.status_mut() = http::StatusCode::from_u16(self.status_code())
#             .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
#         response.extensions_mut().insert(ModeledErrorExtension::new(
#             aws_smithy_schema::intern_header_name(self.schema().shape_id().shape_name()),
#         ));
#         response
#     }
#
#     fn into_response_with_protocol(self, protocol: &dyn ServerProtocol) -> Response {
#         // Schema upgrades supply the actual routed protocol instance.
#         protocol.serialize_error(&self)
#     }
# }
# #[derive(Clone, Debug)]
# struct CallerInfo;
# fn authenticate(parts: &http::request::Parts) -> Result<CallerInfo, MyError> {
#     parts.extensions.get::<CallerInfo>().cloned().ok_or_else(|| MyError {
#         message: "Caller identity could not be resolved".to_owned(),
#     })
# }
use http::request::Parts;
use sdk::server::request::FromParts;

impl<P> FromParts<P> for CallerInfo {
    type Rejection = MyError;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        // Your function returns Result<CallerInfo, MyError>.
        authenticate(parts)
    }
}
```

Schema-generated modeled error structures and operation error enums already
implement both contracts for every `P`, including protocol tuples. For example,
if your SDK contains `ValidationException`, the associated type can be
`type Rejection = sdk::error::ValidationException;`. No wrapper or handwritten
response conversion is required for those generated types.

`HttpModeledError` alone does not imply `IntoResponse<P>`. For new handwritten
errors, prefer the `ModeledRejection` adapter shown above. If you need to own
the conversion, the following example shows the complete direct implementation.

### Alternative: write the modeled error and its response conversion yourself

A handwritten error supplies its schema, member walker, error traits, and HTTP
status. Add `aws-smithy-schema` to your application's dependencies using the same
version as the generated SDK. This complete error example has one message
member and a modeled `401` status:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# extern crate aws_smithy_schema;
# use sdk::server::http;
use std::sync::LazyLock;
use aws_smithy_schema::{
    serde::{SerdeError, SerializableStruct, ShapeSerializer},
    shape_id, Schema, ShapeType, StringTrait, TraitMap,
};
use sdk::server::{
    body,
    extension::ModeledErrorExtension,
    response::{IntoResponse, Response},
    schema::{HttpModeledError, ServerProtocol},
};

#[derive(Debug)]
struct MyError {
    message: String,
}

static MESSAGE: Schema<'static> = Schema::new_member(
    shape_id!("my_service.auth", "MyError", "message"),
    ShapeType::String,
    "message",
    0,
);
static ERROR_TRAITS: LazyLock<TraitMap> = LazyLock::new(|| {
    let mut traits = TraitMap::new();
    traits.insert(Box::new(StringTrait::new(
        shape_id!("smithy.api", "error"),
        "client",
    )));
    traits
});
static ERROR_SCHEMA: Schema<'static> = Schema::new_struct(
    shape_id!("my_service.auth", "MyError"),
    ShapeType::Structure,
    &[&MESSAGE],
)
.with_traits(&ERROR_TRAITS);

impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MyError {}

impl SerializableStruct for MyError {
    fn schema(&self) -> &Schema<'_> {
        &ERROR_SCHEMA
    }

    fn serialize_members(
        &self,
        serializer: &mut dyn ShapeSerializer,
    ) -> Result<(), SerdeError> {
        serializer.write_string(&MESSAGE, &self.message)
    }
}

impl HttpModeledError for MyError {
    fn status_code(&self) -> u16 {
        401
    }
}

impl<P> IntoResponse<P> for MyError {
    fn into_response(self) -> Response {
        // Define the behavior for callers without a selected protocol.
        let mut response = Response::new(body::empty());
        *response.status_mut() = http::StatusCode::from_u16(self.status_code())
            .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);
        response.extensions_mut().insert(ModeledErrorExtension::new(
            aws_smithy_schema::intern_header_name(self.schema().shape_id().shape_name()),
        ));
        response
    }

    fn into_response_with_protocol(self, protocol: &dyn ServerProtocol) -> Response {
        // Schema upgrades supply the actual routed protocol instance.
        protocol.serialize_error(&self)
    }
}
# let response = IntoResponse::<()>::into_response(MyError { message: "missing caller".to_owned() });
# assert_eq!(response.status(), 401);
# assert!(response.extensions().get::<ModeledErrorExtension>().is_some());
```

Use this `MyError` directly in the extractor above. `SerializableStruct` supplies
the schema and walks the value's fields; schema metadata alone cannot read a
Rust value. `HttpModeledError` requires `SerializableStruct`, `std::error::Error`,
`Send`, `Sync`, and `'static`, and supplies the modeled HTTP status and optional
operation context. An error returned outside an operation can retain the default
`operation_schema()` result of `None`.

The generic `IntoResponse<P>` implementation works for both a concrete protocol
and a protocol tuple. Its static method explicitly chooses an empty response
with the modeled status and error extension. Its contextual method preserves
the schema and lets the selected protocol serialize the error's members and
protocol-specific headers. The handwritten error names no concrete protocol.

### Required and optional extensions

Keep the existing extraction semantics. A missing required `Extension<T>`
produces `MissingExtension`, whose generic response is an empty `500`.
`Option<Extension<T>>` allows the handler to run with `None`. Custom extractors
can continue delegating required fields with `?`.

Do not replace a validation or modeled authentication error with
`MissingExtension`; that changes its HTTP semantics. Existing connection-info,
request-ID, and metrics rejections retain their custom response behavior.

## 5. Keep existing custom responses; override contextual conversion when needed

An existing rejection that deliberately returns a complete HTTP response can
remain unchanged:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# use sdk::server::http;
use sdk::server::response::{IntoResponse, Response};
# struct MissingCaller;
impl<P> IntoResponse<P> for MissingCaller {
    fn into_response(self) -> Response {
        http::Response::builder()
            .status(401)
            .header("x-caller-error", "missing")
            .body(sdk::server::body::to_boxed("missing caller"))
            .unwrap()
    }
}
# let response = IntoResponse::<()>::into_response(MissingCaller);
# assert_eq!(response.status(), 401);
# assert_eq!(response.headers()["x-caller-error"], "missing");
```

The new trait method preserves this response by default. An explicit
implementation of that behavior is:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# use sdk::server::http;
use sdk::server::response::{IntoResponse, Response};
# struct MissingCaller;
use sdk::server::schema::ServerProtocol;

impl<P> IntoResponse<P> for MissingCaller {
#     fn into_response(self) -> Response {
#         http::Response::builder().status(401)
#             .body(sdk::server::body::to_boxed("missing caller")).unwrap()
#     }
    fn into_response_with_protocol(self, _protocol: &dyn ServerProtocol) -> Response {
        <Self as IntoResponse<P>>::into_response(self)
    }
}
```

Schema serde therefore preserves that rejection's status, headers, body, and
extensions. Tuple rejection wrappers forward the contextual method to the
failing extractor. An implementation for one concrete protocol still needs to
be made generic before use with a protocol tuple.

If you maintain a conversion implementation whose response must follow the
selected protocol's encoding, override the new method. For an error implementing
`HttpModeledError`, the override can delegate to this helper:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::{response::Response, schema::{HttpModeledError, ServerProtocol}};

fn contextual_response<E: HttpModeledError>(
    error: &E,
    protocol: &dyn ServerProtocol,
) -> Response {
    protocol.serialize_error(error)
}
```

Keep or define its context-free `into_response()` behavior too. A generated or
handwritten modeled error with both conversion methods can be the extractor's
rejection directly. For a new handwritten modeled extractor error, prefer
`ModeledRejection<MyError>` from step 4 so the runtime supplies both methods.

There is no blanket `IntoResponse<P>` implementation for every
`HttpModeledError`: it would conflict with existing types implementing both
traits. The runtime-owned wrapper supplies the bridge while preserving those
implementations.

The shared `RuntimeError` also supplies both methods. Its contextual method calls
`serialize_runtime_error`, preserving each protocol's existing runtime-error
framing. Its static fallback has an empty body, a `RuntimeErrorExtension`, and
the semantic status: `400` for validation or serialization, `500` for internal
failure, `406` for not acceptable, and `415` for unsupported media type.
Existing protocol-local runtime errors retain their established conversions.
Do not reinterpret a runtime validation error as a modeled constraint violation.

## 6. Continue returning modeled errors from operation handlers

Keep returning your generated operation error enum. For example, this Pokémon
handler returns a modeled not-found error:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
async fn get_pokemon_species(
    input: sdk::input::GetPokemonSpeciesInput,
) -> Result<sdk::output::GetPokemonSpeciesOutput, sdk::error::GetPokemonSpeciesError> {
    Err(sdk::error::ResourceNotFoundException {
        message: format!("No species named {}", input.name),
    }.into())
}
```

The selected protocol serializes the modeled error, including its modeled status
and bindings. Schema handler execution uses `HttpModeledError` directly; it does
not require the handler to choose a protocol or wrap its operation error in
`ModeledRejection`.

Authentication middleware should likewise retain the modeled operation error
returned by `config.map_error(err)`. Converting it into a runtime validation error
would change its HTTP status, bindings, and response body.

The current generator also emits response implementations for schema-generated
modeled error structures and operation error enums. Direct context-free calls:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::response::IntoResponse;

let schema_generated_error = sdk::error::ResourceNotFoundException {
    message: "No species found".to_owned(),
};
let response = IntoResponse::<()>::into_response(schema_generated_error);
# assert_eq!(response.status(), 404);
```

return an empty response with the modeled status and error extension. To obtain
protocol framing, use the service's converter or supply the selected protocol
to the contextual method. The direct fallback is not the handler's serialization
path.

## 7. Use the service's converter in middleware supporting both serde modes

The updated AWS auth middleware tested with this runtime retains its application
entry point, `HttpPlugins::new().authenticate(authenticator, config)`.
That extension method belongs to the auth library. With a constructed HTTP
authentication plugin, the public runtime API is:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::plugin::{HttpMarker, HttpPlugins, IdentityPlugin, PluginStack};

fn authentication_plugins<P: HttpMarker>(
    authentication_plugin: P,
) -> HttpPlugins<PluginStack<P, IdentityPlugin>> {
    HttpPlugins::new().push(authentication_plugin)
}
```

That implementation already performs the required dispatch. Check that your
dependency includes this updated implementation; an older package may still
call `into_response()` directly.

If you maintain middleware yourself, import the runtime contracts:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::{
    http::Request,
    operation::OperationShape,
    response::{ErrorConverter, Response},
    runtime_error::InternalFailureException,
    service::{ServiceSerde, ServiceShape},
};
```

Add these bounds to the plugin implementation:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::{
    http::Request,
    operation::OperationShape,
    response::{ErrorConverter, Response},
    runtime_error::InternalFailureException,
    service::{ServiceSerde, ServiceShape},
};

fn authentication_response<Ser, Op, B>(
    mapped_error: Op::Error,
    request: &Request<B>,
) -> Response
where
    Ser: ServiceShape + ServiceSerde,
    Op: OperationShape,
    Ser::SerdeKind:
        ErrorConverter<Op::Error> + ErrorConverter<InternalFailureException>,
{
    <Ser::SerdeKind as ErrorConverter<Op::Error>>::convert(mapped_error, request)
}
```

Thread `Ser::SerdeKind` into the middleware service as its `Converter` type.
`ServiceSerde` is implemented by the regenerated HTTP 1.x service and supplies
the converter marker. The `Plugin<Ser, Op, S>` trait retains its three parameters;
applications do not choose a serde mode.

Replace direct error conversion of the mapped error:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::response::{IntoResponse, Response};

fn generated_error_response<P, E: IntoResponse<P>>(mapped_error: E) -> Response {
    IntoResponse::<P>::into_response(mapped_error)
}
```

with:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::{
    http::Request,
    operation::OperationShape,
    response::{ErrorConverter, Response},
};

fn authentication_response<Converter, Op, B>(
    mapped_error: Op::Error,
    request: &Request<B>,
) -> Response
where
    Op: OperationShape,
    Converter: ErrorConverter<Op::Error>,
{
    <Converter as ErrorConverter<Op::Error>>::convert(mapped_error, request)
}
```

Convert framework failures through the same marker:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
use sdk::server::{
    http::Request,
    response::{ErrorConverter, Response},
    runtime_error::InternalFailureException,
};

fn internal_failure_response<Converter, B>(request: &Request<B>) -> Response
where
    Converter: ErrorConverter<InternalFailureException>,
{
    <Converter as ErrorConverter<InternalFailureException>>::convert(
        InternalFailureException,
        request,
    )
}
```

The generated service selects the converter:

| Service mode | `ServiceSerde::SerdeKind` | Conversion in this checkout |
| --- | --- | --- |
| Generated serde | `GeneratedSerde<P>` | Calls the error's `into_response()`. |
| Schema serde | `SchemaSerde<P>` | Reads `SelectedOperation` and calls the error's `into_response_with_protocol(...)`. |

This works for single-protocol and multi-protocol schema services. The current
converters both require the error's `IntoResponse<P>` implementation; generated
operation errors, framework failures, and `Infallible` satisfy their bounds.
A handwritten modeled error also satisfies the converter contract when it
implements `IntoResponse<P>`; alternatively, a `ModeledRejection<E>` value
supplies that conversion for an `E: HttpModeledError`.

Compilation alone does not establish correct framing. Old middleware bounds can
compile against schema-generated errors while a direct `into_response()` call
still produces the empty fallback. Verify actual HTTP responses.

Schema-only middleware can instead require `Op::Error: HttpModeledError` and
call the selected protocol's `serialize_error(&mapped_error)` directly.
Framework failures use `serialize_runtime_error`. The service converter is the
shared API when one middleware implementation supports both serde modes.

`ServiceSerde` exists in the updated HTTP 1.x runtime. Libraries also supporting
the HTTP 0.x runtime or older generators need to retain their implementation for
those older APIs.

## 8. Preserve routing context when middleware handles requests

Per-operation HTTP middleware can read the selection:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# use sdk::server::http;
use sdk::server::schema::SelectedOperation;
# fn inspect_request<B>(request: &http::Request<B>) {
if let Some(selected) = SelectedOperation::get_from_request(request) {
    let protocol = selected.protocol();
    let operation = selected.operation();
    // Inspect metadata or serialize with this protocol.
}
# }
```

Extractors read the same selection through:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# use sdk::server::http;
use sdk::server::schema::SelectedOperation;
# let (parts, _) = http::Request::new(()).into_parts();
let selected = SelectedOperation::get_from_extensions(&parts.extensions);
```

`SharedServerProtocol::get_from_request(&request)` remains a convenience accessor
when only the protocol is needed. Clone the protocol handle before consuming
the request or moving it into an asynchronous task.

Preserve request extensions when forwarding or rebuilding requests. For example,
auth middleware retains the original parts after body collection:

```rust
# extern crate pokemon_service_server_schema_sdk as sdk;
# extern crate bytes;
# extern crate http_body_util;
use sdk::server::{
    body::RequestBody,
    error::Error,
    http::Request,
};
use http_body_util::BodyExt;

async fn collect_request(request: Request<RequestBody>) -> Result<Request<bytes::Bytes>, Error> {
    let (parts, body) = request.into_parts();
    let collected_body = body.collect().await?.to_bytes();
    Ok(Request::from_parts(parts, collected_body))
}
```

On a collection failure, rebuild a request from the original parts before calling
the converter, even if its replacement body is `()`. The extensions remain
available to frame the failure.

Middleware surrounding the entire router runs before selection, so its accessor
can return `None`. Routing rejections do not reach per-operation plugins.
Missing selection inside a schema operation's middleware indicates a wiring
problem: the schema converter logs and returns an empty `500` with a runtime-error
extension. Guessing a protocol from the tuple loses the actual selection.

The authoritative selection lives in a private extension carrier.
`SelectedOperation` is a read-only view: direct
`extensions.get::<SelectedOperation>()` does not access the carrier, and inserting
a public `SelectedOperation` does not replace the routed context. Use the
accessors above.

Clearing extensions removes the selection. Schema upgrades and missing-handler
fallbacks then return an empty `500` before polling the body or invoking the
handler. A body-routed protocol may already have buffered the request before
middleware runs.

## 9. Review streaming handlers and compatibility settings

### RPC v2 CBOR streaming blobs

For RPC v2 CBOR streaming blob inputs, schema serde exposes `ByteStream` where
generated serde previously exposed `Blob`. Replace direct blob access with stream
collection, mapping collection failures into your operation error. This helper
accepts your application's error mapper and preserves its error type:

```rust
# extern crate aws_smithy_types;
# extern crate bytes;
use aws_smithy_types::byte_stream::{ByteStream, error::Error};
# struct UploadInput { data: ByteStream }

async fn collect_upload<E>(
    input: UploadInput,
    map_stream_error: impl FnOnce(Error) -> E,
) -> Result<bytes::Bytes, E> {
    let bytes = input.data
        .collect()
        .await
        .map_err(map_stream_error)?
        .into_bytes();
    Ok(bytes)
}
```

CBOR input is still buffered and decoded before the handler runs, under the
configured body limit and timeout. The `ByteStream` contains decoded blob bytes,
including when other fields follow the blob; it does not contain CBOR framing.
An absent nullable CBOR streaming blob becomes an empty stream.

REST streaming uploads continue passing the live body to the handler after HTTP
metadata validation. Extractor rejections in either schema upgrade path skip the
handler and do not poll the live body.

### Buffered body limits

Put body limits alongside `"codegen"` in the server plugin configuration:

```json
{
  "customizationConfig": {
    "protocols": {
      "common": {
        "request": {
          "body": {
            "maxBytes": 1048576
          }
        }
      }
    }
  }
}
```

This replaces the deprecated `codegen.requestBodyMaxBytes` setting. If both are
present, the nested setting takes precedence. An explicit zero disables the byte
limit; omission retains the runtime configuration's default.

### AWS JSON operation targets

Check operation targets during the runtime upgrade. An operation modeled as
`getFoo` routes as `Service.getFoo`. To retain acceptance of `Service.GetFoo`,
enable `capitalizeRoutes` in the corresponding AWS JSON protocol settings:

```json
{
  "customizationConfig": {
    "protocols": {
      "aws.protocols#awsJson1_1": {
        "capitalizeRoutes": true
      }
    }
  }
}
```

Use `aws.protocols#awsJson1_0` for AWS JSON 1.0. The setting adds the capitalized
alias; the modeled operation name remains accepted. Preserve your existing
protocol compatibility settings when comparing generated and schema serde.

## 10. Register custom protocols and verify actual HTTP behavior

A custom or internal protocol needs a schema runtime implementation and codegen
registration. Its existing generated-serde implementation alone does not provide
schema routing support. The schema implementation supplies routing and
serialization, including modeled-error and runtime-error framing.

The service model declares the protocol trait, and codegen recognizes that trait.
Its decorator contributes the runtime `ProtocolRegistry` through
`ServerCodegenDecorator.protocolRegistries`. The runtime validates protocol
registrations, ordering, settings, and bindings during service construction.
See [Adding a protocol registry](../rfcs/rfc0048_server_protocol_claims.md#adding-a-protocol-registry)
for runtime and decorator examples.

This applies to protocols such as Coral. Internal package tests establish source
compatibility with the extended response trait; serving a particular custom
protocol through schema routing requires its own end-to-end verification.
The auth middleware checks established concrete SDK builder compatibility and
its local tests; they did not execute authentication against an external service.

Before rollout, exercise every supported protocol against the regenerated service:

- Successful operations and modeled errors.
- Authentication failures and extractor rejections, including direct modeled
  errors and `ModeledRejection`.
- Required and optional extensions.
- Streaming operations and body-collection failures.
- Status, headers, exact body bytes, and instrumentation extensions.
- Alternating protocols across requests to detect stale selection.

Compare existing protocols against a generated-serde baseline with the same
model, inputs, and configuration. Preserve established wire behavior and
compatibility settings.

| Case | What to verify |
| --- | --- |
| Existing custom response | Status, headers, body, and extensions stay unchanged through the contextual default. |
| Shared runtime rejection | Each variant matches its established protocol-local framing. |
| Direct modeled extractor rejection | A generated or handwritten error's contextual conversion reaches the selected serializer through ordinary and streaming upgrades. |
| Wrapped modeled extractor rejection | `ModeledRejection` reaches the selected serializer through both schema upgrades without a handwritten response conversion. |
| Multiple extractors | Tuple rejection wrappers forward contextual conversion from each failing position. |
| Required and optional extensions | Required absence retains the empty `500`; optional absence reaches the handler with `None`. |
| Rejected extraction | The handler is skipped; a live body is not polled by the upgrade. |
| Direct static conversion | Shared runtime errors, direct modeled errors, and modeled wrappers have their static response behavior checked separately from protocol framing. |
| Custom protocol | Its serializers are invoked through routing and a generated operation's upgrade. |

The runtime regression tests cover the modeled wrapper's static fallback and
JSON/CBOR contextual conversion through both schema upgrade paths. The
[response-conversion compatibility decision](../rfcs/rfc0048_server_protocol_claims.md#response-conversion-and-source-compatibility)
explains why the existing `FromParts` bound and `IntoResponse` trait are retained.
