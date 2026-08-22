# Handoff — schema-decoupled server work (state as of 2026-08-22)

Read this first in a fresh session, then `specs/rfc_schema_decoupled_server.md` (§2b
has the new `ServerProtocol` trait definition) and `specs/assumptions_register.md`
(all assumptions verified — treat its verdicts as ground truth, they corrected
several RFC beliefs).

## Where things live

| Thing | Location |
|---|---|
| Main working branch | `fahadzub/mproto-clean` in `D:\smithy-rs` (head f97cba901) |
| **Spike branch — start work HERE** | `mproto-schema-spike`, worktree `D:\smithy-rs-schema-spike`, commit 91c37c45d = #4721 runtime+codegen subset imported and verified compiling |
| PR #4721 reference checkout | worktree `D:\smithy-rs-pr4721` (head 043eae3c7, UNMERGED upstream — do not merge into mproto-clean; when it lands on main, rebase the spike and drop the copied files) |
| Verified register | `specs/assumptions_register.md` |
| RFC | `specs/rfc_schema_decoupled_server.md` |
| Draft upstream bug (panic) | `specs/draft-issue-default-violating-constraint-panic.md` — ready to post; check smithy-rs#2134 first (related TODO) |
| Wire-capture golden harness (37 tests) | `codegen-server-test/build/smithyprojections/codegen-server-test/wire-capture` (build dir — regenerate-able; run `cargo test -p wire-capture -- --nocapture`) |
| F2 byte-diff spike crates | session scratchpad `f2-spike/{legacy,schema}` (temp dir — may be gone; trivially recreatable, see register F2) |
| Scenario models (A1/B5/D1/D3) | `codegen-server-test/custom-test-models/*.smithy` + entries in `codegen-server-test/build.gradle.kts` |

## ⚠ Uncommitted state on mproto-clean (`D:\smithy-rs`)

`specs/` (all docs), `codegen-server-test/custom-test-models/` (6 new models), and
`codegen-server-test/build.gradle.kts` (assumptionsVerificationTests +
failingAssumptionsTests blocks) are **not committed**. Commit them before anything
destructive. The failing scenarios are gated behind
`-P includeFailingAssumptionTests=true` so default builds/tests stay green.

## Environment gotchas

- Gradle needs `JAVA_HOME` = scoop corretto21 — already set in the bash login env, so
  run `./gradlew` via bash (or `make` in examples/). PowerShell default Java is 8.
- `generateSmithyBuild` is NOT input-sensitive to `-P modules`: always pass
  `--rerun-tasks` when changing the module list.
- Generated workspace has `-D warnings`; generated crates outside the members list
  need `[workspace]` appended to their Cargo.toml to cargo-check standalone.

## Next work items (in order — all de-risked by the register)

1. **Server `SchemaDecorator` mirror** (new, in codegen-server on the spike branch):
   register `SchemaGenerator` (already in codegen-core on the spike) for server
   shapes **including error shapes**. Member-write order MUST match today's
   `JsonSerializerGenerator` order (F2 proved byte-identity hinges on it, e.g.
   ValidationException writes `fieldList` before `message`).
2. **`ServerProtocol` trait** (RFC §2b has the full definition): implement on the five
   markers in `aws-smithy-http-server`. Single trait, associated `type Codec`, no
   Inner/object-safe split. Slots into the five `IntoResponse<P>` seams mapped in
   register E2.
3. **Discriminator injection**: wrapper `SerializableStruct` prepending synthetic
   `__type` member. Forms (wire-verified): restJson1 header name-only; awsJson1.0
   full `ns#Name`; awsJson1.1 name-only; rpcv2Cbor full `ns#Name` first map key.
4. **`@httpHeader` error-member binding split** (naive write_struct puts them in body).
5. **Float formatting codec fix** — upstream to #4721 ideally: `Display` vs legacy
   ryu drops `.0` on integral floats (`aws-smithy-json/src/codec/serializer.rs:393-429`).
6. **Golden tests**: old `IntoResponse` bytes vs `serialize_error` bytes, seeded from
   the wire-capture suite.

## Facts that override intuition (from the register — don't re-derive)

- Fail-fast validation everywhere, `fieldList` always 1 entry; map-entry order is the
  one nondeterminism. restXml server error bodies are broken today (freeze = bug).
- Every event-stream error is ALSO an operation error (normalizer hoisting): §2b's
  "no-HTTP" bucket is empty; frame payload and HTTP body share one serializer.
- Framework validation path hard-codes `x-amzn-errortype: ValidationException` on
  restJson1 even with a custom @validationException shape.
- awsJson1.1 framework-error body is empty string; 1.0 is `{}`; rpcv2Cbor is `0xa0`;
  406/415 unreachable outside REST protocols.
- Known upstream panic: defaulted primitive whose default violates its own @range
  (draft issue ready).
