# Handoff — schema-decoupled server work (state as of 2026-08-22, evening)

Read this first in a fresh session, then `specs/rfc_schema_decoupled_server.md` (§2b
has the `ServerProtocol` trait definition; §2 carries an implementation correction on
the blanket `IntoResponse` impl) and `specs/assumptions_register.md` (all assumptions
verified — treat its verdicts as ground truth; F2 has an implementation follow-up
block recording the protocol-dependent member-order discovery).

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
