# Companion Context: Schema-Decoupled Server RFC

Purpose: everything a future working session needs that is *not* in the RFC — source
evidence behind decisions, verification records (what was actually read, when, where),
the adopt/reject rationale against smithy-java, and the isolated decision log. The RFC
states conclusions; this records why and how they were reached, so stale-context errors
(see "Known verification gaps") can be caught rather than inherited.

Verification date for everything below: **2026-08-22**, against `smithy-lang/smithy-rs`
`main` (shallow clone, HEAD at chore release 1.1.7 era) and `smithy-lang/smithy-java`
`main` (shallow clone). smithy-rs#4721 was **open, unmerged, expected ~1 week out**;
its content was read from the PR description and commit list, not merged source.

---

## 1. smithy-java sources read, and adopt/reject per mechanism

| File | What it shows | Verdict for smithy-rs |
|---|---|---|
| `core/.../core/error/ModeledException.java` | `extends CallException implements SerializableStruct`; `schema()`; static `getHttpStatusCode(schema)` (`@httpError` → `@error` fault default → 500); `deserialized()` flag | **Adopt shape**: `ModeledError: SerializableStruct` + `schema()`; status resolution rules adopted but applied at codegen time (literal), not runtime. `deserialized()` deferred (matters only for proxy re-serialization, P3). |
| `core/.../schema/SerializableStruct.java` | `schema()`, default `serialize(encoder)` → `writeStruct`, `serializeMembers`, `getMemberValue` | **Adopt via dependency**: aws-smithy-schema already ships a Rust `SerializableStruct` (no `schema()` — ours lives on `ModeledError`). |
| `server/server-core/.../ServerProtocol.java` | `serializeError(Job, Throwable)` → `instanceof ModeledException` else `translate()` (SerializationException→MalformedRequest, else InternalFailure); registry membership check before serialize; funnels into `serializeOutput(job, error, isError=true)` | **Adopt**: explicit error/output seam, unmodeled funnel, generic-message rule. **Reject**: runtime registry membership check — Rust's closed enums + `HttpModeledError` bound make legitimacy static. |
| `core/.../schema/PresenceTracker.java` | Bitset presence tracking; `NoOpPresenceTracker` singleton for 0 required; ≤64 → `long`, else `BitSet`; `validateRequiredMembers(schema, bits)` with `trailing_zeros` name recovery | **Reject on generated path** (Option fields are the single source of truth; bitmask = Java workaround for non-nullable primitives; dual bookkeeping desyncs). **Adopt on dynamic path only** (P2) where no typed fields exist; keep the no-op singleton and >64 fallback. |
| `core/.../schema/Schema.java` + `SchemaBuilder.ValidationState` | Construction-time digestion: `stringValidationFlags` bitfield, pre-extracted min/max primitives, compiled pattern on the schema | **Reject**: compensates for missing monomorphization/const-folding. Rust: checker fns with literal constants, LLVM folds; typed digestion unnecessary on generated path. Dynamic path (P2) reads trait maps, caches compiled patterns per schema at startup. |
| `core/.../schema/DeferredMemberSchema.java`, `DeferredRootSchema.java` | Lazy resolution for recursive shapes; delegates `requiredMemberCount`/bitfield to resolved target; release notes record bitmask/recursion bugs here | **Adopt lesson, not mechanism**: recursive schema consts via `LazyLock` + codegen cycle detection; dedicated recursive-shape presence/validation tests day one. |
| `core/.../serde/TypeRegistry.java` | ShapeId→builder registry, client-side error deserialization | **Reject server-side**; #4721 supplies the client-side Rust equivalent; RFC adopts #4721 vocabulary. |
| smithy-java `ShapeBuilder.setMemberValue` default | Throws at runtime if unsupported | **Reject**: replaced with `DynamicShapeBuilder` capability trait — unsupported = compile error. |
| smithy-java `Validator` implements `Serializer` | Validation as a serialization walk | **Adopt** (P2 dynamic front-end). |

