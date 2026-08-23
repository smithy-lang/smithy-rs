# Handoff — schema-decoupled server work (state as of 2026-08-23)

Read this first in a fresh session, then `specs/rfc_schema_decoupled_server.md` (§2b
has the `ServerProtocol` trait definition; §2 carries an implementation correction on
the blanket `IntoResponse` impl) and `specs/assumptions_register.md` (all assumptions
verified — treat its verdicts as ground truth; F2 has an implementation follow-up
block recording the protocol-dependent member-order discovery).

## Commit map (branch `mproto-schema-spike`, working tree clean)

- `8403a69b6` upstream ask doc (`specs/upstream-ask-schema-generator.md`)
- `3f90feb58` ServerSchemaGenerator + `schema_serde` module refactor
- `3ad0fc895` P1 seam implementation (contains the original core-`SchemaGenerator`
  diff — the reference for the upstream ask)
- `71dc2d608` #4721 import • `999256c81` specs + scenario models

## What is DONE (all six handoff work items implemented and green)

1. **Server `SchemaDecorator` mirror** — `ServerSchemaDecorator`
   (`codegen-server/.../customizations/ServerSchemaDecorator.kt`, registered in
   `RustServerCodegenPlugin`): emits schema statics + `SCHEMA` const +
   `SerializableStruct` (serialize-only, via `ServerSchemaGenerator.renderSerializeOnly()`
   — a server-side serialize-only copy of core's `SchemaGenerator`; core `SchemaGenerator`
   and `ServerCodegenVisitor` are byte-identical to the #4721 import so the eventual
   rebase drops the imports cleanly) for every shape in the **error closure** (error
   shapes + transitively reachable structs/unions), rendered into a dedicated
   `schema_serde` module, one file per shape (`schema_serde/shape_<name>.rs`,
   mirroring the `protocol_serde` layout — shape modules stay readable), plus
   `ModeledError`/`HttpModeledError` impls with codegen-baked status literals for
   `@error` shapes. Gated to http 1.x runtimes. Constrained-string members
   (`publicConstrainedTypes=true` newtypes) serialize via `as_str()`; errors whose
   closure reaches any OTHER input-reachable constrained newtype (numbers, blobs,
   collections, strings inside lists/maps) are excluded wholesale so generated
   crates always compile (e.g. `ebs` — the constrained story is RFC §6 work).
2. **`ServerProtocol` trait** —
   `rust-runtime/aws-smithy-http-server/src/protocol/server_protocol.rs`: single
   trait (`type Codec`, `serialize_output(schema, output, is_error)`,
   `serialize_error<E: HttpModeledError + ?Sized>`), implemented on all five markers.
   `ModeledError`/`HttpModeledError` live in `src/modeled_error.rs`.
3. **Discriminator injection** — wrapper `SerializableStruct`s in
   `server_protocol.rs`; wire-verified forms: restJson1 header name-only;
   awsJson1.0 full ID **appended last**; awsJson1.1 name-only **appended last**
   (NOT prepended — legacy writes `__type` after the members); rpcv2Cbor full ID
   first map entry + `smithy-protocol: rpc-v2-cbor` header.
4. **`@httpHeader` error-member binding split** — `HeaderSplitter` in
   `server_protocol.rs` diverts header-bound top-level members to response headers
   (skip-empty-string rule preserved); active on REST protocols.
5. **Float formatting codec fix** — `aws-smithy-json/src/codec/serializer.rs`
   `write_float`/`write_double` now use `aws_smithy_types::primitive::Encoder`
   (ryu; f32 widened to f64 first) matching legacy bytes; the one codec unit-test
   pin was updated to the legacy-correct expectation.
6. **Golden tests** —
   `codegen-server-test/wire-capture/tests/schema_serde_goldens.rs` (harness MOVED
   from the build dir into the repo, own `[workspace]`, relative paths): 10
   byte-identity tests, all green — restJson1 (InvalidGreeting, ComplexError header
   split, empty-header skip, ValidationException `fieldList`-before-`message`),
   awsJson1.0/1.1 `__type` forms, rpcv2Cbor `__type`-first + `smithy-protocol`, and
   an explicit pin of the event-stream content-type divergence (A2 quirk). The
   original 37-capture suite also passes there (`tests/captures.rs`).

