# Assumptions Register — Verification Results

Verification executed 2026-08-22 on branch `fahadzub/mproto-clean` (codegen version
f97cba901). Method: generated 14 existing codegen-server-test modules plus 8 new
scenario projections (models in `codegen-server-test/custom-test-models/`, entries in
`codegen-server-test/build.gradle.kts` under `assumptionsVerificationTests` /
`failingAssumptionsTests`), read codegen + runtime + generated source, ran 36
in-process wire captures (`wire-capture` crate in the generated workspace), executed
the F2 byte-diff spike, and checked PR #4721 at head `043eae3c7` in worktree
`D:\smithy-rs-pr4721`. All items are now resolved — no PENDING entries remain.

Status legend: **CONFIRMED** = assumption verified as stated; **WRONG** = falsified,
correction recorded; **REVISED** = partially true, statement needed correction.

Reproduction:

```
# succeeding scenario crates
./gradlew --rerun-tasks -P modules='assumptions_a1_enum,assumptions_b5_distinct_ns,assumptions_d1_default,assumptions_d1_flag_true,assumptions_d3_custom_reason' \
    -P includeFailingAssumptionTests=true :codegen-server-test:generateSmithyBuild :codegen-server-test:smithyBuild
# expected-to-fail scenarios (a1_off, a1_on): same command with modules='assumptions_a1_off' or 'assumptions_a1_on'
```

## A. Event streams

### A1 — "Constraint traits reachable via event streams are rejected at codegen time"

**REVISED — the original RFC claim was closer to right than the register's first correction.**

- The check lives in `ValidateUnsupportedConstraints.kt`. The event-stream case
  (`UnsupportedConstraintOnShapeReachableViaAnEventStream`, lines 89–102) hard-codes
  `Level.SEVERE` with `canBeIgnored = false`: **`ignoreUnsupportedConstraints=true`
  does NOT downgrade it**. Empirical: `assumptions_a1_off` aborts codegen;
  `assumptions_a1_on` (flag=true) **also aborts** with the same SEVERE. The register's
  earlier note ("flag downgrades to WARNING and generates anyway") was wrong — the
  flag only downgrades the member-constraint / streaming-blob-length / range-on-float /
  uniqueItems cases (the `level` variable path, line 80).
- **EnumTrait carve-out is real** — excluded from both the non-error walk (line 420)
  and the error-shape walk (line 432). `assumptions_a1_enum` (enum in event payload,
  flag off) passes codegen. **But "enums in event streams fully supported" is WRONG:
  the generated crate does not compile.** `event_stream_serde.rs:81` calls the
  constrained event builder's fallible `build()` (returns
  `Result<EnumCapturingEvent, ConstraintViolation>`) where the plain struct is
  expected → E0308, plus a `redundant_semicolons` deny-warnings error. Genuine
  codegen bug: the validator permits what the generator cannot emit.
- Traits checked in the event-stream walks: Length, Pattern, Range, UniqueItems,
  Required (= `allConstraintTraits − EnumTrait`, `Constraints.kt:56-64`).
- **The error-shape leg of the check is dead code**: `ValidateUnsupportedConstraints.kt:423-434`
  walks `SyntheticEventStreamUnionTrait.errorMembers`, but `EventStreamNormalizer`
  removes those members from the model first, and the precomputed neighbor index
  returns no neighbors for removed shapes — so constraints inside event-stream ERROR
  shapes are silently accepted today (proof: pokemon's `@required pokeball` inside
  `InvalidPokeballError` generates fine with a fallible builder; jshell run of the
  compiled validator on the repo's own test model produced 1 message where the unit
  test expects 2).
- Enum support (`cf7f865f8`) is incomplete: only the payload-only unmarshaller branch
  got the fallible-build guard (`EventStreamUnmarshallerGenerator.kt:242-255`); the
  general branch emits bare `builder.build()` → the E0308 above. Where the guarded
  branch runs, enum violations surface as stream-level *unmarshalling errors*, not
  ValidationException.
- **No constraint validation runs per event today** in the pokemon SDK unmarshallers —
  builders are infallible by construction (constraints banned from event members).
- Extra sharp edge: setting `ignoreUnsupportedConstraints=true` when nothing needs it
  is itself a SEVERE abort ("the flag has no effect … please remove").
- Upstream: smithy#1388 (event-stream constraints) and #1389 (length on streaming
  blob) both **still open** as of 2026-08-22; no upstream semantics resolution.
- RFC consequence: current behavior = hard abort for event-member-reachable
  constraints (flag notwithstanding); silent acceptance for error-shape constraints
  (dead check); enums allowed but only the payload-only branch compiles.

### A2 — Event-stream error shapes never traverse the HTTP error path — **WRONG**