## 2. smithy-rs sources read (evidence for RFC claims)

- `codegen-server/.../PubCrateConstraintViolationSymbolProvider.kt`: confirms
  `publicConstrainedTypes=false` still generates ConstraintViolation enums, relocated
  `pub(crate)` into `*_internal` modules → basis for the §6 compat split.
- `codegen-server/.../ServerRustSettings.kt`: full `ServerCodegenConfig` read:
  `publicConstrainedTypes`, `ignoreUnsupportedConstraints`,
  `experimentalCustomValidationExceptionWithReasonPleaseDoNotUse` (preserved, out of
  scope), **deprecated** `addValidationExceptionToConstrainedOperations` (nullable;
  explicit true = warn+inject, explicit false = old behavior), `http1x`,
  `alwaysSendEventStreamInitialResponse`.
- `codegen-server/.../transformers/AttachValidationExceptionToConstrainedOperationInputs.kt`:
  auto-injection is **current default**; walks service closure incl. resources; injects
  `smithy.framework#ValidationException`; **constructs the shape programmatically** if
  absent from classpath / stripped by projections; honors deprecated flag.
- `codegen-server/.../validators/CustomValidationExceptionValidator.kt` +
  `customizations/UserProvidedValidationExceptionDecorator.kt`: `@validationException`
  (requires `@error`), member traits `@validationMessage` (or implicit member named
  `message`, auto-annotated), `@validationFieldList`, `@validationFieldName`,
  `@validationFieldMessage`; conversion generator emits
  `From<ConstraintViolation>`-style bridges + `as_validation_exception_field` per
  constraint — **this generator's output is the extraction source for frozen message
  templates**, and it is what gets reimplemented over `InputConstraintViolations`.