Verified green: `cargo test -p aws-smithy-json` / `-p aws-smithy-http-server` /
`-p aws-smithy-schema`; SchemaGeneratorTest; regenerated codegen-server-test
crates compile under `-D warnings`.

## Key discoveries made during implementation (recorded in specs)

- **Legacy error-body member order is protocol-dependent**: REST protocols sort
  document members by member name (`HttpTraitHttpBindingResolver.mappedBindings`
  `.sortedBy { memberName }`); RPC protocols use model order. `ServerSchemaGenerator` has
  a `serializeMemberOrder` override; `ServerSchemaDecorator` passes member-name
  order for `@error` shapes on restJson1/restXml. Multi-protocol services get
  byte-identity only on the primary protocol (order-only divergence elsewhere).
  Recorded in register F2 follow-up + RFC §2 note. (The `serializeMemberOrder`
  override and the constrained-string `as_str()` handling live in
  `ServerSchemaGenerator`, not in core.)
- **RFC §2's blanket `impl<P, E: HttpModeledError> IntoResponse<P> for E` is not
  implementable** — Rust coherence (no negative reasoning) makes it conflict with
  every generated `IntoResponse<P> for {Op}Output`. Correction recorded in the RFC:
  codegen must emit per-error-type `IntoResponse<P>` impls delegating to
  `serialize_error`. NOT YET IMPLEMENTED (see next steps).
- **restXml `serialize_error` deliberately diverges** (bare-`<Error>` legacy
  envelope is broken today — register B4/B6): serializes through the XML codec
  as-is, excluded from goldens. Explicit freeze-or-fix decision still open.
- Pre-existing gating bug fixed: `assumptions_d1_flag_false` (codegen aborts by
  design) and `assumptions_a1_enum` (generated crate doesn't compile) were in the
  DEFAULT codegen-test list; both now sit behind `-P includeFailingAssumptionTests=true`.

## Where things live

| Thing | Location |
|---|---|
| **Working checkout** | `D:\smithy-rs` on branch `mproto-schema-spike` |
| Fallback base branch | `fahadzub/mproto-clean` (head 999256c81) |
| PR #4721 reference checkout | worktree `D:\smithy-rs-pr4721` (head 043eae3c7, UNMERGED upstream — when it lands on main, rebase the spike and drop the imported copies) |
| Runtime seam | `rust-runtime/aws-smithy-http-server/src/{modeled_error.rs,protocol/server_protocol.rs}` |
| Server codegen | `codegen-server/.../customizations/ServerSchemaDecorator.kt` (renders via `extras` into `schema_serde/shape_*.rs`) + `codegen-server/.../generators/ServerSchemaGenerator.kt` (serialize-only copy; core `SchemaGenerator` + `ServerCodegenVisitor` untouched) |
| Golden + capture harness (committed now) | `codegen-server-test/wire-capture/` — `cargo test` from that dir; regenerate deps first (command in its Cargo.toml header) |
| Verified register | `specs/assumptions_register.md` |
| RFC | `specs/rfc_schema_decoupled_server.md` |
| Draft upstream bug (panic) | `specs/draft-issue-default-violating-constraint-panic.md` — ready to post; check smithy-rs#2134 first |

## Environment gotchas (unchanged)

- Gradle needs `JAVA_HOME` = scoop corretto21 — set in the bash login env, so run
  `./gradlew` via bash. PowerShell default Java is 8.
- `generateSmithyBuild` is NOT input-sensitive to `-P modules`: always pass
  `--rerun-tasks` when changing the module list.
- Generated workspace has `-D warnings`; crates outside the members list need
  `[workspace]` appended to cargo-check standalone (wire-capture already has it).

## IN FLIGHT — RESOLVED: run KILLED by user 2026-08-23 ~08:05

**The re-run described below was killed on user instruction before completing**
(all cargo + gradle processes, including daemons, terminated). There are NO
valid `:codegen-server:test` results — the 7:22:57 XMLs on disk are the
corrupted run's 174-failure garbage. A full clean re-run is still owed at some
point; candidate real failures to watch for remain the three named below.

## Original in-flight notes (historical)

- **Clean `:codegen-server:test` re-run** launched 2026-08-23 ~13:00 in the
  background (gradle daemon survives the session). The PREVIOUS run reported
  280 tests / 174 failed after 10h49m — that result is GARBAGE: the failures
  are `ZipException: invalid LOC header` and `NoClassDefFoundError` on testutil
  classes, i.e. classpath jars rebuilt underneath the running test JVM by this
  session's concurrent `compileKotlin` invocations (plus heavy CPU contention
  from parallel cargo workspace builds). Check the fresh result at
  `codegen-server/build/reports/tests/test/index.html` /
  `codegen-server/build/test-results/test/TEST-*.xml` (timestamps must be
  2026-08-23 13:00+). Candidate REAL failures to triage if they reproduce:
  `UserProvidedValidationExceptionDecoratorTest`, `EventStreamAcceptHeaderTest`,
  `PostprocessValidationExceptionNotAttachedErrorMessageDecoratorTest`
  (MultiVersionTestFailure / assertion errors, not zip corruption).
  **Rule learned: never run gradle compile/codegen tasks while
  `:codegen-server:test` is executing.**
