# Checkpoint 4 — Step 4 (codegen) walkthrough material

Prepared 2026-08-24 for the review session (plan principle 6: nothing is done
until walked through together). Commits under review, in order:

| Commit | Content |
|---|---|
| `93d690455` | Phase 1: protocol-free ValidationException conversion (2d), model-order deletion (2e) |
| `49d24c172` | Phases 2–3: full schema closure, deserialize walker, serving flip (items 1–4, 6, 7) |
| `48fc38549` | Protocol-test parity: JSON codec strictness + runtime empty-body/Accept/payload rules |
| `fd5ce1a00` | Item 8a: streaming-blob splice glue |
| `b7e9a0483` | Item 8b: event-stream `Marshaller<P>`/`Unmarshaller<P>` — Step 4 complete |

Standing evidence at HEAD: all 33 generated http-1.x crates green under
`cargo test` — including the full Smithy protocol suites running through the
schema pipeline (rest_json **750/750**, json_rpc11 100/100, rpcv2Cbor 60/60,
json_rpc10 44/44, constraints, pokemon) and their legacy counterparts; 37 wire
captures; 10 error goldens; 2×21 event-stream integration tests; runtime unit
suites (`aws-smithy-http-server` 142, `aws-smithy-json` 217); clippy clean.

---

## 1. What generated code looks like now (pokemon crate pair)

### Request path (`protocol_rest_json1/operations.rs`, schema crate)

An ordinary operation's `FromRequest` is a thin body-collect plus ONE runtime
call — no transport interpretation in generated code (2g Design B):

```rust
fn from_request(request: ::http_1x::Request<B>) -> Self::Future {
    let fut = async move {
        let (parts, body) = request.into_parts();
        let bytes = { use ::http_body_util::BodyExt; body.collect().await?.to_bytes() };
        <RestJson1 as ServerProtocol>::deserialize_request::<crate::input::GetPokemonSpeciesInput>(
            crate::input::GetPokemonSpeciesInput::SCHEMA,
            crate::output::GetPokemonSpeciesOutput::SCHEMA,   // Accept validation target
            &parts,
            bytes.as_ref(),
        )
    };
    ...
}
```

The legacy counterpart was ~120 lines per operation of generated URI/nom
parsing, query loops, header glue, and body-parser dispatch, plus one
`de_*`/`ser_*` function per shape per protocol in `protocol_serde`.

### Response path

One generic impl per output and per error enum (2c), rendered once, identical
on single- and multi-protocol crates:

```rust
impl<P: ServerProtocol> IntoResponse<P> for crate::output::GetPokemonSpeciesOutput {
    fn into_response(self) -> Response {
        P::serialize_response(Self::SCHEMA, &self)
    }
}
impl<P: ServerProtocol> IntoResponse<P> for crate::error::GetPokemonSpeciesError {
    fn into_response(self) -> Response {
        let mut response = match &self {
            Self::ResourceNotFoundException(e) => P::serialize_error(e),
            Self::ValidationException(e) => P::serialize_error(e),
        };
        response.extensions_mut().insert(ModeledErrorExtension::new(self.name()));
        response
    }
}
```

### The walker (`schema_serde/shape_*.rs`)

One uniform deserialize fn per struct/union, indistinguishable from any nested
struct's; feeds the pre-existing `pub(crate) set_*` builder setters (the
builder's unconstrained ingestion surface — principle 3, validation unmoved):

```rust
pub(crate) fn deser_get_storage_input(
    deserializer: &mut dyn ShapeDeserializer,
) -> Result<crate::input::get_storage_input::Builder, SerdeError> {
    let mut builder = crate::input::get_storage_input::Builder::default();
    deserializer.read_struct(&GETSTORAGEINPUT_SCHEMA, &mut |member, deser| {
        match member.member_index() {
            Some(0) => { if deser.is_null() { deser.read_null()?; } else {
                builder = ::std::mem::take(&mut builder).set_user(deser.read_string(member)?);
            } }
            Some(1) => { /* passcode, likewise */ }
            _ => {}
        }
        Ok(())
    })?;
    Ok(builder)
}

impl DeserializableShape for crate::input::GetStorageInput {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        let builder = deser_get_storage_input(deserializer)?;
        builder.build().map_err(Into::into)   // → From<ConstraintViolation> for DeserializeError
    }
}
```

