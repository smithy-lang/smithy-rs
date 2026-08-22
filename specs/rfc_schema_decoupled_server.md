RFC: Schema-Decoupled Serialization, Modeled Errors, and Runtime Constraint Validation for smithy-rs Servers
=============================================================================================================

> Status: RFC
>
> Applies to: server (with noted, non-breaking interactions with client codegen)

> Phasing: Phase 1 targets post-#4721 `main` and depends on its **types, traits, and codecs** (`Schema<'a>`, `SerializableStruct`, schema-driven JSON codec) while deferring its **dynamic machinery** (documents at request time, registries) to Phases 2-3. See [Phasing](#8-phasing).

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

This RFC defines how `aws-smithy-http-server` adopts schema-decoupled serialization and
deserialization on the server side, introduces a modeled-error abstraction (a Rust trait
implemented by every `@error` shape) as the single path through which all errors are
serialized, makes `ValidationException` a framework-level concern instead of a per-operation
model obligation, and replaces per-shape generated constraint-validation code with a single
runtime validation engine fed by compile-time constants. Throughout, the design follows two
tenets, in priority order:

1. **Backward compatibility first** — on the wire and in the public Rust API. Existing
   service teams recompile and redeploy with zero changes.
2. **Adopt what is good from smithy-java, discard what is Java-shaped** — smithy-java's
   schema/serde architecture is the reference design, but wherever Java performs runtime
   work to compensate for missing monomorphization, const evaluation, or niche-optimized
   `Option`, this design moves that work to codegen/compile time.

This RFC builds on the "Document Types and Type Registries" SEP as implemented in
smithy-rs#4721 (expanded `Document`, `ShapeId<'a>`/`Schema<'a>`, package-level
`registry()`/`error_registry()`, tier-2 client error dispatch) and is designed to be
compatible with it.

**Crate placement invariant**: all runtime changes in this RFC live in
`aws-smithy-http-server` (as modules; no new runtime crate). This RFC **depends on but
does not modify** `aws-smithy-schema` and `aws-smithy-types`: Phase 1 builds against the
post-#4721 surface of both (see Phasing) and pins a minimum `aws-smithy-schema` version,
but no change in this RFC lands in either crate. Where schema-carried metadata is needed
on the dynamic path (P2), `aws-smithy-http-server` reads it through the existing untyped
`DocumentTrait` fallback via server-owned helper functions; upstreaming typed traits into
`aws-smithy-schema` post-#4721 is optional future work, never a dependency. (Codegen
changes live in `codegen-server` as usual.)

Terminology
-----------

- **Schema-decoupled serde**: Serialization/deserialization driven by runtime `Schema`
  values (from `aws-smithy-schema`) through generic serializer/deserializer interfaces,
  rather than by per-shape generated wire code. Analogous to smithy-java's
  `SerializableStruct` / `ShapeSerializer` / `ShapeDeserializer` design.
- **`SerializableStruct`**: The Rust trait (name illustrative) implemented by generated
  structures and unions exposing `schema()`, `serialize_members(&mut dyn ShapeSerializer)`,
  and member access — the Rust analog of smithy-java's interface of the same name.
- **Modeled error**: A structure carrying the `@error` trait in the Smithy model. In this
  design every error that reaches the wire is a modeled error; there are no hand-rolled
  error serializers.
- **`ModeledError` / `HttpModeledError`**: New marker/extension traits. `ModeledError:
  SerializableStruct` marks a shape as a modeled error; `HttpModeledError: ModeledError` adds
  `status_code()` with a default derived from schema traits. `serialize_error` accepts only
  `E: HttpModeledError`.
- **`ServerProtocol`**: The new server-side protocol trait (defined in §2b), implemented
  once per protocol on the existing zero-sized markers in `aws-smithy-http-server`. It
  owns the schema-driven codec and the `serialize_output`/`serialize_error` seam. A
  single trait — deliberately not mirroring #4721's client `ClientProtocolInner` /
  object-safe `ClientProtocol` pair, because server protocol dispatch is fully static.
  Named after smithy-java's `ServerProtocol`.
- **Framework errors**: Modeled errors owned by the framework rather than the service
  model — e.g. `InternalFailure` (reusing the existing smithy-rs internal error shape),
  `MalformedRequest`-class errors, and the framework `ValidationException`.
- **Middleware errors**: Modeled errors defined in a middleware's own Smithy file (e.g. an
  auth middleware's `UnauthorizedException`). Operations opt into the middleware via a
  middleware-defined Smithy trait (e.g. `@awsauth`); the middleware's errors are **not**
  added to the operation's `errors` list.
- **`InputConstraintViolations`**: A single runtime type aggregating constraint violations as data
  (path + kind), replacing per-shape `ConstraintViolation` enums as the internal currency
  of validation.
- **`PresenceTracker`**: A bitset over schema member indices tracking which members of a
  structure were supplied, used only on the dynamic/`Document` path where typed `Option`
  fields do not exist. Ported from smithy-java, including the >64-member fallback.