- **2026-08-23 ~08:00 intervention**: the fresh run stalled at ~7:25 — its
  `cargo test --all-features` (shared workspace
  `~/.local/share/smithy-test-workspace`, crate `smithy-test17446620918955556225`,
  a `ServerProtocolTestGenerator` compile test) hung for 33+ min on crates.io
  downloads with both TCP connections in CloseWait (cargo's stall timeout never
  fired; zero CPU, no rustc children, held the build-dir lock). Killed the two
  cargo PIDs; the suite resumed immediately (next cargo spawned within a minute).
  **Expect exactly one spurious network-failure test** in the final report from
  that protocol-test compile at ~07:24–08:00 — re-run just that test to confirm
  it's clean before trusting a failure there.

## NEXT TASK (user-directed): benchmark schema-decoupled vs legacy error serialization

### Status 2026-08-23 (opt-in flip session)

- DONE (user-directed redesign): `schemaSerde` is now a REAL OPT-IN, mirroring
  `http-1x`: `DEFAULT_SCHEMA_SERDE = false` in `ServerRustSettings.kt`; enable
  per service with `"schemaSerde": true` in codegenConfig (http 1.x only).
- DONE (user-directed): **flag ON now also FLIPS THE SERVING PATH and drops the
  legacy error serializers.** In `ServerHttpBoundProtocolTraitImplGenerator`
  (`ServerHttpBoundProtocolGenerator.kt`): `operationServedBySchema(op)` —
  http1x + flag on + all op errors in `ServerSchemaDecorator.errorClosure` +
  NOT an event-stream op — switches the operation-error-enum `IntoResponse<P>`
  impl to a variant match delegating to `ServerProtocol::serialize_error`
  (ModeledErrorExtension still stamped; note: also stamped on the internal
  serialization-failure fallback, unlike legacy). The legacy
  `ser_*_http_error` fn is then never referenced, and lazy fn generation means
  it and the error payload serializers are NOT generated at all (per-protocol
  in multi-protocol crates — the compile-time win the user asked for).
  Ineligible ops (event-stream: A2 content-type quirk + frame marshallers need
  the payload serializers; ops with schema-excluded constrained errors, e.g.
  ebs) keep the full legacy path, so flag-on crates always compile.
  NOT yet schema-driven: input deser + output ser (serialize-only P1 scope) —
  per-protocol `protocol_serde` generation still runs for those; the "no
  protocol info at compile time" endgame needs schema-driven input/output.
