# Plan — Schema-Decoupled Server, Phase 1

This document supersedes `specs/rfc_schema_decoupled_server.md` as the working plan.
The RFC remains as background reading; where they disagree, this plan wins. Scope is
Phase 1 ONLY — nothing here depends on documents-at-request-time, registries, or
runtime model loading.

## Goal

Generated server crates carry **zero protocol knowledge** — request and response.
For a flag-on crate there is **no `protocol_serde` module at all**: input
deserialization, output serialization, and error serialization are all
schema-driven through the runtime `ServerProtocol`. **All five server protocols
are targets** (restJson1, rpcv2Cbor, awsJson 1.0, awsJson 1.1, restXml — user
directive: none left behind); restJson1 + rpcv2Cbor lead the implementation
order. Multiprotocol is an assembly-time concern only.

## Binding principles (violating any of these is a design bug, even if bytes match)

1. **Generated types and their serde code are 100% protocol-free.** All protocol
   knowledge — discriminators, error headers, status placement, member order,
   content types, URI/query/header parsing, HTTP itself — lives in the runtime
   `ServerProtocol`/codec.
2. **Model metadata is data, not code.** `@http`, `@httpHeader`, `@httpLabel`,
   `@httpQuery`, `@httpPayload`, `@httpResponseCode`, `@httpPrefixHeaders`,
   `@jsonName`, `@timestampFormat` are transcribed into schema statics by codegen
   and interpreted at runtime. Generated walkers pair fields with member schemas
   and know nothing else.
3. **Validation does not move.** The schema-driven deserializer feeds the existing
   constrained builders; `build()` / constrained-newtype `TryFrom` keep doing
   constraint enforcement, producing today's `ConstraintViolation` values with the
   frozen message text. Rejections carry the modeled error value
   (`Box<dyn HttpModeledError + Send>`), serialized once at the protocol boundary.
4. **Single-protocol and multi-protocol services generate identical code.** A
   multiprotocol service only additionally attaches the selection router at
   assembly time. No per-protocol generated serde, no per-marker impls.
5. **The client is the reference architecture.** The schema-allowlisted pokemon
   client SDK is kept generated and in view at all times; the server mirrors its
   architecture (statics + walkers + runtime protocol + thin glue), diverging only
   where server semantics demand (builders instead of error correction, unknown
   union variants rejected, constraint failures → 400).
6. **Nothing is "done" until walked through together.** Every milestone ends in a
   review: which traits were added/changed and where, what the generated code
   looks like (before/after), what legacy code was deleted. No "it works" claims
   without showing the code and the test evidence.

## Non-goals (Phase 1)

- (REMOVED 2026-08-23, user directive "target all protocols — none to be
  left": ALL FIVE server protocols — restJson1, rpcv2Cbor, awsJson 1.0,
  awsJson 1.1, restXml — are verification targets with full gates. The
  restXml freeze-or-fix decision is thereby CLOSED as **fix-forward**: legacy
  restXml server error bodies are broken (register B4/B6), so the restXml
  error path is gated by its own pinned goldens as a recorded divergence,
  while restXml outputs/inputs get normal legacy-compare gates.)
- **No dynamic machinery**: documents at request time, registries, runtime model
  loading, schema-read status resolution (statuses stay codegen-baked literals).
- **Validation engine redesign** (checkers, presence tracking, two-pass builders)
  — out. Principle 3 is the boundary.
- **Constrained-shape schema exclusions** stay as-is where they exist; revisit
  with the validation engine.
- Benches run at the end, after review; never before.

## DECIDED (user sign-off 2026-08-23) — event streams and streaming payloads: full schema path, no carve-out

Grounded in the clean single-protocol client drill (see Step 1 snapshots): the
schema-mode client generates ZERO per-shape protocol_serde for either protocol;
the event-stream unmarshaller is byte-identical across restJson1 and rpcv2Cbor;
frame payloads go through `protocol.payload_codec()` + the schema walkers on
both directions. The server mirrors this, cleaned up:

- **Event streams**: generated `Marshaller<P>` / `Unmarshaller<P>` per stream
  union, generic over the protocol, containing only model facts (`:event-type`
  strings, `@eventHeader` names, blob-vs-struct payload shape). Payloads via
  `P::codec()`; `:content-type` via `P::EVENT_PAYLOAD_CONTENT_TYPE` (fixes the
  client's baked-literal leak). Server semantics: no `Unknown` arms — unknown
  `:event-type` on unmarshal is an error. Op-level glue (specialized generated
  impls, still `impl<P>`): response = prelude via schema + marshalled frame
  stream body with the eventstream HTTP content-type; pre-first-event HTTP
  errors keep the A2 quirk (pinned by existing golden). Input side: REST reads
  prelude members from URI/headers; RPC unframes the initial-request message
  (`P::FRAMES_INITIAL_MESSAGES`); then attaches the receiver with
  `Unmarshaller<P>`.
- **Streaming blobs**: no new trait member. Generated glue calls
  `P::serialize_response` with the payload member skipped, splices the raw
  `ByteStream` as the body, fixes content-type from `media_type()` (schema
  data) — mirror of the client's splice on both directions.

---

## Step 0 — Housekeeping: revert the drift

Execute the REVERT list from `specs/handoff.md` ("Uncommitted working-tree
inventory") — the half-finished multi-protocol lift (order views, `renderScoped`,
`ServerBuilderGenerator` renderer param, lifted guard). Keep the KEEP list (the
green opt-in flip state). Commit the clean state so every later diff is legible.

**Checkpoint 0**: `git status` clean, wire-capture 37 + 10 goldens green, short
summary of what was reverted.

## Step 1 — Client reference SDK — DONE 2026-08-23 (drill executed with user)

Executed: `com.aws.example#PokemonService` allowlisted in
`SchemaSerdeAllowlist.allowedServices` (`SchemaDecorator.kt`, marked TEMPORARY);
two clean single-protocol snapshots generated (model temporarily flipped to one
protocol each, restored to `@restJson1 @restXml @rpcv2Cbor` after) and kept as
the standing reference:

- `smithy-rs-example-sdk/pokemon-client-restjson1/`
- `smithy-rs-example-sdk/pokemon-client-rpcv2cbor/`

(Caution: the projection build dir accumulates orphans across regens — the
snapshots were orphan-filtered against `lib.rs` references; regenerate into a
clean tree if refreshing them. A stale `D:\smithy-rs-example-sdk\` folder
outside the repo should be deleted by hand.)

Measured findings (the mirror map):
- Zero per-shape `protocol_serde` in either crate. Schemas, types, walkers, and
  the event-stream unmarshaller are byte-identical across protocols.
- Complete per-protocol difference inventory: config default protocol object;
  `json_errors.rs` vs `cbor_errors.rs` (client error dispatch — N/A server);
  glue-inserted `smithy-protocol`/`accept` headers (leak; server keeps these
  runtime-side); restJson1 bodyless-GET fixup in glue; event marshaller
  `:content-type` baked literal (leak; fixed by `EVENT_PAYLOAD_CONTENT_TYPE`);
  `initial_message_from_body` CBOR-only; `into_builder` helper (harmless).
- Client patterns the server must NOT copy: `deserialize()` error correction,
  union `Unknown` arms, header/status binding reads in generated code
  (client puts transport reads in `deserialize_with_response` glue — server
  policy decided at 2g), the two leaks above.
- `ClientProtocolInner` mirror table: `serialize_request`↔`deserialize_request`,
  `deserialize_response`↔`serialize_response`,
  `deserialize_error_response`↔`serialize_error`, `payload_codec()`↔`Self::Codec`
  (static dispatch), `parse_error_metadata`/`update_endpoint`↔not needed.

## Step 2 — Design decisions to lock (short reviews; no implementation until each is agreed)

**2a. `ServerProtocol` — DECIDED (user sign-off 2026-08-23). Base trait:
codec + three verbs; event-stream capability as a separate subtrait with three
consts (Option B + content-type correction, see below). Associated functions,
infallible response surface:**

```rust
pub trait ServerProtocol: ProtocolShape {
    /// Body codec. Also the event-stream frame-payload codec — the client
    /// needs a dyn `payload_codec()` accessor because its protocol is a
    /// runtime value; server dispatch is static, so `Self::Codec` serves both.
    type Codec: Codec;
    fn codec() -> &'static Self::Codec;

    /// Request path. Reads @http bindings off INPUT_SCHEMA (labels from the
    /// matched URI, query, headers, body via codec), feeding a generated
    /// server-side deserialize target. Distinguishes malformed-request
    /// failures (protocol 4xx) from constraint violations (ValidationException).
    fn deserialize_request<T: DeserializableShape>(
        schema: &Schema<'_>,
        request: /* parts + collected body; exact shape at Checkpoint 3 */,
    ) -> Result<T, /* rejection, see 2d */>;

    /// Success path. Status: @httpResponseCode member if bound and set, else
    /// schema.http().code(), else 200. REST protocols honor response bindings
    /// read off member schemas; RPC protocols serialize body-only.
    /// Serialization failure falls back to the protocol's internal-error
    /// response (legacy IntoResponse contract).
    fn serialize_response(
        schema: &Schema<'_>,
        output: &dyn SerializableStruct,
    ) -> http::Response<BoxBody>;

    /// Error path. Status from error.status_code(), discriminator framing,
    /// header-bound members split (REST). Same internal fallback.
    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody>;

}

/// DECIDED (user sign-off 2026-08-23, Option B of
/// specs/eventstream-capability-options.md — kept as the decision record):
/// event-stream capability is a SUBTRAIT, implemented only by protocols whose
/// Smithy definition declares `eventStreamHttp`. Misuse (wiring an
/// event-stream op to a non-supporting protocol) is a compile error at
/// assembly, not a runtime failure. Frame glue and event-stream op impls
/// bound on `P: EventStreamProtocol`; ordinary ops stay `P: ServerProtocol`.
/// Bounds never reach user-facing signatures (concrete-marker instantiation).
pub trait EventStreamProtocol: ServerProtocol {
    /// Frame-level `:content-type` (json: "application/json", cbor:
    /// "application/cbor"). Fixes the client's baked-literal leak.
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str;
    /// HTTP-level Content-Type of the streaming response. NOT uniform (user
    /// correction, verified): restJson1/restXml/rpcv2Cbor declare
    /// "application/vnd.amazon.eventstream"; awsJson keeps
    /// "application/x-amz-json-1.x" (`AwsJson.kt:93` — response = request
    /// content type unconditionally, no eventStreamContentType override).
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str;
    /// RPC protocols frame initial-request/initial-response messages; REST
    /// puts the prelude in HTTP and the body is frames-only. See the
    /// initial-message facts below.
    const FRAMES_INITIAL_MESSAGES: bool;
}
```

**Initial-message facts (confirmed 2026-08-23 against git history + code —
treat as ground truth):**
- Server support added by Russell Cohen: #4344 (initial-request, 2025-10),
  #4352 (initial-response, 2025-10), sigv4 fixes #4400/#4431; partial-revert
  arc ended by #4734 (2026-07): **initial messages are RPC-protocols-only**
  (awsJson 1.0/1.1 + rpcv2Cbor implement `handlesEventStreamInitial*`; every
  REST resolver returns false — `HttpBindingResolver.kt:228/269`).
- Per-op conditions (generated-glue logic, NOT on the const): initial-request
  consumed iff input has a stream member plus ≥1 non-stream DOCUMENT member;
  initial-response emission additionally gated by the codegen setting
  `alwaysSendEventStreamInitialResponse` (default false,
  `ServerRustSettings.kt:151`). No const split needed — both directions gate
  on RPC-ness at the protocol level.
- Server tests exist and pin this:
  `codegen-server-test/integration-tests/eventstreams/tests/structured_eventstream_tests.rs`
  (`test_streaming_operation_with_initial_data[_missing]`,
  `test_server_sends_initial_response`,
  `test_server_no_initial_response_when_disabled`).
- Pokemon demonstrates initial-request with NO model change: `CapturePokemon`
  input = `events` + `region`, service carries `@rpcv2Cbor`, and the example
  client resolves rpcv2Cbor by priority (`ClientProtocolLoader.kt:42`) —
  `examples/pokemon-service/tests/event_streaming.rs` already sends `region`
  in an initial-request frame. PERMANENT change made (user-directed): both
  pokemon server projections (`pokemon-service-server-sdk` and its `-schema`
  pair) now set `"alwaysSendEventStreamInitialResponse": true` in
  `codegen-server-test/build.gradle.kts` so the response direction is
  demonstrated too (crate-pair goldens require both sides to share the
  setting).

Settled within this: no `is_error` flag (discriminator wrapping is internal to
each impl's error path); `serialize_error` takes `&dyn` (rejections hand us a
box; per-type monomorphization bought nothing); associated functions — protocols
are types, no instance mechanism needed (per-service protocol facts like
`@xmlNamespace` ride on schemas, not on protocol values); no `payload_codec()`
method and no client-only members (`parse_error_metadata`, `update_endpoint`).

**2b. Response binding interpretation (runtime) — deferred to Checkpoint 3
(user, 2026-08-23): design reviewed with the code in hand.** Generalize `HeaderSplitter`
into a response-binding splitter handling, per member schema: `@httpHeader`
(exists), `@httpPrefixHeaders`, `@httpResponseCode` (captured, not written to
body), `@httpPayload` (the body IS that member: struct → codec doc, string →
`text/plain`, blob → `media_type()` or `application/octet-stream`). Non-REST
protocols ignore bindings wholesale.

**2c. Generic `IntoResponse` — settled.** One generated impl per output and per
operation error enum: `impl<P: ServerProtocol> IntoResponse<P> for {Op}Output`
delegating to `P::serialize_response(SCHEMA, &self)`; error enums variant-match
into `P::serialize_error(e)`. No instance mechanism (associated fns). The trait
stays public (framework bound in `Upgrade`, user surface, carve-out hatch).

**2d. ValidationException / rejection seam — DECIDED (user sign-off
2026-08-23: `Box<dyn HttpModeledError + Send>`, replacing the legacy
pre-serialized `String`/`Vec<u8>` bytes).** ValidationException is an ordinary
modeled error whose constructor is the framework:

- The shape (default or decorator-custom) gets schema + `SerializableStruct` +
  `HttpModeledError` like every error shape. Nothing else.
- One generated **protocol-free** `From<ConstraintViolation> for {ValidationShape}`
  building the value (message, fieldList — frozen format strings). The three
  validation decorators customize THIS conversion only — never serialization.
- `RequestRejection::ConstraintViolation` carries
  `Box<dyn HttpModeledError + Send>` instead of pre-serialized per-codec bytes
  (today: `String` on restJson1, `Vec<u8>` on CBOR — the root of every
  per-protocol validation artifact). Serialization happens once, at the protocol
  boundary, via the rejection's `IntoResponse<P>` → `P::serialize_error(&*err)`.
- Deletes: per-protocol serializer calls in the generated `From` impls,
  `serverValidationExceptionErrorSerializer` (`ServerProtocol.kt`), the
  RuntimeType materialization workaround, the multi-protocol raw-path branch.
- The other `RequestRejection` variants (deser failures, content-type, URI) are
  genuine per-protocol framework errors and stay this phase.

**2e. Member order policy — DECIDED (user sign-off 2026-08-23).** Canonical
**model order** everywhere in generated walkers; `serializeMemberOrder` is deleted
(it baked the REST binding-resolver name-sort — protocol knowledge — into codegen).
Gate policy: rpcv2Cbor and awsJson stay **byte-exact** (legacy already uses model
order). restJson1 bodies relax to **parse-equal at the top level only** (JSON
member order is insignificant per spec; all generated clients parse
order-insensitively); status, headers, nested content, escaping, and number
formatting remain byte-exact. No runtime reordering.

**2f. Divergence register — CONFIRMED (fix-forward, each pinned by a test):**
- restJson1 legacy hard-codes `x-amzn-errortype: ValidationException` even for
  custom validation shapes — confirmed as a bug (user, 2026-08-23); schema path
  emits the actual shape name. Only observable when the custom shape's name
  differs (header is name-only).
- restXml bare-`<Error>` envelope: out of scope; stays legacy this phase.
- Request-side note: which violation fires first on a multi-violation request
  can shift if the schema walker visits members in a different order than legacy
  deser (constrained newtypes fail during parsing). Gate: pin fail-fast behavior
  per protocol; investigate only if goldens actually diverge.

**2g. Server-side deserialization — DECIDED (user sign-off 2026-08-23):
Design B, the runtime composite deserializer.** (Design A — the client's
pattern of transport reads in generated `deserialize_with_*` glue — was
rejected: it re-violates principle 1 and dead-ends the P3 runtime-server path.)

- `P::deserialize_request<T: DeserializableShape>(schema, request)` does ALL
  transport interpretation internally: `@httpLabel` values from the matched URI
  against `schema.http().uri()` (plumb the router's existing pattern match
  through if possible — design at Checkpoint 3; re-match is the correct
  fallback), query parsing, header reads, `@httpPayload` routing, body via
  `Self::Codec`. It presents ONE composite `ShapeDeserializer`: per member
  schema, binding-bound members coerce through the existing
  `HttpStringDeserializer` (`aws-smithy-schema/codec/http_string.rs`), the rest
  delegate to the body deserializer. RPC protocols: body-only + framework
  header checks (`smithy-protocol`, `x-amz-target`). Content-type / Accept
  validation stays runtime, per protocol.
- Generated side is the ONE uniform walker — indistinguishable from any nested
  struct's: `impl DeserializableShape for {Op}Input` walks `read_struct` into
  the existing INTERNAL (unconstrained) builder and calls `build()`. No
  transport parameters in generated code at all.
- **Constraint validation happens on `build()` — unmoved** (principle 3): the
  walker validates nothing; `build_enforcing_all_constraints` + constrained
  `TryFrom`s enforce `@required`/`@length`/`@range`/`@pattern`/`@enum`,
  producing today's `ConstraintViolation` values with frozen messages. No
  error correction, no defaulting. Nested shapes build innermost-first during
  the walk, matching legacy fail-fast ordering; encounter order is wire order
  in both worlds. One watch item for request goldens: relative order of
  binding-bound vs body members is now schema-walk order, not legacy's
  URI→headers→body phase order.
- Two error channels: wire-level failures (bad document, type mismatch,
  unknown union variant, unparseable header) → malformed-request rejection →
  protocol 4xx, as today; `ConstraintViolation` → 2d seam
  (`From<ConstraintViolation>` → `Box<dyn HttpModeledError>` → serialized once
  at the boundary).
- `FromRequest<P>` generated impl body becomes a thin call into
  `P::deserialize_request::<{Op}Input>(INPUT_SCHEMA, ...)`.

**2h. Flip granularity — DECIDED (user sign-off 2026-08-23).** The flag flips
the WHOLE crate (per-protocol serving within a crate is impossible by
construction — one generic `impl<P>` per type, coherence forbids carve-outs,
and that is the point). The mixed-protocol question dissolved under the "all
protocols targeted" directive: every attached protocol of a flag-on crate is
schema-served AND gated — there are no unverified ride-alongs. Any protocol
mix may be flag-on, in tests and for users.

## Step 3 — Runtime implementation (`aws-smithy-http-server`)

In the order agreed in Step 2:

1. Reshape `ServerProtocol` per 2a (three verbs, associated fns, status
   resolution off schemas).
2. Response-binding splitter per 2b.
3. Request-binding reader per 2g (labels/query/headers/payload/body). Order:
   restJson1 + rpcv2Cbor first, then awsJson 1.0/1.1 (body-only, cheapest),
   then restXml (XML codec both directions; error path is the fix-forward
   divergence).
4. Rejection change per 2d: `ConstraintViolation(Box<dyn HttpModeledError + Send>)`
   with serialize-at-boundary `IntoResponse`. **Bound gap (verified, see
   specs/validation-rejection-options.md):** `HttpModeledError` today has no
   `Debug`/`Display`/`Send` supertraits — add them here (Display comes free:
   generated `@error` shapes implement `std::error::Error`); `RequestRejection`
   derives `Debug` and `Upgrade` logs rejections via `Display`, so this is a
   prerequisite of the variant change, not a nicety.

**Checkpoint 3**: walk through every new/changed trait and type — name, file,
signature, why. Unit tests green (`cargo test -p aws-smithy-http-server`,
`-p aws-smithy-schema`, `-p aws-smithy-json`, `-p aws-smithy-cbor`).

## Step 4 — Codegen implementation (`codegen-server`)

1. **Schema closure = full operation closure**: inputs, outputs, errors, and
   everything transitively reachable. `.with_http(...)` attached to input AND
   output schemas (input side exists in core `SchemaGenerator`; output side is
   new — candidate for the upstream ask).
2. **Deserialize walker generation** in `ServerSchemaGenerator` per 2g (server
   semantics; the core client `deserialize()` is the anti-reference: no
   `Self::builder()` assumptions, no error correction, no Unknown arm).
3. **Generic `IntoResponse` impls** per 2c, replacing the per-marker branch in
   `ServerHttpBoundProtocolGenerator`.
4. **`FromRequest` flip** per 2g: generated impl calls `P::deserialize_request`.
5. **ValidationException conversion** per 2d; rewire the three decorators; delete
   the serializer plumbing.
6. **No `protocol_serde` generation for flag-on crates.** Not "unreferenced and
   lazily dropped" — not generated. Grep-proof gate.
7. **Predicate cleanup**: `operationServedBySchema` = flag + http1.x + supported
   closure. Streaming and event-stream ops are IN (schema-served via the decided
   `Marshaller<P>`/`Unmarshaller<P>` + splice designs — specialized generated
   impls, still generic over P). **No `!isMultiProtocol`**, no per-protocol
   conditions in generated code.
8. **Event-stream codegen** per the decided design: `Marshaller<P>` /
   `Unmarshaller<P>` per stream union (strict server semantics, no Unknown
   arms), op-level `impl<P>` glue for prelude + frame stream, streaming-blob
   splice glue.

**Checkpoint 4**: side-by-side generated code review, pokemon server crate:
legacy vs schema projection. Every impl accounted for; grep-proof that flag-on
crates contain zero `protocol_serde`, zero legacy serde fns, zero
protocol-conditional generated code.

## Step 5 — Gates

- **Response goldens (crate-pair), ALL FIVE protocols**: outputs — one case per
  binding kind on the REST protocols (`@http(code)`, `@httpResponseCode`,
  `@httpHeader`, `@httpPrefixHeaders`, `@httpPayload` struct/string/blob+mediaType,
  plain body, empty body), body-only cases on the RPC protocols — plus the
  existing 10 error goldens, per the 2e order policy. restXml ERROR path is the
  one exception: gated by its own pinned goldens (fix-forward divergence, legacy
  is broken per B4/B6); restXml outputs/inputs legacy-compare like the rest.
- **Request goldens (round-trip), ALL FIVE protocols**: same wire request into
  legacy crate and schema crate → deserialized inputs are `==`; rejection cases
  (malformed body, bad content-type, constraint violation) → responses compared
  per 2e policy. Constraint-violation cases across binding locations (label,
  query, header, body member).
- **ValidationException end-to-end**: router-driven, through the real
  `FromRequest` path, legacy vs schema crate, both protocols, custom-shape
  decorator variants included.
- **Event-stream goldens**: frame-level byte comparison legacy vs schema crate
  (event frames, modeled stream errors, initial-request/-response framing on
  CBOR, `:content-type` per protocol, A2 pre-first-event quirk), plus
  streaming-blob splice cases both directions.
- **Multiprotocol assembly test**: multiprotocol crate compiles and serves with
  the single generic impls; diff of generated code vs single-protocol crate
  shows the selection layer as the only difference.
- Existing 37 wire captures stay green (re-pointed at the schema crate's full
  pipeline). `:codegen-server:test` full clean run (nothing else running).
- Benches last (crate-pair harness exists): criterion + dhat, results recorded in
  `specs/bench-results-error-serde.md`. Beyond the existing error-serde benches,
  add a **request-path bench pair**: legacy's per-operation compile-time
  specialization (generated nom URI parsers monomorphized against each
  `@http` template, per-op query-match loops, per-member header glue) vs the
  schema path's single generic runtime interpreter
  (`request_bindings::extract_labels` / `parse_query_pairs` / composite
  deserializer, including the label re-match the router's capture-less
  `Match` forces). Direction is an open empirical question — the interpreter
  pays per-request template walking, but legacy pays nom combinator overhead
  and per-op code bloat (icache) — so measure, don't assume: throughput
  (criterion) + allocations (dhat) on a label+query+header+body operation and
  a body-only RPC operation, both directions. Response-path pair likewise
  (splitter vs generated ser fns). Record verdicts per protocol; a regression
  is a finding to weigh at the Step 6 walkthrough, not an automatic blocker.

## Step 6 — Final walkthrough deliverable

A review session + short doc enumerating:
- every trait added/changed: name, crate, file, one-line role (`ServerProtocol`
  three verbs, `HttpModeledError`, `SerializableStruct`/`DeserializableShape`
  usage, generic `IntoResponse` impls, the rejection change);
- generated-code before/after snippets from the pokemon pair, both directions;
- everything deleted (protocol_serde generation, legacy serde fns, validation
  plumbing, `serializeMemberOrder`);
- divergence register with the test that pins each entry;
- what Phase 1 explicitly leaves for later (awsJson/restXml gates, validation
  engine, constrained closures, routing metadata at runtime, upstream ask status,
  and: model the remaining framework `RuntimeError` variants —
  `SerializationException`, `UnsupportedMediaTypeException`,
  `NotAcceptableException`, `InternalFailureException` — as smithy.framework
  `@error`/`@httpError` shapes serialized via `P::serialize_error`, completing
  the symmetry ValidationException started in 2d and collapsing the per-protocol
  `IntoResponse<P> for RuntimeError` bodies. Deliberately deferred because it is
  wire-changing: today's frozen framework bodies are quirks — restJson1 literal
  `{}`, awsJson1.1 empty body, rpcv2Cbor empty map with NO `__type` (#3716),
  restXml a JSON `{}` on an XML protocol — and each fix must land as a pinned
  divergence-register entry once the Step 5 gates exist, not as a side effect).

---

## Standing environment rules

- `JAVA_HOME` = scoop corretto21; run `./gradlew` via bash.
- `generateSmithyBuild` is not input-sensitive to `-P modules` — pass
  `--rerun-tasks` when the module list changes.
- Never run gradle/cargo builds while `:codegen-server:test` executes.
- Generated workspaces are `-D warnings`; standalone crates need `[workspace]`.