- **Generated path / dynamic path**: The two front-ends of every mechanism in this RFC.
  The generated path is code emitted per shape and monomorphized with literal constants;
  the dynamic path operates on runtime `Schema<'a>` values and `Document`s (proxy-style
  services, smithy-rs#4721's `deserialize_document`). Both converge on shared runtime
  engines and must produce byte-identical wire output.

The user experience if this RFC is implemented
----------------------------------------------

### Service teams (server)

Nothing is required. Existing services recompile unchanged; wire behavior — status codes,
error bodies, `ValidationException` messages, `fieldList` contents and ordering — is
byte-identical.

New capabilities, all opt-in:

1. **`ValidationException` framework ownership is already upstream — this RFC preserves
   it and re-plumbs its internals.** Current `main` already auto-injects
   `smithy.framework#ValidationException` into constrained operations that don't declare
   it (`AttachValidationExceptionToConstrainedOperationInputs`, which even constructs the
   shape programmatically if absent from the classpath or stripped by projections), and
   already honors the deprecated `addValidationExceptionToConstrainedOperations` flag for
   backward compatibility (explicit `true` warns and injects; explicit `false` preserves
   old behavior). This RFC introduces **no new flag and no new transform**: the existing
   behavior, including the deprecated flag's semantics, is frozen. The client-side
   consequence stands as-is: typed client variants require the injected error to be
   present in the model the client is generated from; #4721 tier-2 reification is the
   softer fallback for new smithy-rs clients.
2. **Custom validation shapes are likewise already supported — via member traits, not
   trait arguments.** `smithy.framework.rust` ships `@validationException` (structure
   trait) with `@validationMessage`, `@validationFieldList`, `@validationFieldName`, and
   `@validationFieldMessage` (member traits), enforced by `CustomValidationExceptionValidator`
   (must be `@error`; exactly one String message member, discovered explicitly via
   `@validationMessage` or implicitly by the name `message`, auto-annotated by the
   decorator). `UserProvidedValidationExceptionDecorator` + its conversion generator
   already produce the violation→shape bridge. This RFC **adopts this surface unchanged**
   (an earlier draft's trait-argument mapping syntax is superseded) and rewires only the
   internals: the conversion generator currently renders from per-shape
   `ConstraintViolation` enums, which section 6 replaces — the bridge is reimplemented
   over `InputConstraintViolations` with byte-identical output. The experimental
   `experimentalCustomValidationExceptionWithReasonPleaseDoNotUse` path is preserved
   until its owners retire it; it is out of scope here beyond not breaking it.
3. **Modeled middleware errors are first class.** A middleware ships a Smithy file with its
   trait (e.g. `@awsauth`) and its error shapes. Marking an operation `@awsauth` causes the
   middleware's generated layer to be applied; its errors serialize through the same
   machinery as operation errors. The service model's operation `errors` lists are not
   polluted, and the core request path is unaware of the middleware.

### Middleware authors

The integration surface is unchanged: plugins/layers return `IntoResponse<P>` exactly as
today. **Middleware are not required to have an error story at all**: a middleware with no
failure modes (metrics, logging, tagging) never touches any of this, and nothing in codegen
or the runtime requires a middleware crate to ship a Smithy file, implement `HttpModeledError`, or
declare anything. The tiers below are strictly optional capabilities offering progressively
more wire control:

- **Tier 1 — modeled middleware error** (preferred): define the error in Smithy; codegen
  implements `HttpModeledError`; a blanket `impl<P, E: HttpModeledError> IntoResponse<P> for E` (per
  protocol `P`) makes it returnable with correct status and body. No new functions are
  exported from middleware; no registration step exists.
- **Tier 2 — `MiddlewareError`**: a hand-written runtime type for authors who do not want
  a Smithy file: `{ status: u16, message: Option<String>, source: Box<dyn Error> }`, with a
  hand-written `HttpModeledError` impl against a small framework schema. The `message` field is
  the explicit opt-in for exposing text on the wire.
- **Tier 3 — `Box<dyn Error>`**: converted by the framework funnel into `InternalFailure`
  with a **generic** message (never `err.to_string()` — internals must not leak) and
  status 500; the cause is preserved for logging only.

### Handler authors

Unchanged. Handlers return `Result<Output, OperationError>`; the closed enum statically
guarantees only modeled operation errors are returned, and the orchestrator's `Err` arm is
the error-serialization entry point (no `is_error` detection inside serializers).

### Client impact (non-breaking)

- Old/regenerated clients from unchanged models: no visible change.
- Clients regenerated from a model processed by the server-side transform (or from models
  where teams used the flag and the published model includes the injected
  `ValidationException`): typed variant as today, minus the boilerplate.
- New smithy-rs clients with smithy-rs#4721 tier-2 dispatch: where the framework
  `ValidationException` (or middleware errors, if the middleware's Smithy file is included
  in client codegen) is present in the service-wide `error_registry()`, an unmodeled error
  code is reified and attached as `source()` of `Unhandled` — typed and downcastable
  without an operation-enum variant. This is a strictly softer failure mode, not a
  replacement for model presence; old clients (no registries) still require model presence
  for a typed experience, which is why declare-or-inject remains the mechanism.

How to actually implement this RFC
----------------------------------

### 1. Error status metadata (no schema-crate changes)

`aws-smithy-schema` currently defines no error-related traits (verified against `main`:
typed traits cover only serialization and HTTP binding concerns) — and per the crate
placement invariant, this RFC does not add any.

- **Generated path (P1)**: codegen already knows `@httpError`/`@error` at generation
  time, so the generated `HttpModeledError` impl bakes the resolved status as a literal
  (`fn status_code(&self) -> u16 { 404 }`). Resolution rules (`@httpError` code, else
  `@error` fault default client=400/server=500, else 500 — smithy-java's
  `ModeledException.getHttpStatusCode` semantics) are applied by codegen, once, at build
  time. No runtime schema lookup exists on this path.
- **Dynamic path (P2+)**: a server-owned helper in `aws-smithy-http-server`,
  `resolve_status(&Schema<'_>) -> u16`, applies the same rules by reading the schema's
  trait map through the existing untyped `DocumentTrait` fallback. `aws-smithy-schema`
  is not modified; if typed `ErrorTrait`/`HttpModeledErrorTrait` are ever upstreamed there, the
  helper's body shrinks but its signature and call sites do not change.

### 2. Error traits and the single serialization path

`SerializableStruct` is **not defined by this RFC**: `aws-smithy-schema` already ships it
(`serialize_members(&self, &mut dyn ShapeSerializer) -> Result<(), SerdeError>`, no
`schema()` method), alongside object-safe `ShapeSerializer`/`ShapeDeserializer`, and
smithy-rs#4721 finalizes their signatures (`Schema<'a>` threading, `aws_smithy_types::Document`
migration). This RFC bounds on that surface as of post-#4721 `main`:

```rust
/// Marker: this shape is a modeled @error structure. Protocol-agnostic.
/// Supplies the schema (absent from aws-smithy-schema's SerializableStruct).
pub trait ModeledError: SerializableStruct {
    fn schema(&self) -> &Schema<'_>;
}

/// HTTP extension; the bound accepted by aws-smithy-http-server.
pub trait HttpModeledError: ModeledError {
    fn status_code(&self) -> u16;   // P1: codegen bakes a literal; no default body
}
```

- Codegen implements both for every `@error` structure (service, framework, and
  middleware models alike), returning the shape's `Schema<'static>` const per #4721's
  codegen conventions. `status_code` is overridable, leaving room for custom impls
  without reopening hand-rolled serialization. The bound and `schema()` are frozen from
  Phase 1 so trait bounds never change across phases (no later semver event).
- **`ServerProtocol`** (new trait, `aws-smithy-http-server`): the server-side protocol
  seam, named after smithy-java's `ServerProtocol` (`serializeError` funneling into
  `serializeOutput(job, error, isError = true)`). It is implemented directly on the five
  existing zero-sized protocol markers (`RestJson1`, `AwsJson1_0`, `AwsJson1_1`,
  `RpcV2Cbor`, `RestXml`) — verified purely additive: the markers carry only derived
  impls plus `ProtocolShape`/`OperationError`/`ProtocolDetector`, no inherent methods,
  no conflicting blanket impls.

  ```rust
  /// Implemented on each protocol marker. One impl per protocol; all dispatch is
  /// static — the multi-protocol router nests `ProtocolService<P, ..>` levels that
  /// are each monomorphized over their marker, so by the time output or an error
  /// is serialized the protocol is statically known.
  pub trait ServerProtocol: ProtocolShape {
      /// The schema-driven body codec for this protocol (e.g. `JsonCodec` configured
      /// for restJson1). Associated type, not `DynCodec`: `FinishSerializer::finish`
      /// is not object-safe, and no protocol-erased call site exists server-side.
      type Codec: Codec;
      fn codec(&self) -> &Self::Codec;

      /// Serialize a success or error payload. `is_error` selects error framing
      /// (discriminator injection, error content-type rules); serializers never
      /// detect errors — call sites declare them.
      fn serialize_output(
          &self,
          schema: &Schema<'_>,
          output: &dyn SerializableStruct,
          is_error: bool,
      ) -> Result<http::Response<BoxBody>, SerdeError>;

      /// Serialize a modeled error to a complete response: status from
      /// `HttpModeledError::status_code`, protocol discriminator
      /// (`x-amzn-errortype` name-only on restJson1; `__type` full shape ID on
      /// awsJson 1.0 / rpcv2Cbor, name-only on awsJson 1.1 — via a wrapper
      /// `SerializableStruct` prepending the synthetic member), content-type,
      /// and body via `serialize_output(.., is_error = true)`. Serialization
      /// failure falls back to the modeled `InternalFailure` path (infallible
      /// surface), preserving today's `IntoResponse` fallback semantics.
      fn serialize_error<E: HttpModeledError + ?Sized>(&self, error: &E)
          -> http::Response<BoxBody>;
  }
  ```

  Serializers never detect errors; call sites declare them. On the handler path the
  `Result` match arm is the declaration; on the middleware path the `HttpModeledError`
  bound is.

  **Why one trait, not the client's `Inner`/object-safe pair**: #4721's client seam is
  split (`ClientProtocolInner` → blanket object-safe `ClientProtocol` →
  `SharedClientProtocol = Arc<dyn ClientProtocol>`) because the client stores a
  runtime-injected protocol in config and its orchestrator needs an erased handle. The
  server has no such requirement: protocols come from the statically nested
  multi-protocol router, never from config, and `serialize_error`'s generic method
  makes the trait non-object-safe anyway. If runtime-injected server protocols ever
  become a requirement, the same flattening precedent applies (an object-safe
  `DynServerProtocol` taking `&dyn HttpModeledError` and using `DynCodec::finish_boxed`,
  blanket-implemented for every `ServerProtocol`) — reserved as future work, not
  designed here.
- **Preserved behaviors from today's generated `IntoResponse` impls** (verified in
  `ServerHttpBoundProtocolGenerator`): the response extension
  (`ModeledErrorExtension::new(error_name)`) is still inserted on modeled-error
  responses, and serialization failure still logs via tracing and falls back to the
  protocol `RuntimeError`-equivalent (now the modeled `InternalFailure` path). Note the
  existing generated impl is on the **operation error enum**; the blanket impl is on
  individual `HttpModeledError` shapes — no impl overlap, and the enum's impl becomes a
  variant-match delegating to the blanket impl.
- **P1 error-body serialization (decided)**: error bodies serialize via
  `serialize_members` through the schema-driven codecs, **for all protocols**. Errors
  are thereby the first server-side consumer of #4721 schema serde. Notes:
  - #4721's rejection of *document-typed shapes* in XML is Smithy-spec compliance and
    does not constrain this path: error structures have concrete members, and
    schema-driven struct-to-XML serialization (including restXml error envelopes) is in
    scope, proven by our tests rather than inherited from #4721.
  - #4721's server-side exercise of schema serde is zero (its compliance evidence is
    client-side, and the codec may ship dormant behind `SchemaSerdeAllowlist`), so the
    **byte-identical error-response test suite is the merge gate**: every protocol, all
    error origins (operation, framework, all middleware tiers), empty-body edge cases,
    status codes, per-protocol discriminator placement, and **full headers**
    (content-type and protocol-specific error headers — header stamping moves from
    generated per-error code to runtime per-protocol code, so headers are asserted, not
    just bodies).
  - **No fallback path exists**: legacy per-error serializer functions are not
    generated at all under P1. The repair doctrine for the single path: a wire
    discrepancy is fixed **in the codec**; missing wire-relevant information is fixed by
    **extending the error serialization metadata** (trait surface / schema-carried
    data) — never by reintroducing per-shape serializers. The byte-identical suite is
    therefore unambiguously load-bearing: it is the safety story, not a tripwire for a
    flip that doesn't exist.
  - **Event streams are excluded from the codec flip** in all phases as currently
    scoped: mid-stream exception frames use the existing event marshallers (see the
    Event streams subsection).
- **The unmodeled-error funnel** (analog of smithy-java's `translate()`): deserialization
  failures map to the framework's malformed-request error; everything else
  (`Box<dyn Error>`, panics caught at the tower layer) maps to `InternalFailure`.
  smithy-rs's existing internal error shape is reused, not replaced.
- **No server-side error registry.** Legitimacy is established at compile time by the
  trait bounds and closed operation enums. smithy-rs#4721's registries remain client-side
  deserialization machinery; the server neither needs nor gains a runtime set-membership
  check. (smithy-java performs a runtime registry check in `serializeError` because its
  `Throwable`-based path is open; Rust's is closed.)
- Because a blanket `impl<P, E: HttpModeledError> IntoResponse<P> for E` cannot coexist with
  bespoke `IntoResponse` impls for individual error types, all framework synthetic errors
  (unknown operation, malformed request, internal failure) become modeled framework
  shapes. One serialization path total; "everything modeled" is a hard rule.

### 2c. Wire discriminators and error namespaces

Protocols identify errors on the wire via a discriminator — `x-amzn-errortype` header
(restJson1) or a `__type` member (awsJson 1.0/1.1, rpcv2Cbor). Three rules govern its
derivation on the new path:

1. **The discriminator derives from the error shape's own full `ShapeId`** — read off the
   error's schema (`ShapeId<'a>` carries the namespace post-#4721) — never assumed and
   never the service's namespace. Consequences, stated explicitly: an
   `@validationException`-annotated custom shape emits under **its own** namespace and
   name (this is the wire-contract change teams opt into by substituting the shape); the
   framework `ValidationException` and other framework errors emit under
   `smithy.framework`; middleware modeled errors emit under the middleware's namespace.
2. **Namespace emission is per-protocol policy applied to that full ID, frozen to
   current behavior.** restJson1 emits the shape **name only** in `x-amzn-errortype` —
   the settled behavior after the 0.52.0–0.55.4 namespace-stripping churn (smithy-rs#1982,
   smithy#1493/#1494); this RFC does not relitigate it. `__type` emission for
   awsJson/rpcv2 is extracted verbatim from current server codegen as part of the
   message-freeze extraction task and pinned by the byte-identical suite. The policy is a
   protocol-layer function over `&ShapeId<'_>` choosing name vs. full form.
3. **Tier-2 `MiddlewareError` serializes under a fixed framework shape ID** (a generic
   `smithy.framework`-namespaced error) regardless of its dynamic status/message; shape
   IDs are never synthesized from strings at runtime.

**Choosing the wire namespace is done by declaration, not by override.** A middleware (or
service) whose error must carry a specific namespace on the wire declares that error in a
file with that namespace — Smithy models are multi-file and multi-namespace, and `use`
imports the shape wherever it is referenced. The shape's ID *is* the wire identity; no
mechanism exists in this RFC to emit a discriminator differing from the shape's own ID.
(A wire-ID override trait was considered and cut: multi-namespace declaration covers the
known requirement, keeps shape identity and wire name one thing, and keeps client
generation and #4721 registry keying trivial. It can be introduced later if a genuinely
inexpressible case appears.)

A server→client round-trip test is added against #4721's tier-2 dispatch (which resolves
discriminators via registries with relative-`__type` handling): our emitted discriminators
must reify correctly there and remain accepted by pre-#4721 clients — guaranteed by the
freeze, verified by the test.

### 2a. Routing (unchanged)

No phase changes routing. The runtime routing engine (`Router<B>`, `RoutingService`,
`Route`, per-protocol routers such as `RestRouter`) and its public API are untouched;
route registration remains code-generated (`RequestSpec` construction from `@http`,
service-builder wiring) for codegen'd servers in all phases. Errors and validation sit
entirely after routing inside the routed `Route`. The router-miss (unknown operation)
response is re-based onto a modeled framework shape — changing what is serialized on a
miss, with identical wire output, not how requests are matched. Phase 3 runtime servers
construct `RequestSpec` values at startup from the loaded model — a parallel construction
path in the runtime server layer; generated services are not migrated onto it, and the
matching engine is shared as-is (the request specs were runtime values all along).

### 2b. Event streams

Event streams cross every subsystem; their scope in this RFC is deliberately minimal:

- **Two error paths, honestly stated**: this RFC's "one serialization path" claim covers
  **HTTP-response errors** (including a stream operation's initial response). Mid-stream
  modeled errors travel as exception frames inside the stream via the existing generated
  event marshallers (`EventStreamErrorMarshallerGenerator` output) — a pre-existing,
  separate path that no phase modifies.
- **Trait implementations**: event-stream error shapes implement `ModeledError`
  (marker; they are `@error` structures) but **not `HttpModeledError`** — a status code is
  meaningless mid-stream, and omitting `HttpModeledError` makes misuse of the blanket
  `IntoResponse` impl a compile error rather than a tested-against hazard. Shapes used
  both as operation errors and event-stream errors implement both.
- **Constraint validation**: ⚠ **pending empirical verification (assumptions register
  A1)** — the rejection of constraint traits reachable via event streams is *not*
  unconditional: it is build-failing by default but downgraded to a warning under
  `ignoreUnsupportedConstraints=true` (generation proceeds; generated validation
  behavior unverified), and `EnumTrait` is excluded from the check entirely (enums in
  event streams are supported today). Upstream smithy issues #1388/#1389 ("semantics
  unclear") may since be resolved. This RFC's event-stream validation scope is therefore
  **to be determined by the verification scenarios in the assumptions register** before
  P1 implementation; whatever current generated behavior is (including under the ignore
  flag and for enums) is the freeze target.
- **Builders**: event structures use the same two-pass `Option`-based `build()` as all
  structures; no special casing.

### 3. `ValidationException`: preserve upstream surface, re-plumb internals

Discovered against current `main` (post-dating this RFC's first draft): auto-injection,
the `@validationException` custom-shape mechanism, and the violation→shape conversion
generator all already exist (see the user-experience section for the inventory). The work
in this RFC is therefore **internal replacement, not feature introduction**:

- **Provenance**: `smithy.framework#ValidationException` is defined in the
  `smithy-validation-model` Maven package of the upstream smithy repo; the
  `@validationException` family of traits ships as `software.amazon.smithy.framework.rust`
  classes in this repo. Evolution of the upstream shape is outside this RFC's control;
  the programmatic-construction fallback in the transformer already insulates codegen
  from its absence.
- **Public surface frozen**: `@validationException` + member traits
  (`@validationMessage`, `@validationFieldList`, `@validationFieldName`,
  `@validationFieldMessage`), `CustomValidationExceptionValidator` rules and messages,
  the auto-injection transformer's behavior (including programmatic shape construction
  and walking resources), and the deprecated `addValidationExceptionToConstrainedOperations`
  flag's compatibility semantics — all unchanged.
- **Internals replaced**: the conversion generators
  (`SmithyValidationExceptionConversionGenerator` for the framework shape,
  `UserProvidedValidationExceptionConversionGenerator` for custom shapes) currently
  render `From<ConstraintViolation>` impls and per-constraint
  `as_validation_exception_field` methods off the per-shape violation enums. These are
  reimplemented as a single `ValidationErrorFactory` over `InputConstraintViolations` (section 6), with
  message strings, `fieldList` construction, and ordering byte-identical — the existing
  generators' output is the extraction source for the frozen templates.
- **Wire discriminator**: per section 2c, the effective validation shape's own `ShapeId`
  drives the discriminator — `smithy.framework#ValidationException` for the injected
  shape, the custom shape's own namespace when substituted (already today's behavior;
  now stated and pinned).
- **Published-model note (P2)**: whether the injection transformer's output reaches the
  *published* model artifact consumed by client pipelines is a model-publishing question
  tracked in Phase 2; server behavior does not depend on it.

### 4. Required members without bitmasks (generated path)

The generated builder keeps `Option<T>` fields as the **single source of truth** for
presence; the public struct keeps `T` for required members (no API change). `build()` is
two passes — aggregate all missing members, then move:

```rust
fn build(self) -> Result<Struct, InputConstraintViolations> {
    let mut v = InputConstraintViolations::new();
    if self.a.is_none() { v.missing("a"); }
    if self.b.is_none() { v.missing("b"); }
    if !v.is_empty() { return Err(v); }               // client fault -> validation error
    Ok(Struct { a: self.a.unwrap(), b: self.b.unwrap() })
}
```

**Wire-compat constraint on aggregation (per RFC-0032's findings)**: today's
deserializers and builders **fail fast** — the first violation short-circuits, so
`fieldList` carries one entry on the wire. The engine and `InputConstraintViolations`
are *capable* of full aggregation, but the **default walk preserves fail-fast wire
semantics byte-for-byte** (the differential fuzzer enforces this). Multi-violation
collection — which RFC-0032 (Accepted) desires but flags as a DoS vector needing
bounds — becomes a deliberate, flagged behavior change coordinated with RFC-0032's
implementation status, not a side effect of this rewrite.

- A missing required member is a **client validation error** feeding `fieldList` — never
  an internal failure and never a panic. The second-pass `unwrap()`s are provably dead
  after the first pass; if a codegen bug ever violates that, the failure mode must be an
  `InternalFailure` response for that request, not a process abort — builders therefore
  generate the defensive arm:

  ```rust
  let Some(a) = self.a else { return Err(internal_invariant("a")) };
  ```

  (One perfectly-predicted, never-taken branch per field; `unsafe`/`unwrap_unchecked` is
  rejected — its failure mode is UB, and benchmarks must first prove the checked form
  costs anything.)
- smithy-java's required-member bitfield (`PresenceTracker`,
  `requiredStructureMemberBitfield`, `validateRequiredMembers`) is **not** ported to the
  generated path: it is a workaround for Java's inability to express presence in the type
  of a primitive field. Rust's `Option` is that expression; niche optimization makes the
  `is_none` checks loads-and-compares; dual bookkeeping (bits + options) would reintroduce
  an invariant that can desynchronize.

### 5. Runtime constraint validation: one engine, two front-ends

The engine is a set of plain, `#[inline]`, monomorphizable checker functions in a runtime
crate, taking values and constraint parameters — never schemas:

```rust
pub fn check_length(len: usize, min: Option<usize>, max: Option<usize>,
                    path: &Path, out: &mut InputConstraintViolations);
pub fn check_range_i64(...); pub fn check_range_f64(...);
pub fn check_pattern(s: &str, re: &Regex, ...);
pub fn check_unique_items(...); pub fn check_enum(...);
```

- **Generated front-end**: shapes call checkers with **literal constants**
  (`check_length(s.chars().count(), Some(1), Some(64), ...)`); LLVM const-folds the
  `Option`s away, yielding the branch structure of today's hand-generated `TryFrom` with
  zero runtime schema/trait lookups. `@pattern` compiles to a per-shape
  `static RE: LazyLock<Regex>`. This deliberately rejects smithy-java's
  `ValidationState` bitflag digestion (`stringValidationFlags`, pre-extracted
  min/max on `Schema`): that is construction-time work compensating for the absence of
  monomorphization and const folding, unnecessary in Rust. Per the crate placement
  invariant, no typed constraint traits are added to `aws-smithy-schema`: the dynamic
  path (P2) reads `@length`/`@range`/`@pattern`/`@uniqueItems`/`@required` off schema
  trait maps through server-owned `DocumentTrait`-parsing helpers in
  `aws-smithy-http-server` (compiled patterns cached per schema at startup, not per
  request). Generated hot paths never read schema traits at request time regardless.
- **Dynamic front-end**: a validation walker implementing the `ShapeSerializer` interface
  (smithy-java's `Validator`-implements-`Serializer` trick), so validation is a
  serialization walk over any `SerializableStruct`: `input.serialize_members(&mut validator)`
  recurses via `write_struct`. It reads constraint parameters off `Schema<'a>` trait maps
  (paying the lookup the generated path avoids) and calls the same checkers. Entry point
  for documents is smithy-rs#4721's `deserialize_document`. On this path only,
  **`PresenceTracker` is ported** (bitset over schema member indices; single `u64` for
  ≤64 required members, bitvec fallback above): `set_member_value(schema, value)` has no
  typed `Option` fields, so out-of-band presence tracking is the minimal representation.
- **Semantics and messages are frozen to current behavior**: `@length` on strings counts
  code points; `@uniqueItems` uses structural equality on documents; float/int `@range`
  edge cases match current generated code even where it diverges from the spec (spec
  fixes are separate, flagged changes). Message templates live in exactly one place — the
  `InputConstraintViolations` renderer — and reproduce current strings and ordering byte-for-byte.
- Validation runs on the **deserialize/request path only**. Output correctness is
  structural (a response struct cannot be built without its required members); adding
  serialize-time constraint checks would be a new failure mode, not a compat-preserving
  change. (Optional debug-mode response validation is future work the walker enables for
  free.)
- **Recursive shapes**: schema consts for cycles use `LazyLock<Schema<'static>>`
  indirection, with codegen detecting cycles from the model's topology. smithy-java's
  `DeferredMemberSchema` bugs (member bitmask/recursion-detection pollution, per their
  release notes) are the cautionary tale: recursive shapes get dedicated tests for
  presence and constraint validation from day one.

- **Cost when unused is zero (generated path) or one static branch (dynamic path).**
  Operations without required members or constraint traits pay nothing on the generated
  path: codegen emits no `is_none` checks and no checker calls — absence of code, not a
  runtime skip. On the dynamic path, `PresenceTracker::of(schema)` returns a shared no-op
  instance when `required_member_count == 0` (one integer compare at construction,
  smithy-java's `NoOpPresenceTracker` pattern), and `required_member_count` is computed
  once at schema build (const for generated schemas, startup for runtime-constructed
  ones), never per request. If profiling ever shows the constraint walk itself mattering
  for unconstrained closures, a per-schema `has_constraints_in_closure` boolean computed
  at schema build is the escape valve — inside the schema, not new API surface. No
  per-operation marker trait is introduced for this: "has required members" is a
  per-structure property already encoded in schemas and generated code, and a parallel
  trait would be a second source of truth to desynchronize.

### 5a. Dynamic servers and the `DynamicShapeBuilder` capability trait

The end state includes servers constructed at runtime from a Smithy model (no codegen).
The dynamic deserialize/validation machinery therefore cannot assume typed builders.
Rather than smithy-java's approach — a default `setMemberValue` on every builder that
throws at runtime when unsupported — support is expressed as a **capability trait** so the
compiler enforces it:

```rust
/// Capability: this builder can be driven by schema-directed member setting.
pub trait DynamicShapeBuilder: ShapeBuilder {
    fn set_member_value(&mut self, member: &Schema<'_>, value: /* carrier */) -> Result<(), ...>;
    // presence access as needed by the walker to emit Missing violations
}
```

The dynamic machinery bounds on `DynamicShapeBuilder` (object-safe: runtime servers
dispatch through `ShapeId → builder-factory` maps). Handing it a builder compiled without
the capability is a **compile error**, not a runtime throw. Consumer profiles:

- **Pure codegen server** (default): a codegen flag (working name
  `generateDynamicBuilders`, default off) is not set; no impls are emitted; zero cost;
  accidental wiring into the dynamic path fails to compile.
- **Fully runtime server**: no generated builders exist; the runtime provides one
  hand-written document-backed builder implementing `DynamicShapeBuilder`, using
  `PresenceTracker` for presence. Schemas built from the model at startup carry member
  indices and `required_member_count` computed then.
- **Hybrid** (proxy over generated types, tooling): flag on for crates whose types must be
  dynamically drivable.

Even with the flag on, **generated builders do not gain a bitmask**: their generated
`set_member_value` is a `match member.member_index()` setting the corresponding `Option`
field, so the `Option`s remain the single source of truth and `build()` is unchanged. The
flag's cost is one extra generated method per shape (plus the per-member value-conversion
glue, which reuses the document coercion rules so dynamic and typed deserialization cannot
disagree). The `PresenceTracker` bitset remains confined to builders with no typed fields.

### 6. Unified `InputConstraintViolations` and the fate of per-shape `ConstraintViolation` enums

```rust
pub struct InputConstraintViolations(Vec<Violation>);            // no allocation until first violation
pub struct Violation { path: Path, kind: ViolationKind }
pub enum ViolationKind {
    Missing,
    Length { len: usize, min: Option<usize>, max: Option<usize> },
    Range { .. }, Pattern { .. }, EnumValue { .. }, UniqueItems { .. },
}
pub enum PathSeg { Member(&'static str), Index(usize), Key(String) }
```

Paths are values; nesting is push/pop during the walk, not enum wrapping; aggregation
across siblings and levels is natural *as an engine capability* (the wire default remains
fail-fast per section 4); message formatting is lazy (cold path only); generated builders
push `&'static str` member names (zero-copy).

Compatibility split, confirmed against current codegen (`ConstraintViolationSymbolProvider`,
`PubCrateConstraintViolationSymbolProvider`):

- **`publicConstrainedTypes=true`**: newtypes (`pub struct Name(String)`), their
  `TryFrom` signatures, and the public per-shape `ConstraintViolation` enums are
  contractual and are kept generated with identical shape. Their bodies are gutted to
  delegate to the shared checkers, and each enum gains `From<...> for Violation` to fold
  into the unified pipeline. Recursive public enums keep their `Box`ed variants
  (`RecursiveConstraintViolationBoxer` output is published API).
- **`publicConstrainedTypes=false`**: the enums are `pub(crate)` in `_internal` modules —
  not API. The entire per-shape enum + internal constrained-wrapper apparatus is deleted;
  the deserializer/walker emits `InputConstraintViolations` directly. This is where the bulk of the
  generated-code reduction lands, and it is compat-safe because wire messages/ordering are
  frozen (above).

### 7. Interactions with smithy-rs#4721 (Document Types and Type Registries)

- **Lifetimes**: `ModeledError`/`HttpModeledError`, `resolve_status`, the validation walker,
  and `PresenceTracker` are written against `&Schema<'_>` / generic `'a`. Checker
  functions are unaffected (they never see schemas). Generated code is unaffected
  (generated schemas remain `'static`).
- **Document rules**: presence bitsets index by schema `member_index`, never by the
  insertion order of the now-ordered `DocumentObject`; all `match`es on the
  `#[non_exhaustive]` `Document` carry wildcard arms; dynamic-path validation runs
  **post-coercion** (JSON base64-string→blob, string/number→timestamp per
  `DocumentSettings`), so `@length` on a blob checks bytes, not base64 text.
- **Protocol scope**: the document-driven dynamic path is JSON/CBOR-first, inheriting
  #4721's boundaries (CBOR documents pending spec; XML rejects documents).
- **Vocabulary**: this RFC adopts #4721's names (`TypeRegistry`, `error_registry`,
  `entry_for_error_code`) wherever registries are discussed, rather than smithy-java's.

### 8. Phasing

**Phase 1 targets post-#4721 `main` and depends on it.** #4721 is expected to merge
imminently; building against pre-#4721 signatures would absorb its churn as a mid-phase
rebase rather than targeting the settled surface. The dependency is on #4721's **types,
traits, and codecs** — not its dynamic machinery. Concretely, P1:

- pins a minimum `aws-smithy-schema` version containing #4721;
- bounds `ModeledError: SerializableStruct` and defines `schema() -> &Schema<'_>`
  against the post-#4721 lifetime'd signatures, frozen for all phases;
- has codegen emit `serialize_members` impls and `Schema<'static>` consts per #4721
  conventions;
- indexes member bookkeeping by schema `member_index`, uses wildcard arms on
  `#[non_exhaustive]` `Document` matches, and adopts #4721 vocabulary throughout.

What P1 explicitly defers: anything consuming `Document` values **at request time** —
`deserialize_document`, document-backed builders, `DynamicShapeBuilder`,
`PresenceTracker`, post-coercion validation, `DocumentTrait` constraint parsing, and
schema-read status resolution (P1 statuses are codegen-baked literals). Registries are
untouched by this RFC in every phase (client-side machinery).

**Phase 1 — errors and validation on the generated path:**
- `ModeledError`/`HttpModeledError` (with `schema()`), blanket `IntoResponse`,
  `serialize_error` seam, unmodeled funnel, framework errors re-based onto modeled
  shapes (section 2); error bodies per the P1 fork in section 2.
- ValidationException framework ownership: detect-if-declared default, opt-out flag,
  `@validationException` trait and mapping (section 3).
- Middleware error tiers (all three) — none require registries or documents.
- Checker functions, `InputConstraintViolations`, frozen message renderer, `ValidationErrorFactory`;
  generated builders move to two-pass `Option`-based `build()`; both
  `publicConstrainedTypes` modes migrated (sections 4-6).
- Full compatibility test suite and performance harness; these gate the phase.

**Phase 2 — dynamic front-end:**
- Validation walker over `SerializableStruct`/schemas; `PresenceTracker`; post-coercion
  document validation; server-owned `resolve_status` and `DocumentTrait` constraint
  readers; model-published injection projection for ValidationException consumed by
  client pipelines.
- `DynamicShapeBuilder` capability trait and the `generateDynamicBuilders` codegen flag
  (section 5a), including the `Document`-carrier conversion glue reusing #4721 coercion.
- If P1 chose fork (ii), the schema-driven codec flip for error bodies lands here.

**Phase 3 — fully runtime servers:**
- Runtime-constructed schemas from a Smithy model at startup; the hand-written
  document-backed builder; proxy-style serving. CBOR document semantics as the upstream
  spec lands; optional debug-mode response validation.

Each phase is independently shippable and independently gated on the byte-identical wire
tests; no phase changes the on-wire behavior of services built under an earlier phase.
**Schedule risk**: P1 carries one external dependency — #4721 merged and released; if it
slips materially, P1 falls back to today's generated protocol serializers for error
bodies (fork (ii)) with traits defined but unbounded, and the bound lands with the first
post-#4721 release.

### 8a. Differential fuzzing as the compatibility backstop

The repo's `aws-smithy-fuzz` tooling performs **differential fuzzing between two
smithy-rs server versions**: each revision's generated server builds as a `cdylib`, both
are dynamically linked into one AFL-driven harness seeded from a model-derived
`lexicon.json`, and responses are diffed. This is the RFC's compatibility claim as an
executable oracle: pre-RFC codegen vs. P1 codegen, same model, same request → identical
response (status, headers, body).

- **Division of labor**: the hand-written byte-identical suite remains the deterministic
  merge gate that names the contract explicitly; differential fuzzing is the coverage
  backstop for everything nobody thought to pin. AFL's bias toward malformed input
  drives traffic into precisely the surfaces P1 rewrites — constraint rejection,
  required-member misses, and the malformed-request funnel.
- **Runs**: one fuzz workspace per protocol (restJson1, awsJson 1.0/1.1, restXml,
  rpcv2Cbor), seeded with constraint-heavy models: the existing `constraints` test model
  plus models exercising custom `@validationException` shapes, middleware errors, and
  event-stream operations (initial-response path).
- **Triage rule**: differential fuzzing proves old-vs-new *equivalence*, not spec
  correctness — a divergence is triaged as an unintended regression by default; "old
  behavior was wrong, accept new" requires an explicit decision recorded against this
  RFC, never a silent test update.
- This backstop is also the counterweight to the no-fallback decision (section 2): with
  no legacy serializer to flip to, the widest practical discrepancy net runs before
  release.

### 9. Performance validation

Benchmarks are a merge gate, not an afterthought. Harness:

- **criterion** (wall time) on hot paths: deserialize + presence check;
  full validation walk (flat, nested struct, list-of-structs, worst-case `@pattern`);
  error serialization (modeled, framework, funnel).
- **iai-callgrind** (instruction/cache counts) on the same paths for CI stability.
- **dhat** (heap): the happy validation path allocates zero; violations may allocate
  (cold path).
- **Baseline**: current generated `TryFrom` chain on identical shapes. The runtime engine
  must match or beat it before landing; the checked-`else`-arm vs. `unwrap_unchecked`
  question is settled by these numbers (with the stated presumption for the checked form).
- CPU and memory results are recorded per release for regression tracking.

Changes checklist
-----------------

Items are tagged **[P1]**, **[P2]**, **[P3]** per the phasing section; untagged items in a
group inherit the group's tag.

**Crate placement & #4721 dependency (invariant)**
- [ ] No changes to `aws-smithy-types` or `aws-smithy-schema`; all runtime code lands as
      modules in `aws-smithy-http-server`
- [ ] **[P1]** Pin minimum `aws-smithy-schema` version containing #4721; build against
      post-#4721 `SerializableStruct`/`ShapeSerializer` signatures
- [ ] **[P1]** `ModeledError: SerializableStruct` bound + `schema() -> &Schema<'_>`,
      frozen for all phases; codegen returns `Schema<'static>` consts
- [ ] **[P1]** Codegen bakes resolved error status as a literal into each generated
      `HttpModeledError` impl (resolution rules applied at build time)
- [ ] **[P1]** Error bodies via `serialize_members` through schema-driven codecs, all
      protocols; legacy per-error serializer functions are not generated (no fallback
      path); repair doctrine: codec fixes for discrepancies, metadata extension for
      missing information
- [ ] **[P2]** Server-owned `resolve_status(&Schema<'_>)` and constraint-trait readers
      over the `DocumentTrait` fallback (compiled patterns cached per schema at startup)

**Runtime (`aws-smithy-http-server` + validation crate) [P1 unless noted]**
- [ ] `ModeledError` and `HttpModeledError` traits; blanket `impl<P, E: HttpModeledError>
      IntoResponse<P> for E`
- [ ] `ServerProtocol` trait on the five protocol markers: `serialize_error<E:
      HttpModeledError>` / `serialize_output(..., is_error)` / `Codec` associated type
      (single trait — no `Inner`/object-safe split; see §2b)
- [ ] Unmodeled-error funnel: serde failure → malformed-request shape; other →
      `InternalFailure` (existing shape) with generic wire message, cause logged
- [ ] Framework synthetic errors re-based onto modeled shapes; bespoke `IntoResponse`
      impls for them removed
- [ ] `MiddlewareError { status, message, source }` with hand-written `HttpModeledError` impl
- [ ] Checker functions (`check_length`, `check_range_*`, `check_pattern`,
      `check_unique_items`, `check_enum`) — zero-alloc happy path
- [ ] `InputConstraintViolations` / `Violation` / `ViolationKind` / `Path`; renderer with frozen message
      templates and ordering
- [ ] `ValidationErrorFactory` (InputConstraintViolations → effective validation shape)
- [ ] **[P2]** Validation walker implementing the serializer interface (dynamic front-end)
- [ ] **[P2]** `PresenceTracker` (u64 + bitvec fallback), dynamic path only, schema-member-indexed
- [ ] **[P2]** Post-coercion validation ordering on the document path; `#[non_exhaustive]`
      wildcard arms
- [ ] **[P2]** `DynamicShapeBuilder` capability trait; document-backed value conversion glue
- [ ] **[P3]** Hand-written document-backed builder for fully runtime servers
- [ ] **[P3]** Model→`RequestSpec` construction at startup for runtime servers (shared
      matching engine; no router API changes)

**Codegen (server) [P1 unless noted]**
- [ ] `HttpModeledError`/`ModeledError` impls for every `@error` shape (service, framework,
      middleware namespaces)
- [ ] Builders: two-pass aggregate-then-move `build()`; defensive `else` arm returning
      internal invariant error; no bitmask
- [ ] Generated constraint checks emit checker calls with literal constants;
      per-shape `static` compiled patterns
- [ ] `publicConstrainedTypes=true`: keep public newtypes/enums/`TryFrom` signatures;
      gut bodies to delegate; `From<enum> for Violation` bridges
- [ ] `publicConstrainedTypes=false`: delete `pub(crate)` enum + internal wrapper
      apparatus; emit `InputConstraintViolations` directly
- [ ] ValidationException: preserve existing `@validationException` + member-trait
      surface, validator rules/messages, auto-injection transformer, and deprecated-flag
      semantics; reimplement conversion generators over `InputConstraintViolations` with byte-identical
      output (existing generator output = extraction source); **[P2]** published-model
      injection reaching client pipelines
- [ ] Middleware codegen plugin surface: trait-driven layer application (e.g. `@awsauth`),
      error shapes generated from middleware Smithy files
- [ ] **[P2]** `generateDynamicBuilders` flag: `DynamicShapeBuilder` impls +
      `set_member_value` match per shape
- [ ] Recursive-shape `LazyLock` schema emission + cycle detection; dedicated recursive
      presence/validation tests

**Compatibility & tests**
- [ ] No changes to `Router`/`RoutingService`/`RequestSpec` public API; router-miss
      response re-based onto modeled shape with identical wire output (pinned by test)
- [ ] Event streams: exception-frame path untouched (pinned); event-stream-only error
      shapes implement `ModeledError` but not `HttpModeledError` (compile-fail test);
      event-stream constraint behavior frozen to **verified** current behavior per
      assumptions register A1-A4 (incl. `ignoreUnsupportedConstraints` and enum cases)
- [ ] Per-protocol discriminator emission extracted verbatim from current codegen and
      pinned (restJson1 name-only header; awsJson/rpcv2 `__type` forms)
- [ ] `@validationException` custom shapes emit own namespace (pinned); `MiddlewareError`
      fixed framework shape ID
- [ ] Server → #4721 tier-2 client discriminator round-trip test
- [ ] Differential fuzzing (`aws-smithy-fuzz`) pre-RFC vs. P1 codegen: per-protocol
      workspaces, constraint-heavy seed models, divergence-is-regression triage rule
- [ ] Fail-fast wire semantics preserved by default (single-entry `fieldList`); pinned
      by fuzzer and suite; collection is a separate flagged change per RFC-0032
- [ ] `ModeledErrorExtension` insertion and serialization-failure log+fallback preserved
      in the blanket impl path
- [ ] Byte-identical wire tests: validation messages, `fieldList` order, error bodies,
      status codes, across both `publicConstrainedTypes` modes
- [ ] Public-API diff (cargo-semver-checks / api-diff) on generated crates for both modes
- [ ] Protocol tests pass unmodified
- [ ] Semantic edge-case pins: code-point `@length`, structural `@uniqueItems`,
      float/int `@range` per current behavior

**Performance**
- [ ] criterion + iai-callgrind + dhat harness; baseline vs. runtime engine; merge gate
- [ ] Zero-alloc assertion on happy validation path

Relationship to prior RFCs
--------------------------

- **RFC-0025 (Constraint traits)** and the builders-of-builders implementation are the
  foundation this RFC rewrites the internals of; the public model (constrained types,
  builders, `publicConstrainedTypes`) they established is preserved per section 6.
- **RFC-0032 (Better Constraint Violations, Accepted)** identified three problems this
  RFC intersects:
  - *Collecting constraint violations*: 0032 wants collection; current behavior is
    fail-fast. This RFC ships an engine capable of collection but defaults to fail-fast
    wire semantics (section 4); enabling collection is a coordinated, flagged change
    honoring 0032's DoS-bounding concerns.
  - *Tightness / impossible constraint violations*: these concern the **public**
    per-shape `ConstraintViolation` enums, which this RFC freezes as thin shells with no
    deprecation intent — 0032's public-API concerns are neither worsened nor resolved
    here; internally, `InputConstraintViolations`-as-data sidesteps the impossible-variant
    problem entirely (kinds are only ever constructed when a violation occurs).
  - This RFC's checklist supersedes none of 0032's; where 0032's changeset lands later,
    it operates over the unified engine rather than per-shape enum internals.

Alternatives considered
-----------------------

- **`unwrap_unchecked` after presence check**: rejected as default; failure mode is UB
  rather than a 500, and the bitmask-free design makes the checked arm's cost a
  never-taken predicted branch. Revisitable if benchmarks demonstrate a measurable gap.
- **Porting smithy-java's required-member bitfield to generated builders**: rejected;
  dual sources of truth (bits + `Option`s) reintroduce a desynchronizable invariant that
  the `Option`-only design eliminates. Retained only on the dynamic path, where it is the
  minimal representation.
- **Porting `ValidationState` bitflag digestion to `Schema`**: rejected; monomorphization
  + const folding achieve the same hot-path cost at compile time with no runtime schema
  state.
- **Server-side error registry with runtime membership check**: rejected; Rust's closed
  enums and the `HttpModeledError` bound establish legitimacy statically, and middleware
  integration via `IntoResponse` requires no registration surface.
- **Validating on serialize**: rejected as default; new failure mode, not compat-preserving;
  structural guarantees already cover required members on outputs.
- **`@wireTypeId`-style discriminator override trait**: cut; declaring the error in the
  desired namespace (multi-file models + `use`) achieves the wire identity without
  splitting shape ID from wire name. Revisit only if an inexpressible case appears
  (e.g. per-mounting-service dynamic namespaces).
- **Placing new traits/helpers in `aws-smithy-schema`/`aws-smithy-types`**: rejected for
  this RFC; server-owned modules decouple the work from client-shared crates and #4721
  churn, at the cost of `DocumentTrait` parsing on the (already lookup-paying) dynamic
  path. Upstreaming later remains open.
- **`MaybeUninit`-based builder with whole-struct move**: deferred; `Option` niche
  optimization + elided unwraps likely capture the win without per-file `unsafe`; the
  benchmark harness is the arbiter.

Open questions
--------------

- Naming is settled: `ModeledError` (marker + `schema()`), `HttpModeledError`
  (`status_code()`), `InputConstraintViolations` (unified runtime container; element
  `Violation`, kind `ViolationKind`) — deliberately distinct from the legacy public
  per-shape `ConstraintViolation` enums retained under `publicConstrainedTypes=true`,
  which carry no deprecation intent (kept indefinitely as thin delegating shells; any
  future opt-out flag is a separate decision outside this RFC). Crate placement settled:
  modules in `aws-smithy-http-server`; `SerializableStruct` bound and `schema()`
  placement settled per section 2.
- Whether/when to upstream typed error and constraint traits into `aws-smithy-schema`
  once #4721 stabilizes — an internal simplification for the P2 helpers, never a
  dependency of this RFC.
- Exact flag name and scope for ValidationException opt-out, and the timeline for the
  public-SDK model-publishing pipeline to consume the injection projection.
- Whether the existing member-trait set (`@validationMessage`, `@validationFieldList`,
  `@validationFieldName`, `@validationFieldMessage`) needs extension (e.g. `reason`/`code`
  routing) — upstream's surface is adopted as-is for now.
- Deprecation horizon (if any) for public per-shape `ConstraintViolation` enums in
  `publicConstrainedTypes=true` behind a future flag.
- CBOR document validation semantics, pending the upstream spec referenced by #4721.
- Value carrier for `DynamicShapeBuilder::set_member_value` (note: this concerns the
  in-memory carrier type `aws_smithy_types::Document`, not document-typed model shapes):
  `Document` (reuses #4721 coercion, presumed default) vs. a leaner borrowed `Value<'_>`.
  **Confirmed deferred to Phase 2 design**, informed by Phase 1's benchmark harness.
- P2 discussion item: an inspection hook exposing `InputConstraintViolations` for
  rejected requests (observe/log before response serialization) — genuinely new user
  capability neither `publicConstrainedTypes` mode offers today; scope and API deferred.