- DONE: `codegen-server-test/build.gradle.kts` gained `schemaSerdeCodegenTests`:
  flag-ON http1x variants (`rest_json-schema`, `json_rpc10-schema`,
  `json_rpc11-schema`, `rpcv2Cbor-schema`, `constraints-schema`,
  `pokemon-service-server-sdk-schema`) of the models the harnesses use.
  Unsuffixed crates are now flag-OFF legacy-only.
- DONE: goldens rewritten as CRATE-PAIR comparisons (legacy crate
  `IntoResponse` vs schema crate `IntoResponse` — the real serving path on
  both sides). Eventstream golden: schema side still calls `serialize_error`
  directly (flag-on crates keep legacy IntoResponse for event-stream ops).
  Bench harness rewritten the same way (both sides consume their enum,
  symmetric iter_batched/pre-clone); crate pairs also give compile-time and
  binary-size comparisons. Regen commands updated in both Cargo.toml headers.
- DONE: **the flip is SINGLE-PROTOCOL ONLY** (`!isMultiProtocol` in the
  predicate). Two reasons, discovered when the multi-protocol pokemon crates
  failed to compile: (a) one baked `serialize_members` order can't match both
  protocols' legacy byte order; (b) the framework validation-rejection path
  (`impl From<ConstraintViolation> for RequestRejection`, NOT P1 scope)
  serializes ValidationException via the legacy per-protocol payload
  serializers. Multi-protocol flag-on crates still emit `schema_serde` +
  ModeledError impls, just serve legacy. Lift after RFC items "framework
  errors on modeled shapes" + the member-order decision.
- DONE (fix): the three validation decorators
  (`SmithyValidationExceptionDecorator`, `CustomValidationExceptionWithReason…`,
  `UserProvidedValidationException…`) referenced
  `ser_validation_exception_error` by RAW PATH, which only existed as a side
  effect of the legacy error pass. New shared helper
  `serverValidationExceptionErrorSerializer` (`ServerProtocol.kt`):
  single-protocol → `structuredDataSerializer().serverErrorSerializer(id)`
  RuntimeType (forces materialization in flag-on crates); multi-protocol →
  per-protocol raw path unchanged (the serializer generator would collide both
  protocols' differently-typed copies in shared `protocol_serde` — the
  String-vs-Vec<u8> E0308 that broke pokemon).
- VERIFIED GREEN: 14 modules regenerated; `rest_json-schema` has zero legacy
  `ser_*_http_error` except the 7 event-stream ops (deliberate carve-out), 41
  schema `IntoResponse` impls, validation serializer materialized; flag-off
  crates have no `schema_serde` (stale 07:00 leftovers were deleted from the
  build dir once — regen does not clean orphans). wire-capture: 37 captures +
  10 goldens ALL PASS (goldens now legacy-crate `IntoResponse` vs schema-crate
  `IntoResponse`, byte-identical).
- REMAINING (paused): benches compiled in background (task completed, output
  never verified) but NOT run; results doc not written; `:codegen-server:test`
  full run still owed (re-check `ProtocolSpecificModuleTest` + validation
  decorator Kotlin tests against the serializer-reference change). ALL paused
  behind the architecture discussion below.

## ⚠️ ARCHITECTURE DISCUSSION IN PROGRESS (2026-08-23, read before ANY more code)

The user stopped implementation mid-way through lifting the multi-protocol
restriction. **Core principle articulated by the user (now binding):**
generated types — errors included — and their generated serializers must be
100% protocol-free. All protocol knowledge (error headers, discriminators,
status placement, member order, HTTP itself) belongs to the runtime
`ServerProtocol`/codec. Anything baking protocol facts into generated code is
a design bug, even if it preserves bytes.

Five issues were enumerated; the user wants them discussed ONE AT A TIME.
State of each:

1. **Form of `IntoResponse` — DISCUSSED, proposal on the table (not yet
   explicitly approved).** `IntoResponse<P>`'s job: convert outputs, operation
   error enums, and runtime framework types into `http::Response` for
   protocol P; called by the runtime `Upgrade` plumbing on handler results.
   The flip's per-marker generated impls are protocol-generic in body
   (`match … => P.serialize_error(e)`), so they should collapse into ONE
   generated generic impl per error enum:
   `impl<P: ServerProtocol> IntoResponse<P> for {Op}Error` (local self type ⇒
   coherent; the RFC's rejected blanket impl was runtime-crate-side, which is
   the thing coherence forbids — "per-error-type" in the earlier correction
   was right, "per-marker" was over-specific). Mechanical prereq: a way to get
   a P value generically (`Default` on markers / `P::instance()` /
   associated-fn `serialize_error`). This erases ALL per-protocol error
   codegen; multi-protocol errors then need zero extra generated code.
2. **RFC ordering is WRONG — pending discussion.** The framework-error rebase
   (currently "next work item 3") is a PREREQUISITE of the flip, not later
   work. Target design: `RequestRejection::ConstraintViolation` carries
   `Box<dyn HttpModeledError + Send>` (serialize_error already takes
   `?Sized`/dyn by design) instead of pre-serialized per-codec bytes
   (String/Vec<u8>). Then: one protocol-free
   `From<ConstraintViolation> for ValidationException` conversion, serialization
   happens once at the protocol boundary, and the ENTIRE validation-serializer
   plumbing built this session (RuntimeType materialization, the
   multi-protocol raw-path branch, the renderScoped attempt) becomes
   deletable. ValidationException needs per-protocol generated code today ONLY
   because the legacy path eagerly pre-serializes — not because it's a
   framework shape. RFC must be corrected: dependency order + an explicit
   "generated serializers are protocol-free" principle.
3. **Member-order leak — pending discussion.** `serializeMemberOrder` bakes
   the REST binding-resolver sort (protocol knowledge!) into the generated
   `serialize_members`. Verified: `HttpBindingResolver.kt` mappedBindings
   sorts request+response+error bindings alike ⇒ same issue hits outputs in
   P2; inputs unaffected (parsing is order-insensitive); nested structs
   unaffected (model order everywhere). Options: (a) canonical model order
   everywhere + relax the byte gate to "top-level member order on REST bodies
   is parse-equal, everything else byte-exact" (JSON/CBOR declare order
   insignificant; every SDK parses order-insensitively; my recommendation);
   (b) codec-side buffer-and-sort by schema member name (keeps full byte
   identity, costs runtime buffering + codec complexity to reproduce a
   `mappedBindings` accident). User previously questioned why byte-identity
   for JSON at all — leaning (a), NOT yet decided.
4. **restXml — pending.** A generic `impl<P>` (issue 1) uniformly covers
   RestXml, implicitly taking the "fix-forward" branch of the still-open
   freeze-or-fix decision (register B4/B6; legacy bare-`<Error>` is broken).
   Needs an explicit decision; current code excludes restXml from the flip.
5. **Housekeeping — pending.** Revert the UNCOMMITTED in-progress
   multi-protocol work (see inventory below). The earlier "option 3 wrapper
   view" approval is SUPERSEDED by issues 1–3 (wrapper bakes protocol order
   into codegen — violates the principle).

### Uncommitted working-tree inventory (HEAD = 6f5241a82, nothing committed this session)

KEEP (the green opt-in flip state, exercised by the goldens):
- `ServerRustSettings.kt` — DEFAULT_SCHEMA_SERDE=false, opt-in docs.
- `ServerHttpBoundProtocolGenerator.kt` — schemaServedErrorClosure lazy +
  operationServedBySchema + per-marker schema IntoResponse branch (to be
  REPLACED by the generic impl of issue 1 once approved).
- 3 validation decorators + `serverValidationExceptionErrorSerializer` in
  `ServerProtocol.kt` (interim scaffolding; deleted by issue-2 rebase).
- `codegen-server-test/build.gradle.kts` schemaSerdeCodegenTests (6 `*-schema`
  projections), wire-capture goldens/Cargo.toml (crate-pair form),
  schema-serde-bench lib/benches/dhat/Cargo.toml (crate-pair form).
- `ServerSchemaDecorator.kt` — opt-in comment + errorSerializeOrder via helper
  (fine), BUT ALSO contains revert-items below.