They traverse **both** paths. `EventStreamNormalizer.addStreamErrorsToOperationErrors`
hoists every input/output stream error into the operation error enum (generated:
`CapturePokemonError` contains `MasterBallUnsuccessful` and `InvalidPokeballError`,
which exist only in stream unions in the model). Consequences:

- Pre-first-event error (handler returns `Err`) → normal HTTP error path:
  `@httpError` status, `x-amzn-errortype: <ShapeName>`, JSON body via
  `ser_<shape>_error` — **but with Content-Type `application/vnd.amazon.eventstream`**
  over the JSON body (resolved from the streaming operation, not the payload); a
  freeze must include that quirk.
- Mid-stream error → exception frame: `:message-type: "exception"`,
  `:exception-type: <union MEMBER name>` (e.g. `invalid_pokeball` — member name, not
  shape name), `:content-type: application/json` (cbor on rpcv2), payload from the
  **same `ser_<shape>_error` function** the HTTP path uses.
- The stream-error enums (`CapturePokemonEventsError` etc.) and individual error
  structs get NO `IntoResponse` in any protocol module (only Debug/Display/Error/
  `name()`/From). Only the operation error enum has `IntoResponse`.

### A3 — Initial response uses the normal response serialization path — **CONFIRMED**

`alwaysSendEventStreamInitialResponse` defaults false (`ServerRustSettings.kt:123`).
Off: no initial-response frame; **non-stream output members (even `@required`) are
silently dropped from the wire**. On: one prepended frame (`:event-type:
"initial-response"`) whose payload comes from `ser_<op>_output_output_output(&output)`
— verbatim the same serializer the non-streaming response path uses; only the framing
differs. Protocol gating: rpcv2Cbor and awsJson handle initial-response; http-bound
protocols (restJson1/restXml) send an empty-payload frame when the flag is on.
Verified by diffing `rpcv2Cbor_extras` vs `rpcv2Cbor_extras_no_initial_response`.

### A4 — §2b SerializeError-yes / HttpModeledError-no split — **REVISED: implementable, but the "no" bucket is empty today**

Nothing breaks at the trait level (stream-error types have no `IntoResponse` to
preserve; exception frames live in separate `MarshallMessage` impls). **But** because
of the A2 hoisting, every event-stream error shape already has a working HTTP error
path — under current codegen the "ModeledError-but-not-HttpModeledError" class has
no inhabitants; withholding `HttpModeledError` from stream errors would break the
pre-first-event path (a regression). The split must key off where the shape appears
(§2b already says so) and the RFC should state the empty-bucket fact explicitly.
Two freeze constraints: the frame payload and HTTP body come from the same
`ser_<shape>_error` today (a schema-driven replacement must serve both or stay
byte-identical), and the `application/vnd.amazon.eventstream`-over-JSON quirk is part
of frozen behavior. §2b's constraint paragraph needs the A1 rewrite.

## B. Error serialization & discriminators

### B1 — restJson1 `x-amzn-errortype` = shape name only — **CONFIRMED**