Parse symbols mirror the legacy parsers exactly (`returnSymbolToParseFn`):
nested structs → their Builder, aggregates → their `XxxUnconstrained` wrapper,
enums/constrained strings → plain `String`. Unions have NO `Unknown` arm and
reject mixed variants with the frozen legacy message.

### Event streams (`schema_serde/shape_attempt_capturing_pokemon_event.rs` etc.)

Generic frame serde per stream union carrying only model facts:

```rust
impl<P: EventStreamProtocol> MarshallMessage for CapturePokemonEventsMarshaller<P> {
    type Input = crate::model::CapturePokemonEvents;
    fn marshall(&self, input: Self::Input) -> Result<Message, Error> {
        match input {
            Self::Input::Event(inner) => event_bindings::marshall_event(
                P::codec(), "event", ":event-type", "event",
                P::EVENT_PAYLOAD_CONTENT_TYPE,          // fixes the client's baked-literal leak
                crate::model::Event::SCHEMA, &inner,
            ),
        }
    }
}
```

`@eventHeader`/`@eventPayload`/body-member routing is interpreted at runtime in
`aws_smithy_http_server::protocol::event_bindings` off the event structure's
schema. The unmarshaller drives the event structures' walkers through the
frame composite; unknown `:event-type` is an error; client-sent modeled stream
errors unmarshal into the stream error enum; constrained events `build()` at
unmarshal time (legacy could not compile them — register A1).

Operation glue stays `impl<P>` (2a Option B): the input glue wraps the body in
a receiver typed as the MEMBER's resolved symbol (decorator wrappers like the
SigV4 unsigning receiver keep working), unframes the initial-request on
`P::FRAMES_INITIAL_MESSAGES` when non-stream members exist, walks the prelude
into the BUILDER via `P::with_request_deserializer`, attaches the receiver,
then `build()`s.

---

## 2. Traits and types added/changed (name · crate · file · role)

**Runtime (`aws-smithy-http-server`)** — Step-3 surface plus Step-4 additions:

- `ServerProtocol` · `protocol/server_protocol.rs` — codec + three verbs.
  Step-4 deltas: `deserialize_request` gained the OUTPUT schema (legacy Accept
  check validates against the response content type, payload-`@mediaType` and
  event-stream aware) and became a provided method over the NEW
  `with_request_deserializer` (callback seam handing the composite deserializer
  to glue — event ops walk into the builder). Bounds: `Codec + 'static`,
  `Self: 'static` (generic marshaller types demand it).
- `protocol/event_bindings.rs` (NEW) — `marshall_event`, `initial_message`,
  `EventFrameDeserializer`: frame-level binding interpretation off event
  structure schemas. The frame counterpart of the HTTP composites.
- `deserialize.rs` — `DeserializableShape`/`DeserializeError` (Step 3),
  now implemented by every generated operation input.
- Rules hardened for protocol-test parity (all pinned by now-running tests):
  no-user-modeled-output responses are empty-bodied without codec invocation
  (`schema.original_name()` is the modeled-ness signal; awsJson keeps its
  content-type, rpcv2Cbor keeps only `smithy-protocol`, REST sends neither);
  RPC inputs with no members never parse or content-type-check the body;
  empty bodies leave raw blob/string payload members unset; `@httpPayload`
  documents serialize the value, not a member-keyed fragment; header-bound
  list elements are RFC-9110-quoted.

**JSON codec (`aws-smithy-json`)** — server-grade strictness:
element-separator discipline in every container loop (trailing commas
rejected), trailing-garbage rejection after the root document, float/double
string forms limited to `NaN`/`Infinity`/`-Infinity`, unknown union variant
keys rejected (`__type` exempt), and `JsonCodecSettings::strict_timestamp_format`
(server codecs only): resolved `@timestampFormat` is enforced, not coerced.

**codegen-server**:

- `ServerSchemaDeserializerGenerator` (NEW) — the 2g walker generator.
- `ServerSchemaEventStreamGenerator` (NEW) — `Marshaller<P>`/`ErrorMarshaller<P>`/
  `Unmarshaller<P>` per stream union.
- `ServerSchemaDecorator` — full-operation-closure computation (flag-on), error
  closure on EVERY http-1.x crate (the 2d seam needs `HttpModeledError` on
  validation shapes even on legacy-serving crates), `schemaSupportedOperations`
  = all operations (no exclusions remain).