REVERT (in-progress multi-protocol lift, UNCOMPILED since last kotlin build,
UNEXERCISED by any regen, and `ServerBuilderGenerator` would CRASH
multi-protocol codegen via checkNotNull because the visitor call site was
never updated to pass the renderer):
- `ServerSchemaGenerator.kt`: `renderErrorOrderView` + the
  membersOverride/accessor params on renderSerializableStruct.
- `ServerSchemaDecorator.kt`: view emission block in extras loop, companion
  NAME_SORTED/MODEL_ORDER_ERROR_VIEW + errorOrderViewName; keep
  isRestFamilyProtocol only if still referenced after revert (errorSerializeOrder
  uses it), and the renderModeledErrorImpls implTarget/schemaConst params can
  revert to the original single-target form.
- `ServerHttpBoundProtocolGenerator.kt`: useOrderView/view-path wrap logic,
  the lifted guard (restore `!codegenContext.isMultiProtocol`, keep or drop the
  added restXml exclusion per issue-4 outcome), shapeModuleName import.
- `ServerProtocolGeneration.kt`: `renderScoped`.
- `ServerBuilderGenerator.kt`: `protocolScopedRenderer` param + renderScoped
  usage in renderProtocolValidationConversions (restore
  rustCrate.withModule form).

### Verified-green snapshot (what the build dir + tests currently prove)

Generated build dir corresponds to the KEEP state (regen `bnw5sfrce`, BEFORE
the revert-items were written; kotlin edits after it never compiled/ran):
14 modules regenerated; `rest_json-schema`: zero legacy `ser_*_http_error`
except 7 event-stream ops (deliberate), 41 schema IntoResponse impls,
validation serializer materialized; flag-off crates schema_serde-free (stale
orphans deleted once by hand — regen does NOT clean orphans).
wire-capture: 37 captures + 10 goldens ALL GREEN (crate-pair byte-identity).
Bench harness compile task finished; output unverified. Benches never run.

### Facts established in discussion (don't re-derive)

- Multi-protocol routing is STATIC: router nests per-protocol monomorphized
  services; `SelectedProtocol` request extension exists
  (`routing/multi_protocol.rs:191`) but is NOT needed for serialization.
- Multi-protocol serde placement works via `ServerProtocolCodegenTransformer`
  (post-render path rewrite + inline-dep relocation w/ crate-wide dedup);
  validation From-impls render OUTSIDE it — that's why the RuntimeType fix
  collided both protocols' differently-typed serializer copies in shared
  `protocol_serde` (pokemon E0308 String vs Vec<u8>).
- `From<ConstraintViolation> for RequestRejection` is per-protocol ONLY
  because rejections carry pre-serialized bytes typed per codec.

### Resume protocol for next session

Continue the one-at-a-time discussion: get explicit sign-off on issue 1
(generic impl), then issues 2 (RFC reorder — then EDIT the RFC), 3
(member-order policy), 4 (restXml), 5 (execute reverts). Only then implement:
likely order = revert → RFC edits → issue-2 rebase (rejection carries dyn
HttpModeledError) → generic IntoResponse impl → re-run goldens (multi-protocol
pair per issue-3 gate decision) → benches.

## SUPERSEDED historical plan (bench-first; kept for context)

Goal per RFC §9 (perf is a merge gate): criterion wall-time + CPU + memory
comparison of the legacy generated error path vs the new schema-driven path.

Plan agreed with the user:

1. **Add a codegen flag** (working name `schemaSerde` or similar, in
   `ServerRustSettings.codegenConfig` alongside `publicConstrainedTypes` etc.)
   gating `ServerSchemaDecorator.extras` — flag ON emits the `schema_serde`
   module + ModeledError impls, OFF emits nothing (legacy-only crate). Decide
   the default (currently the decorator is unconditionally ON for http1x).
   Add two codegen-server-test projections of the same model (one per flag
   state) to generate the two SDK variants side by side.
2. **Note**: for pure RUNTIME benchmarks the flag is not strictly needed —
   every current crate contains BOTH paths (legacy `ser_*` fns + schema
   `serialize_error`), so a single crate can bench both. The flag matters for
   the second-order comparisons: compile time, binary size, and making sure
   the benched schema path can't accidentally lean on legacy code.