`RestJson.kt:111-112`: `"x-amzn-errortype" to errorShape.id.name`. No namespace, no
URI suffix; no `__type` in restJson1 bodies at all (deliberate, smithy-rs PR #1982).
Generated proof: `pokemon-service-server-sdk/src/protocol_rest_json1/serde/shape_capture_pokemon.rs:150-151`.

### B2 — awsJson `__type` value form — **RESOLVED: 1.0 and 1.1 DIFFER**

`ServerAwsJson.kt:86-95`:
- **awsJson 1.0: full shape ID** — `"aws.protocoltests.json10#ComplexError"`,
  `"smithy.framework#ValidationException"` (the shape's OWN namespace, not the service's).
- **awsJson 1.1: name only** — `"ComplexError"`, `"ValidationException"`.
- **Framework errors (RuntimeError) emit no `__type` at all**: awsJson1.0 body `{}`,
  awsJson1.1 body `` (empty string) — except `Validation`, whose body is the
  pre-rendered ValidationException JSON (which contains `__type`).

### B3 — rpcv2Cbor `__type` — **CONFIRMED: full shape ID**

`AddTypeFieldToServerErrorsCborCustomization.kt:45-52` writes
`.str("__type").str("<namespace>#<Name>")` as the FIRST map entry for any `@error`
shape. Framework errors: no `__type` (explicit TODO smithy-rs#3716); non-validation
body is `0xa0` (empty CBOR map).

### B4 — restXml error envelope — **WRONG (both the assumption and its premise)**

- Premise "server doesn't support restXml" is false: `ServerRestXmlFactory` exists on
  origin/main (`ServerProtocolLoader.kt:74`); this branch inherits it.
- The envelope is a bare `<Error>` root with raw (un-renamed, lowercase) member
  element names — **no** `<ErrorResponse>` wrapper, no `<Code>`/`<Type>`/`<RequestId>`.
  This does NOT round-trip with smithy-rs's own client parser
  (`rest_xml_wrapped_errors.rs` expects `ErrorResponse/Error/Code/Message`).
- Worse: the restXml runtime discards pre-rendered validation/framework bodies and
  sends literal `"{}"` under `Content-Type: application/xml`
  (`rest_xml/runtime_error.rs:68`). restXml server error serialization is effectively
  broken today; "byte-identical schema-driven reproduction" is trivially achievable
  but freezing today's behavior would freeze a bug.

### B5 — custom `@validationException` emits its own namespace — **REVISED: true for the shape id, but the discriminator depends on WHICH PATH emits the error**

- New scenario `assumptions_b5_distinct_ns`: service in `com.aws.example.distinctns`
  (awsJson1.0), custom validation shape in `com.custom.errors`. Generated serializer
  emits `__type: "com.custom.errors#DistinctNsValidationException"` — the shape's own
  namespace, on namespace-carrying protocols. The generated restJson1 serializer for
  a custom shape likewise emits the custom name in `x-amzn-errortype`.
- **BUT wire capture shows the framework-originated validation-rejection path on
  restJson1 hard-codes the header**: a constraint violation against
  `custom-validation-exception-example` returns
  `x-amzn-errortype: ValidationException` (the `RuntimeError::Validation` name in
  `rest_json_1/runtime_error.rs`) — neither `MyCustomValidationException` nor its
  namespace appears anywhere in that response; only the BODY layout is customized
  (`customFieldList`/`customFieldMessage`/`reason`). The custom shape's own
  name/namespace reaches the wire only where the pre-rendered body carries it
  (awsJson/rpcv2 `__type`) or when the handler returns it as a modeled error.
- Caveat: custom-path top-level message drops the count prefix —
  `"validation error detected. …"` vs standard `"1 validation error detected. …"`.
- §2c must therefore distinguish "shape id used by serializers" (true) from
  "discriminator on the framework validation path" (hard-coded per-protocol).

### B6 — synthetic error wire forms — **CONFIRMED BY WIRE CAPTURE**

Live captures (harness: `codegen-server-test/build/smithyprojections/codegen-server-test/wire-capture`,
run `cargo test -p wire-capture -- --nocapture`; 36 captures):

| Case | restJson1 | awsJson1.1 | awsJson1.0 | rpcv2Cbor |
|---|---|---|---|---|
| Unknown op/route | 404, `x-amzn-errortype: UnknownOperationException`, body `{}` | 404, empty body | 404, empty body | 404, empty body |
| Wrong content-type | **415**, `UnsupportedMediaTypeException`, `{}` | **400**, empty | 400, `{}` | **400**, body `a0` |
| Bad Accept | **406**, `NotAcceptableException`, `{}` | **400**, empty | 400 | **400**, body `a0` |
| Malformed body | 400 | 400, empty | 400, `{}` | 400, `a0` |
| Missing `smithy-protocol` hdr | — | — | — | 404, empty |

Modeled awsJson1.1 error body: `{"Message":"Hi","__type":"InvalidGreeting"}`;
awsJson1.0: `{"Message":"Hi","__type":"aws.protocoltests.json10#InvalidGreeting"}`;
rpcv2Cbor: indefinite-length CBOR map with `__type` first
(`{_ "__type": "smithy.protocoltests.rpcv2Cbor#InvalidGreeting", "Message": "Hi"}`),
response carries `smithy-protocol: rpc-v2-cbor`. Empty-input operations skip both the
content-type check and body parsing entirely (garbage body/content-type still reaches
the handler) — a freeze-relevant quirk.

RuntimeError statuses shared across protocols: Serialization=400, InternalFailure=500,
NotAcceptable=406, UnsupportedMediaType=415, Validation=400. Bodies:

| Protocol | Content-Type | non-Validation body | Validation body |
|---|---|---|---|
| restJson1 | `application/json` + `X-Amzn-Errortype` header | `{}` | pre-rendered JSON |
| awsJson1.0 | `application/x-amz-json-1.0` | `{}` | pre-rendered JSON |
| awsJson1.1 | `application/x-amz-json-1.1` | empty | pre-rendered JSON |
| rpcv2Cbor | `application/cbor` | `0xa0` | pre-rendered CBOR |
| restXml | `application/xml` | `{}` (!) | `{}` (!) — pre-rendered XML discarded |

Reachability surprise: on awsJson and rpcv2Cbor, generated `NotAcceptable` /
`MissingContentType` rejections collapse to `Serialization` (400) — the 406/415
variants are dead code there; 406/415 only surface on REST protocols (406 restJson1
only). Router miss: 404 with `X-Amzn-Errortype: UnknownOperationException` + body
`{}` on restJson1; 404 empty-body on the others. Method mismatch: 405 empty, no
content-type, all protocols. Multi-protocol total miss (this branch):
`DefaultNotFoundService` → 404 `application/json` `{}` regardless of protocol.

### B7 — `IntoResponse` only on operation error enums — **CONFIRMED**

Exhaustive listing across all generated protocol modules: impls exist only on
`*Input` (FromRequest), `*Output` (IntoResponse), and operation error enums
(IntoResponse). Zero impls on individual error structs. Blanket-impl no-overlap claim
holds. Codegen site: `ServerHttpBoundProtocolGenerator.kt:453-476`.

### B8 — `ModeledErrorExtension` on all protocols — **CONFIRMED**

Single protocol-generic emission site (`ServerHttpBoundProtocolGenerator.kt:456-469`);
present in generated restJson1/restXml/rpcv2Cbor/awsJson1.0/awsJson1.1 modules. Only
on the modeled-error path; framework errors get `RuntimeErrorExtension`.

### B9 — `RuntimeError::Validation` carries a pre-rendered body — **CONFIRMED**

Generated `From<ConstraintViolation> for RequestRejection` per protocol builds the
full ValidationException and serializes it eagerly at conversion time. Nuances:
rpcv2Cbor's variant is `Validation(Vec<u8>)` (CBOR bytes) not `String`; restXml
pre-renders XML that the runtime then throws away (see B4).

## C. Constraint validation & builders

### C1 — Fail-fast, single-entry `fieldList` — **CONFIRMED (the freeze is right)**

Every level short-circuits: builder members in declaration order via `?`; nested
collections via short-circuiting `collect::<Result>`; composite scalar checks length
before pattern. `fieldList` is a literal `Some(vec![first_validation_exception_field])`
— no code path constructs a multi-entry list; the message prefix is the hard-coded
literal `"1 validation error detected. "`. Identical impls across restJson1, awsJson,
rpcv2Cbor. **Wire-confirmed**: a request violating two constraints
(`lengthString:"a"` + `rangeInteger:999`) returns exactly ONE `fieldList` entry (the
first violating member in codegen member order).

### C2 — Message templates — **EXTRACTED VERBATIM (frozen-template appendix source)**

Templates in `codegen-server/.../smithy/*ValidationErrorMessage.kt` and
`SmithyValidationExceptionDecorator.kt:217-219`:

- length: `Value with length {} at '{path}' failed to satisfy constraint: Member must have length between {min} and {max}, inclusive` (variants: `greater than or equal to {min}` / `less than or equal to {max}`)
- range: `Value at '{path}' failed to satisfy constraint: Member must be {range description}`
- pattern: `Value at '{path}' failed to satisfy constraint: Member must satisfy regular expression pattern: {pattern}`
- enum: `Value at '{path}' failed to satisfy constraint: Member must satisfy enum value set: [{values, comma-joined}]`
- uniqueItems: `Value with repeated values at indices {:?} at '{path}' failed to satisfy constraint: Member must have unique values`
- missing required: `Value at '{path}/{memberName}' failed to satisfy constraint: Member must not be null`

Path grammar: struct member `path + "/member"`; list element `path + "/<index>"`;
map value `path + "/<key>"`; map KEY violation reuses the map's own path.
Two live typos worth freezing verbatim: Range's shape-level Display is missing the
space before "failed" (`` `ns#Shape`failed ``); pattern's shape-level Display says
"match the regular expression pattern" / "the constraint" where member-level says
"satisfy" / "constraint".

**Wire-confirmed verbatim** (constraints crate, restJson1): length
`Value with length 1 at '/conA/lengthString' failed to satisfy constraint: Member must have length between 2 and 69, inclusive`;
range/pattern/required templates as above; path syntax `/conA/lengthString`
(slash-separated member names from the operation-input root). Rendering quirk:
`@range(min: -0)` renders as `between 0 and 69`.

### C3 — `@length` counts code points — **CONFIRMED**

Generated: `let length = string.chars().count();` (`ConstrainedStringGenerator.kt:248-254`
cites the Smithy spec's code-point rule). Unicode scalar values — not bytes, not
graphemes. Lists/maps use element count. **Unit-test confirmed with astral-plane
chars**: `"🚀"` (1 cp / 4 bytes) fails `@length(min:2)`; 69×`🚀` (276 bytes) passes
`max:69`; 70×`🚀` fails.

### C4 — `publicConstrainedTypes=false` relocation — **CONFIRMED**

Wrapper newtypes become `pub(crate)`; ConstraintViolations live in `pub(crate) mod
*_internal`; builders are doubled (public `build_enforcing_required_and_enum_traits`
+ internal `build_enforcing_all_constraints` used by deserializers).

### C5 — `build()` = TryFrom iff fallible — **CONFIRMED**

Fallible shapes: `build() -> Result<T, ConstraintViolation>` + `TryFrom<Builder>`;
infallible: `build() -> T` + `From<Builder>`. Missing-required Display:
`` `member` was not provided but it is required when building `Shape` ``; wire
message is the C2 "must not be null" template.

### C6 — Missing `@required` → 400 ValidationException, all protocols — **CONFIRMED BY WIRE CAPTURE**

All four protocols return 400, never 500:
- restJson1: `x-amzn-errortype: ValidationException`, JSON body, no `__type` in body.
- awsJson1.1: body `__type: "ValidationException"` (name only), no header.
- awsJson1.0: body `__type: "smithy.framework#ValidationException"` (namespaced).
- rpcv2Cbor: CBOR body with `__type: "smithy.framework#ValidationException"`;
  fail-fast (only the first of ~10 missing members reported).

The 1.1-name-only vs 1.0/rpcv2-namespaced `__type` split mirrors B2/B3. (The
awsJson Pokémon SDK could not exercise this path — its only `@required` member is
the event stream itself, which is always constructed; covered via json_rpc11.)

### C7 — Two-pass Option-based `build()` byte-identical — **FEASIBLE with one hard caveat**

Deterministic ordering rules (member declaration order; depth-first; length-before-
pattern; element-before-outer-collection-check) are all reproducible. **Blocker: map
entries iterate in `HashMap` order — when 2+ map entries violate, which one lands in
`fieldList` is already nondeterministic run-to-run today.** A freeze can only promise
"some violating entry" for that case; byte-determinism is impossible for both old and
new code. Aggregate-then-emit-all would change the wire contract (hard-coded "1", 
single-entry list) — a two-pass design must truncate to first by the current ordering.

## D. ValidationException machinery

### D1 — Auto-injection default / resource walk / programmatic construction / flag semantics — **CONFIRMED (all four, empirically + source)**

`AttachValidationExceptionToConstrainedOperationInputs.kt`: flag defaults `null` →
inject; walks full service closure (resources included — `walker.walkShapes(service)`,
line 93); `ensureValidationExceptionShapeExists` builds the three shapes
programmatically when `smithy-validation-model` is absent/projected away. Empirical
(`d1-injection.smithy`, no VE declared anywhere, one op behind a resource):
`assumptions_d1_default` generates with `TopOpError`/`GetWidgetError` =
ValidationException-only enums; `assumptions_d1_flag_true` same (+ deprecation warn);
`assumptions_d1_flag_false` **aborts** with SEVERE "You must model this behavior …
errors: […, ValidationException]".

### D2 — `@validationMessage` & friends — **CONFIRMED, one no-op found**

Explicit trait OR member named `message` (resp. `name`) — `CustomValidationExceptionUtil.kt:16-22`.
Validator enforces exactly-one message member, String target, default-constructibility.
**"Auto-annotation" is a silent no-op**: `annotateValidationMessageMember` builds the
annotated member and discards it (`UserProvidedValidationExceptionDecorator.kt:188-194`)
— harmless because all lookups are name-convention based, but the behavior exists
only in doc comments. `@validationFieldList` opts the field-list machinery in (absent
→ no list on the wire); `@validationFieldName` required in the field struct;
`@validationFieldMessage` optional, feeds both per-field and top-level message.
Custom-path top-level template: `"validation error detected. {}"` (no "1 ").

### D3 — Experimental `...PleaseDoNotUse` path — **REVISED: preserved cheaply, but NOT fully independent**

Shares the `ValidationExceptionConversionGenerator` seam; decorator order (-69) makes
it silently win over the trait path when both present; the injection transformer has
an explicit skip-branch for it; validators thread its shapeId. Empirical
(`assumptions_d3_custom_reason`): generates the with-reason conversion — same
fail-fast, same `"1 validation error detected. "` prefix, plus
`reason: FieldValidationFailed` and per-field reasons (e.g. `LengthNotValid`).
Cost of preserving: the skip-branch, the order invariant, interface parity. Cheap,
not free.

### D4 — `smithy.framework.rust` traits location/ownership/publication — **RESOLVED**

Backing model: `codegen-traits/src/main/resources/META-INF/smithy/validation-exception.smithy`
(this repo, module `codegen-traits`, added in PR #4321, on origin/main). Published to
Maven Central as `software.amazon.smithy.rust:codegen-traits` (0.1.3–0.1.25).
`smithy.framework#ValidationException` itself is NOT vendored — comes from upstream
`software.amazon.smithy:smithy-validation-model`, with the D1 programmatic fallback.
Anomaly: the `TraitService` registration file is doubly dead (wrong path
`META-INF/smithy/services/`, stale class names) — everything works because traits
load dynamically from the .smithy file and Kotlin checks are ID-based.

## E. Protocol layer & multi-protocol (this branch)

### E1 — Markers zero-sized, methodless; additive trait impl possible — **CONFIRMED**

All five markers are unit structs with only derived `Debug/Clone/Copy` plus
`ProtocolShape` (const ID), `OperationError`, and (branch) `ProtocolDetector<B,S>`.
No inherent impls, no blanket impls that could conflict. `ServerProtocol` (the RFC's
single server protocol trait — no `Inner`/object-safe split, see RFC §2b) can be
added purely additively.

### E2 — Multi-protocol architecture — **CONFIRMED (corrected names), zero `dyn` in protocol dispatch**

Runtime: `routing/multi_protocol.rs` — `ProtocolLayer<P,R>` / `ProtocolService<P,R,Inner>`
statically nested per protocol (canonical order rpcv2Cbor → awsJson1.1 → awsJson1.0 →
restJson1 → restXml, `ServerProtocolOrder.kt`), terminal `DefaultNotFoundService`.
Detection: rpcv2Cbor via `smithy-protocol: rpc-v2-cbor` header; awsJson via
`x-amz-target` + exact content-type; REST protocols via content-type essence
classification with Accept-header disambiguation for payloadless requests. Matched
protocol inserts `SelectedProtocol` extension. "ProtocolShape{protocol_id}" in the
register was a misremembering: real names are the `ProtocolShape` trait + generated
`{Service}ProtocolRoutes { p0_rpcv2_cbor, p1_rest_json1, … }`. No `dyn` anywhere in
protocol dispatch (only orthogonal operation-level `BoxCloneService` inside `Route`).
**The `serialize_error` seam = the `IntoResponse<P>` impl family at five sites**:
(1) router-error per protocol (`multi_protocol.rs:205-213`, never falls through);
(2) protocol-agnostic `DefaultNotFoundService` 404; (3) `FromRequest` rejection →
`RuntimeError: IntoResponse<P>` (with validation bodies pre-rendered in generated
`From<ConstraintViolation>` per protocol module); (4) generated
`IntoResponse<P> for {Op}Error` (modeled); (5) `MissingFailure<P>` (build_unchecked).

### E3 — Validation fused into deserialization — **CONFIRMED**

The only deserialized-but-unvalidated state is a function-local `Builder` inside
`de_*_http_request`; `FromRequest` yields only validated input or `RuntimeError`.
No middleware-observable unvalidated state. Nuance for the RFC: enforcement is
interleaved throughout parsing (constrained newtypes during `de_*`, required checks
at `build()`), not a discrete post-pass.

### E4 — Content-type/accept before deserialization — **REVISED (true in spirit, two caveats)**

Accept check strictly first in `from_request`. Content-type check runs before any
*parsing* but (a) after the body is fully collected into memory, and (b) is **skipped
entirely when the body is empty**. Bodiless ops check `content_type(None)`. In the
multi-protocol stack, router-level detection has already consumed content-type
before `from_request` runs.

## F. smithy-rs#4721 (verified at head 043eae3c7, 2026-08-22 — NOT yet merged; re-verify at merge)

### F1 — Companion doc §3 claims — **MOSTLY MATCHES, two corrections, several omissions**

- Document: extended in place (`Blob`/`Timestamp`/`BigInteger`/`BigDecimal` variants,
  insertion-ordered `DocumentObject` on indexmap, `DiscriminatedDocument`,
  `DocumentSettings` coercions). **"Document now has a lifetime" is WRONG — fully
  owned**; lifetimes are on `Schema<'a>`/`ShapeId<'a>` (covariance compile-asserted;
  codegen emits `&'static Schema<'static>` consts).
- TypeRegistry/error_registry/tier-2 dispatch: matches in every particular
  (`entry_for_error_code` sanitization, `.or()` composition, `reify_error` swallowing
  failures, `default_namespace` lifting of relative `__type`).
- Codecs: JSON full Document support; CBOR rejects documents AND bignums; XML rejects
  documents; **plus an unlisted awsQuery codec** (serialize-only, deser delegates to
  XML). Unlisted: `ClientProtocol`/`ClientProtocolInner`/`SharedClientProtocol` with
  `parse_error_metadata`/`deserialize_error_response` — the exact seam shape the
  server `serialize_error` will mirror; runtime protocol-swap config setter;
  `disableSchemaSerde` escape hatch; error_envelope module; header-name interning.
- **Allowlist: currently ENABLED FOR ALL FIVE protocols at head** with an in-source
  TODO to restore `emptySet()` before merge (phased rollout). `allowedServices` empty.
  When allowlisted, schema serde is the SOLE path (no legacy protocol_serde generated).
- Server-side exercise: zero (only a mechanical `DocumentObject::new()` touch in
  server builder codegen). `protocol-swap.rs` test fully commented out.

### F2 — JSON codec byte-identical error bodies — **CONFIRMED BY SPIKE (with two obligations)**

Executed spike (crates under the session scratchpad `f2-spike/{legacy,schema}`,
re-runnable with `cargo run`): legacy
`IntoResponse::<RestJson1>::into_response(GetPokemonSpeciesError::ValidationException(…))`
vs PR #4721 `JsonCodec` (`use_json_name(true)` + EpochSeconds) with a hand-mirrored
`Schema` + `SerializableStruct` for ValidationException. **Both sides produced the
identical 279-byte body** (md5 `2a12d4be21093d2ed7b608ec5f526347`), exercising
backslash/quote/`\n`/`\t` escaping and raw-UTF-8 non-ASCII. Legacy seam also yielded
status 400 + `content-type: application/json` + `x-amzn-errortype: ValidationException`.

Obligations for the RFC's no-fallback bet:

1. **Member write order is dictated by `serialize_members`, not the schema member
   array** — byte-identity required mirroring the legacy serializer's order
   (`fieldList` before `message`). Schema codegen must emit member writes in
   `JsonSerializerGenerator.kt`'s order.
2. **Float/double formatting diverges — confirmed byte-for-byte**: legacy (ryu via
   `aws_smithy_types::primitive::Encoder`) emits `10000000000.0` / `1.0`; PR codec
   (Rust `Display`) emits `10000000000` / `1` — integral-valued floats lose the `.0`.
   Non-integral values and NaN/±Infinity (quoted) match. Must route the codec through
   the legacy encoder (fix in `aws-smithy-json/src/codec/serializer.rs:393-429`).
   The ValidationException case cleared the bar only because it has no float members.
3. Codec produces body bytes ONLY — status/headers/`x-amzn-errortype`/content-length
   stay in the server error seam. `FinishSerializer::finish` is not object-safe; a
   protocol-erased hook must use `DynCodec`/`finish_boxed` (provided).
4. `@httpHeader`-bound error members still need a binding split (read-level finding;
   not exercised by the spike). Discriminator injection is pluggable via a wrapper
   `SerializableStruct` prepending a synthetic `__type` member.

**Implementation follow-up (2026-08-22, `mproto-schema-spike`) — all four obligations
discharged; obligation 1 needed a correction:**

- **Legacy member write order is PROTOCOL-DEPENDENT, not one order.** REST protocols
  (restJson1/restXml) serialize error document members in **member-name-sorted order**:
  `HttpTraitHttpBindingResolver.mappedBindings` ends in `.sortedBy { it.memberName }`
  (`HttpBindingResolver.kt:226`) — that is why `fieldList` precedes `message` and
  rest_json's `ComplexError` writes `Nested` before `TopLevel`. RPC protocols
  (awsJson 1.0/1.1, rpcv2Cbor) use `StaticHttpBindingResolver`, which binds
  `shape.members()` verbatim — **model member order** (json_rpc11's `ComplexError`
  writes `TopLevel` before `Nested`). One `serialize_members` impl per shape therefore
  cannot match both orders on a service mounting both protocol families; the server
  schema codegen orders `@error` shapes by the service's primary protocol
  (`SchemaGenerator.serializeMemberOrder`, set by `ServerSchemaDecorator`).
- Float fix landed (`Encoder`/ryu, f32 widened to f64 first, matching legacy).
- The seam is `ServerProtocol` in `aws-smithy-http-server` (`protocol/server_protocol.rs`);
  header split + discriminator wrappers live there. `__type` placement pinned by
  goldens: awsJson writes it **after** the members (not prepended); rpcv2Cbor first.
- Goldens: `codegen-server-test/wire-capture/tests/schema_serde_goldens.rs` —
  10 byte-identity tests green across restJson1 (incl. header split, empty-header
  skip, ValidationException ordering), awsJson 1.0/1.1, rpcv2Cbor, plus an explicit
  pin of the event-stream content-type divergence (A2 quirk: legacy stamps
  `application/vnd.amazon.eventstream` on pre-first-event errors; the
  operation-agnostic seam stamps `application/json`).
- **New coherence finding**: the RFC §2 blanket
  `impl<P, E: HttpModeledError> IntoResponse<P> for E` cannot coexist with the
  generated `impl IntoResponse<P> for {Op}Output` impls — Rust coherence performs no
  negative reasoning on the `E: HttpModeledError` bound (the classic manual-`ToString`
  conflict), so every generated crate would fail to compile. B7's "no overlap" was
  verified against actual impls, not coherence's future-proofing rules. Tier-1
  middleware ergonomics must come from codegen-emitted per-error-type
  `IntoResponse<P>` impls delegating to `serialize_error` (no coherence issue), not
  from a blanket impl.

## Side-findings (bugs discovered during verification)

1. **Request-time PANIC on a valid request** (constraints crate): `ConA.fixedValueInteger`
   targets `@range(min:69,max:69) integer` — a non-boxed primitive, so the member is
   non-`Option` with implicit default `0`, and the generated fallback
   `0i32.try_into().expect("this check should have failed at generation time…")`
   panics because the default violates the shape's own range (`model.rs:5118`; same
   landmine on `fixedValueShort/Long/Byte`). Masked whenever an earlier member
   violates (fail-fast). Any "deserialization failures are always 400" framing is
   qualified by this. Pinned by wire-capture test
   `side_finding_default_violating_range_panics_at_request_time`.
2. **Enum-in-event-stream codegen emits uncompilable Rust** (A1): E0308 +
   `redundant_semicolons` under `-D warnings` (the latter also present in
   `rpcv2Cbor_extras`'s generated `event_stream_serde.rs`).
3. **Dead validator leg**: event-stream error-shape constraints silently accepted (A1).
4. **restXml server error bodies broken**: bare `<Error>` envelope no client parses;
   validation/framework bodies replaced by `"{}"` under `application/xml` (B4/B6).
5. **`annotateValidationMessageMember` no-op** (D2) and **doubly-dead `TraitService`
   registration file** in codegen-traits (D4).
6. **Flag-on-with-nothing-to-ignore aborts codegen** (A1) — surprising DX.

## Cross-cutting conclusions for the RFC

1. **Fail-fast freeze stands** (C1); map-entry nondeterminism must be carved out of
   any byte-identity promise (C7).
2. **The discriminator appendix must be per-protocol AND per-version**: name-only
   (restJson1 header, awsJson1.1 body), full-ID (awsJson1.0, rpcv2Cbor body), and the
   custom-shape's own namespace rides along wherever the full ID is used (B2/B3/B5).
3. **restXml server errors are broken today** (bare `<Error>`, discarded validation
   bodies) — freezing current behavior would freeze a bug; the RFC needs an explicit
   decision here (B4/B6).
4. **Event streams**: constraints are hard-rejected regardless of flag; the enum
   carve-out ships uncompilable code; the error-shape check is dead (A1). Every
   stream error is ALSO an operation error (A2) — the §2b "SerializeError-yes /
   HttpModeledError-no" bucket is empty today (A4), and the frame payload and HTTP
   body share one serializer, so a schema-driven `serialize_error` must serve both.
   Treat "constrained shapes in event streams" as out of scope pending smithy#1388.
5. **The serialize_error seam on this branch is exactly the `IntoResponse<P>` family**
   (E2) — same shape as #4721's client-side `ClientProtocol::deserialize_error_response`,
   so the symmetric server trait is architecturally consistent.
6. **#4721 is not merged and still moving** (allowlist flip-flopping, "Bump codegen
   version (again)" at head). Don't merge into mproto-clean; re-verify F1 at merge.
7. **The central P1 bet is de-risked**: the schema codec reproduced a legacy restJson1
   error body byte-for-byte on the first real comparison (F2); the only codec-level
   fix needed is float formatting, and the remaining risk is procedural (member write
   order, header/binding split in the seam), not architectural.

## Scenario inventory (new, reusable)

| Projection | Model | Purpose | Outcome |
|---|---|---|---|
| `assumptions_a1_off` | pokemon-eventstream-constrained.smithy | @length in event payload, flag off | codegen ABORTS (expected) |
| `assumptions_a1_on` | same | same, flag on | codegen ABORTS — flag has no effect |
| `assumptions_a1_enum` | pokemon-eventstream-enum.smithy | enum in event payload, flag off | codegen OK, crate does NOT compile |
| `assumptions_b5_distinct_ns` | custom-validation-distinct-ns{,-errors}.smithy | custom VE in foreign namespace, awsJson1.0 | `__type` = `com.custom.errors#…` |
| `assumptions_d1_default` | d1-injection.smithy | no VE declared, op behind resource | injected everywhere |
| `assumptions_d1_flag_false` | same | deprecated flag explicit false | codegen ABORTS (old behavior) |
| `assumptions_d1_flag_true` | same | deprecated flag explicit true | warn + inject |
| `assumptions_d3_custom_reason` | d3-custom-reason.smithy | experimental with-reason flag | with-reason conversion generated |

(`assumptions_a1_off/on/enum` are gated behind `-P includeFailingAssumptionTests=true`
so default builds and cargo-based test tasks stay green.)