- `codegen-core/.../protocols/RestJson.kt` (lines ~95-112): server error header =
  `x-amzn-errortype: errorShape.id.name` (**name only**); comment documents the
  0.52.0–0.55.4 namespace-stripping churn (smithy-rs#1982, smithy#1493/#1494) → §2c
  freeze rationale.
- `codegen-server/.../protocols/ServerHttpBoundProtocolGenerator.kt` (~330-395):
  generated `impl IntoResponse<Marker>` for output and **for the operation error enum**
  (not per error struct) — calls generated `serialize_error`, inserts
  `ModeledErrorExtension::new(self.name())`, on failure logs via tracing and falls back
  to `RuntimeError::from(e).into_response()`. → blanket-impl overlap analysis (enum vs.
  struct impls: disjoint) and preserved-behavior checklist items.
- `rust-runtime/aws-smithy-http-server/src/routing/` + `src/protocol/*`: `Router<B>`
  trait, `RoutingService<R, Protocol>`, `Route`, `RequestSpec`/`UriSpec`; per-protocol
  zero-sized markers (`pub struct RestJson1;`), `rejection.rs`, `runtime_error.rs`
  (hand-written `into_response` per protocol — the synthetic-error code that gets
  re-based onto modeled shapes). rpcv2 `__type` injection currently lives in codegen
  customization `AddTypeFieldToServerErrorsCborCustomization` → moves to runtime
  protocol error-response fn under P1.
- `codegen-server/.../ValidateUnsupportedConstraints.kt`: constraint traits on shapes
  reachable via event streams are **rejected at codegen time** (semantics undefined);
  both event payloads and event-stream error shapes → zero event-stream validation
  surface in P1; rejection messages part of the freeze.
- `ServerOperationErrorGenerator.kt`: generates operation error enum **and** a separate
  per-union event-stream error enum (`eventStreamErrors()`); event errors travel as
  exception frames via `EventStreamErrorMarshallerGenerator` (codegen-core) — the second
  error path acknowledged in §2b.
- `rust-runtime/aws-smithy-schema` (v0.2.0, current main): serde traits exist —
  `SerializableStruct { serialize_members(&self, &mut dyn ShapeSerializer) }` (no
  `schema()`), object-safe `ShapeSerializer`/`ShapeDeserializer`; typed traits cover
  serialization + HTTP binding only (no error/constraint traits; `DocumentTrait` is the
  untyped fallback).
- `rust-runtime/aws-smithy-fuzz`: differential fuzzing harness (two revisions as
  cdylibs, AFL, model-derived `lexicon.json`) → §8a.
- `design/src/rfcs/rfc_template.md`: required RFC sections (followed).
- `design/src/rfcs/rfc0032_better_constraint_violations.md` (Accepted): current
  behavior is **fail-fast** (first violation short-circuits; single-entry `fieldList`);
  collection desired but flagged as DoS vector; tightness/impossible-variant problems
  concern the public per-shape enums. → RFC's "Relationship to prior RFCs" section and
  the fail-fast wire default in §4.

## 3. smithy-rs#4721 findings (from PR description + commit list; NOT merged source)

- Breaking `aws_smithy_types::Document`: adds Blob/Timestamp/BigInteger/BigDecimal,
  `#[non_exhaustive]`, insertion-ordered `DocumentObject` (indexmap); companions
  `DiscriminatedDocument`, `DocumentSettings`, `DocumentError`; "Document now has
  lifetime"; schema-crate's own Document **deleted**, serde migrated to types-crate
  Document.
- `ShapeId<'a>` / `Schema<'a>` (from_static → from_parts); covariance assertions;
  "Codegen now creates Schema<'static>"; recursive shapes with arbitrary lifetimes;
  **every codec signature threads the lifetime** (the churn our P1 targets post-merge).
- `TypeRegistry`/`error_registry()` per package; tier-2 error dispatch:
  operation-registry then service-registry (`ComposedRegistry::or`), reified error
  attached as `Unhandled.source()` via existing `CreateUnhandledError` seam;
  `entry_for_error_code` sanitized lookup; relative `__type` resolved against default
  namespace.
- JSON codec: full document support + protocol coercion (base64→blob,
  string/number→timestamp). CBOR: native blob/timestamp, documents
  `UnsupportedOperation` (spec pending). XML: documents rejected read+write (spec).
- Maturity: five runtime crates unit-tested + clippy-clean; client protocol compliance
  (restJson1, awsJson1.0/1.1, restXml+extras, rpcv2Cbor) green **via client codegen**;
  dynamodb + S3 integration tests green **with allowlist populated**;
  `SchemaSerdeAllowlist` empty-vs-enabled flagged as highest-scrutiny open review item;
  `protocol-swap.rs` gated off; **zero server-side exercise** → basis for "our suite is
  the merge gate" and the no-fallback repair doctrine.

## 4. Decision log (all settled by the owner unless marked)

1. P1 targets post-#4721 `main`; depends on its types/traits/codecs; defers all
   request-time Document machinery to P2; registries untouched in every phase.
2. Names: `ModeledError` (marker + `schema()`), `HttpModeledError` (`status_code()`),
   `InputConstraintViolations` (container; `Violation`/`ViolationKind` elements) —
   renamed from plural `ConstraintViolations` to avoid collision with legacy singular
   per-shape enums.
3. Legacy public per-shape `ConstraintViolation` enums: kept indefinitely as thin
   delegating shells; **no deprecation intent**; any opt-out flag is a future, separate
   decision.
4. Error bodies: `serialize_members` through schema-driven codecs, **all protocols**,
   P1. **No legacy error serializers generated at all.** Repair doctrine: wire
   discrepancy → fix codec; missing info → extend error serialization metadata. Never
   per-shape serializers.
5. Required members, generated path: two-pass Option-based `build()`; missing required =
   client validation error; defensive `else → internal invariant` arm; **no bitmask**,
   no `unwrap_unchecked` (UB failure mode rejected).
6. Wire discriminators: derived from the error shape's own full ShapeId; per-protocol
   emission policy frozen (restJson1 name-only; `__type` forms extracted verbatim);
   **`@wireTypeId` override cut** — multi-file/multi-namespace declaration is the
   mechanism; tier-2 `MiddlewareError` = fixed framework shape ID; no runtime-synthesized
   IDs.
7. Middleware: no error story required; three optional tiers; modeled middleware errors
   first-class via middleware-defined traits (e.g. `@awsauth`), never added to operation
   `errors`; integration surface stays `IntoResponse`.
8. ValidationException: upstream surface (auto-injection, `@validationException` +
   member traits, deprecated flag semantics, experimental WithReason path) **frozen**;
   this RFC only reimplements conversion internals over `InputConstraintViolations`.
9. Validation: request path only; fail-fast wire semantics preserved by default
   (single-entry fieldList; RFC-0032 coordination for collection); event-stream
   constraint rejection preserved; `@length` = code points etc. frozen to current
   behavior.
10. Dynamic servers: `DynamicShapeBuilder` capability trait + `generateDynamicBuilders`
    flag (default off); compile-time enforcement replaces smithy-java's runtime throw;
    generated builders never gain a bitmask even flag-on.
11. Testing: hand-written byte-identical suite (bodies AND headers) = merge gate;
    `aws-smithy-fuzz` differential fuzzing pre-RFC vs. P1 = backstop;
    divergence-is-regression triage rule.
12. Deferred to P2: value carrier for `set_member_value` (`Document` presumed default vs.
    borrowed `Value<'_>`); violation-inspection hook (new user capability) scope/API.
13. Crate placement: all runtime changes as modules in `aws-smithy-http-server`; depend
    on but never modify `aws-smithy-schema`/`aws-smithy-types`.

## 5. Known verification gaps / first tasks for an implementation session

1. **Extract frozen templates**: validation message strings, fieldList construction +
   ordering, and per-protocol `__type`/header discriminator forms — verbatim from
   current generated code (the conversion generators and protocol serializer output).
   This is the prerequisite for the byte-identical suite.
2. **Re-verify #4721 post-merge**: final signatures, allowlist decision
   (empty-for-merge?), whether `Document` lifetime landed as described. All P1 signatures
   in the RFC assume the PR description; the merged reality wins.
3. `ServerBuilderGenerator.buildFn` exact semantics (TryFrom vs From split, current
   short-circuit points) — read fully before replacing; only partially read here.
4. RFC-0032 implementation status of its own checklist (was its changeset ever landed?)
   — determines the coordination story for enabling collection.
5. `smithy.framework.rust` trait classes: locate backing smithy file / ownership, and
   whether the traits are published outside this repo (affects "surface frozen"
   promise scope).
6. **Correction log** (same failure class, caught in-session): (a) ValidationException
   auto-injection + `@validationException` existed upstream, missed on first read;
   (b) event-stream constraint rejection overstated — `ignoreUnsupportedConstraints`
   downgrades to warning and `EnumTrait` is excluded (RFC A1); (c) protocol was first
   modeled as type-parameter-only, missing `ClientProtocolInner` and the mproto-clean
   requirement for a value-level `ServerProtocolInner` (no `dyn` — server protocols are
   static; shape-side `&dyn` only); (d) validation initially proposed as a separate
   pipeline stage — rejected: middleware between deserialize and handler observe the
   conformant-input invariant, so validation fuses into `deserialize_request`.
   See `assumptions_register.md` for the full unverified-claims inventory.
7. Watch item: the ValidationException area changed upstream **between two reads within
   one working session** (auto-injection + `@validationException` landed unnoticed at
   first). Any claim in the RFC older than the current commit must be re-greppable; this
   file's tables give the paths.

## 6. Discriminator history quick-reference

restJson1 server header: `x-amzn-errortype`, shape **name only**. History: namespace
stripped in smithy-rs 0.52.0–0.55.4 per spec SHOULD → reverted after breakage;
references smithy-rs#1982, smithy#1493, smithy#1494. Do not relitigate. awsJson/rpcv2
`__type`: extract current emission verbatim (task 5.1); rpcv2's injection currently in
`AddTypeFieldToServerErrorsCborCustomization` (codegen) → moves to runtime per §2c.