3. **Bench harness**: new crate (suggest `codegen-server-test/schema-serde-bench/`,
   committed like wire-capture, own `[workspace]`, path deps into the build
   dir). Criterion benches over the golden shapes/cases (reuse construction
   code from `wire-capture/tests/schema_serde_goldens.rs`):
   - restJson1 ValidationException (message + 1-entry fieldList) — the hot
     validation-rejection shape;
   - restJson1 ComplexError with @httpHeader member (header-split cost);
   - awsJson1.1 / rpcv2Cbor InvalidGreeting (discriminator wrapper cost);
   - legacy side = `IntoResponse::<P>::into_response(enum)`, schema side =
     `P.serialize_error(&err)` — bench full response assembly on both sides
     (same work: body + headers + status).
4. **CPU/memory tooling — Windows caveat**: RFC §9 names iai-callgrind, but
   valgrind does NOT run on Windows (this box). Use:
   - criterion for wall time (works everywhere);
   - `dhat` crate (pure Rust) for heap profiling / allocation counts — add a
     `#[global_allocator]` dhat harness binary; assert/record allocs per
     serialize on both paths;
   - CPU counters: either run iai-callgrind under WSL2 if available, or skip
     instruction counts on this box and record criterion + dhat only (note it
     in the results). `cargo bench` from bash.
5. Record results in `specs/` (e.g. `specs/bench-results-error-serde.md`) per
   the RFC's "results recorded per release" requirement.

Watch out: benches build against the generated build-dir crates — regenerate
first (command in wire-capture/Cargo.toml header) and do NOT run gradle while
benching.

## Next work items

1. **Codegen-emitted per-error-type `IntoResponse<P>` impls** delegating to
   `ServerProtocol::serialize_error` (the coherence-safe replacement for the RFC's
   blanket impl), then migrate the operation-error-enum `IntoResponse` to a
   variant-match over those — this is the actual "flip" that puts serialize_error
   on the serving path. Gate with the golden suite (extend it to router-driven
   end-to-end captures, not just direct-call comparisons).
2. **restXml decision**: freeze the broken bare-`<Error>` envelope or fix forward
   to the codec-driven body (current serialize_error behavior). Needs an explicit
   decision recorded against the RFC (register conclusion 3).
3. **Framework errors re-based onto modeled shapes** (unknown operation, malformed
   request, internal failure, ValidationException factory) — RFC §2/§3.
4. **Constraint-validation engine** (checkers, `InputConstraintViolations`, frozen
   message renderer, two-pass builders) — RFC §4–6; untouched so far.
5. Multi-protocol member-order divergence: decide whether order-only divergence on
   non-primary protocols is acceptable or needs per-protocol serialize paths.
6. Known limitation: schemas are generated only for the error closure; input/output
   shapes are out of scope of the P1 serialize-only pass, and error closures
   reaching non-string constrained newtypes (`publicConstrainedTypes=true`) are
   excluded entirely (see item 1 in "What is DONE") pending RFC §6.

## Facts that override intuition (from the register — don't re-derive)

- Fail-fast validation everywhere, `fieldList` always 1 entry; map-entry order is the
  one nondeterminism. restXml server error bodies are broken today (freeze = bug).
- Every event-stream error is ALSO an operation error (normalizer hoisting): §2b's
  "no-HTTP" bucket is empty; frame payload and HTTP body share one serializer. The
  pre-first-event HTTP error carries `Content-Type: application/vnd.amazon.eventstream`
  over a JSON body (goldens pin the divergence from the schema path).
- Framework validation path hard-codes `x-amzn-errortype: ValidationException` on
  restJson1 even with a custom @validationException shape.
- awsJson1.1 framework-error body is empty string; 1.0 is `{}`; rpcv2Cbor is `0xa0`;
  406/415 unreachable outside REST protocols.
- Known upstream panic: defaulted primitive whose default violates its own @range
  (draft issue ready).