- `ServerSchemaGenerator` — `.with_http` now on outputs too (status
  resolution); constrained-newtype unwrapping in every serialize position
  (`.0` for aggregate/number/blob wrappers, `as_str()` for strings — the
  legacy serializers' access patterns); `serializeMemberOrder` DELETED (2e).
- `ServerHttpBoundProtocolGenerator` — schema-served branch: thin
  `FromRequest`, generic `IntoResponse`, streaming-blob splice glue,
  event-stream glue, A2 content-type override on event operations' error
  enums; `operationServedBySchema` = flag + http-1.x (no `!isMultiProtocol`,
  no per-protocol conditions, no closure carve-outs).
- `ValidationExceptionConversionGenerator` — the interface method is now the
  protocol-free `renderImplFromConstraintViolationForValidationException`
  (+ `validationExceptionShapeId()`; the user-provided decorator's `shapeId`
  is a sentinel). `ServerBuilderGenerator` renders it once per input builder
  plus trivial per-rejection boxed delegations (http-1.x) or the pre-serialized
  legacy form (frozen http-0.x fork).
- `ServerProtocolTestGenerator` — response tests name the marker explicitly
  (`IntoResponse::<Marker>` — the generic impls made inference ambiguous);
  `PassingOnSchemaServedCrates` moves fixed-by-schema tests out of ExpectFail
  on flag-on crates.

---

## 3. Everything deleted

- `protocol_serde` generation for flag-on crates: not generated (the schema
  path references no legacy serde fn, and legacy fns only generate on demand).
  Clean-regen proof: `pokemon-service-server-sdk-schema/src` contains NO
  `protocol_serde`/`event_stream_serde`; `protocol_rest_json1.rs` declares
  exactly `mod operations;`.
- `serializeMemberOrder` / `errorSerializeOrder` / `isRestFamilyProtocol`
  (protocol knowledge baked into codegen — 2e).
- The per-marker schema error-serving branch and the multi-protocol raw-path
  branch (`renderProtocolValidationConversions`) with its `!isMultiProtocol`
  gates.
- The constrained-newtype schema exclusions (`unsafeForSchemaSerialization`)
  — superseded by newtype unwrapping (goes BEYOND the plan's "exclusions stay"
  non-goal; flag-on crates would otherwise keep legacy paths for every set).
- Legacy `verifyAcceptHeader`/content-type generation for served ops (runtime
  owns those checks now).

Kept deliberately: `serverValidationExceptionErrorSerializer` — used only by
the frozen http-0.x fork's pre-serialized rejection form.

---

## 4. Divergence register (2f additions; each pinned)

1. restJson1 error-body member order is canonical MODEL order; the two
   multi-member wire-capture goldens compare parse-equal (2e policy), all else
   byte-exact. RPC protocols byte-exact as before.
2. `RestJsonHttpPayloadWithStructureAndEmptyResponseBody`: schema path FIXES
   the legacy bug; the test runs as a normal (passing) test on flag-on crates.
3. Event-stream initial-request receive failures → the protocol's
   malformed-request rejection (`SchemaDeserialize`) instead of the legacy
   raw-string-in-validation-body hack (http-1.x only).
4. Client-sent modeled stream errors on CBOR/awsJson event streams marshal
   without a `__type` member (restJson1 parity; legacy cbor injected it via
   `AddTypeFieldToServerErrorsCborCustomization`). To pin at Step-5 frame
   goldens.
5. x-amzn-errortype carries the actual custom validation shape name (2f,
   confirmed bug in legacy; already decided).

## 5. Known watch items for Step 5

- Frame-level event goldens (legacy vs schema crate pair) are the missing
  verification for item 8 — the current evidence is compile + suites + the
  legacy eventstream tests staying green, not byte comparisons of schema-crate
  frames.
- Request round-trip goldens across binding locations (plan Step 5) not yet
  built; the schema request path is currently exercised by the 900+ protocol
  request tests and the constraint-violation wire captures.
- REST event outputs with non-stream members would leak those members into the
  (discarded) HTTP body serialization and the initial-response payload where
  legacy sent `{}` — no such shape exists in the test models; note for gates.
- Projection build dirs accumulate orphan files across regens; regenerate into
  clean trees before eyeballing generated output.
