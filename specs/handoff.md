# Handoff — mproto-schema-spike (written 2026-08-24, pre-context-clear)

**Read order for a fresh session: this file → `specs/plan.md` (the working
plan; its "STEP 4 STATUS (2026-08-24)" block under Checkpoint 4 is the
authoritative state + implementation-delta list) → `specs/checkpoint4-walkthrough.md`
(review material). Auto-memory `schema-spike-state` mirrors this.**

## State: Steps 0–4 ALL COMPLETE and COMMITTED (tree clean except this file)

Commits, newest first:

| Commit | Content |
|---|---|
| `5d6a4d635` | plan.md STEP 4 STATUS block + checkpoint4-walkthrough.md |
| `b7e9a0483` | Step 4.8b: event streams (`Marshaller<P>`/`Unmarshaller<P>`, runtime `event_bindings.rs`, `with_request_deserializer` seam) — Step 4 complete |
| `fd5ce1a00` | Step 4.8a: streaming-blob splice glue |
| `48fc38549` | Protocol-test parity (JSON codec server strictness, empty-body/Accept/payload rules, walker union strictness) |
| `49d24c172` | Steps 4.1–4.4/4.6/4.7: full closure, deserialize walker, generic IntoResponse, FromRequest flip, predicate |
| `93d690455` | Step 4.5 (2d seam) + 2e order deletion; http-0.x keeps pre-serialized form |
| `b342e96e4` | Checkpoint 3: runtime (already walked through with user) |

Flag-on crates are FULLY schema-served — no operation keeps a legacy path;
grep-proof holds (clean regen of `pokemon-service-server-sdk-schema` has zero
`protocol_serde`/`event_stream_serde`; multiprotocol per-protocol modules hold
only `operations` glue).

**Verified green** (as of `b7e9a0483`): all 33 generated http-1.x crates
`cargo test` (schema + legacy + multiprotocol + assumptions), including the
FULL Smithy protocol suites through the schema pipeline (rest_json 750/750,
json_rpc11 100/100, rpcv2Cbor 60/60, json_rpc10 44/44, constraints, pokemon);
37 wire captures; 10 error goldens (the two multi-member restJson1 error
goldens now compare parse-equal per 2e); 2×21 eventstream integration tests;
runtime unit suites (http-server 142, json 217); clippy clean on changed
runtime crates.

## IN FLIGHT at handoff: the full `:codegen-server:test` Kotlin suite

Running as a DETACHED OS process (nohup) because the harness kept killing
long background tasks:

- Log: `D:\smithy-rs\codegen-server-test-suite.log`
- Completion marker: `D:\smithy-rs\codegen-server-test-suite.done`
  (contains `EXIT:<code>` when finished; absent = still running)
- Check: marker exists? then `grep -E "BUILD (SUCCESSFUL|FAILED)|tests completed" codegen-server-test-suite.log`.
- It had been running >1h at handoff (normal-ish: many tests cargo-compile
  generated Rust). Java PIDs alive at handoff: daemon 36972 + workers.
- **GOTCHA (cost three failed attempts):** if a run is killed, a zombie
  Gradle *test-worker* JVM survives `./gradlew --stop` and holds
  `codegen-server\build\test-results\test\binary\output.bin`, making the next
  run fail in ~16s with "Unable to delete directory". Fix: find the java.exe
  with `-Dorg.gradle.internal.worker.tmpdir=...codegen-server...` via
  `Get-CimInstance Win32_Process`, `Stop-Process -Force` it, delete
  `codegen-server/build/test-results/test`, rerun.
- NEVER run gradle/cargo while this suite executes (memory rule
  `gradle-test-suite-isolation`); ZipException/lock failures during overlap =
  corrupted run, not real failures.
- Some Kotlin unit tests may assert OLD generated-code shapes (suite hadn't
  completed once since the Step-4 changes) — if it fails, expect assertion
  drift in codegen unit tests, not product bugs; fix the tests to the new
  shapes (mind `no-tautological-tests`).

## Next steps, in order

1. Verify the `:codegen-server:test` result (above); fix any Kotlin-test
   drift; commit.
2. **Checkpoint-4 walkthrough WITH THE USER** (plan principle 6 — do not skip):
   material ready in `specs/checkpoint4-walkthrough.md` (before/after generated
   code, every trait/impl, deletions, divergence-register additions, Step-5
   watch items).
3. Step 5 gates per plan.md. Missing verifications called out in the
   walkthrough doc: request ROUND-TRIP goldens across binding locations, and
   FRAME-LEVEL event-stream goldens legacy-vs-schema (incl. the cbor
   event-error `__type` divergence to pin). Wire captures may need re-pointing
   at the schema crate's full pipeline per plan. Benches LAST
   (`specs/bench-results-error-serde.md`, request-path pair added in plan).
4. Step 6 walkthrough doc.

## Key architecture facts (don't re-derive)

- Walker feeds the pre-existing `pub(crate) set_*` builder setters (the
  builder's unconstrained ingestion surface; principle 3 — validation unmoved,
  single top-level `build()`); parse symbols mirror legacy
  `returnSymbolToParseFn` (struct→Builder, aggregates→`XxxUnconstrained`,
  enums/constrained strings→String). `mem::take(&mut builder)` dance in FnMut.
- `ServerProtocol::with_request_deserializer` = callback seam
  (`deserialize_request` is a provided method over it); `deserialize_request`
  takes the OUTPUT schema (Accept validation, payload/@mediaType/event aware);
  `Codec + 'static`, `Self: 'static`.
- Event streams: runtime `protocol/event_bindings.rs` interprets frames off
  event-struct schemas (marshall_event / initial_message /
  EventFrameDeserializer); generated marshallers use `PhantomData<fn() -> P>`
  + manual Debug; receiver constructed as the MEMBER's resolved symbol (SigV4
  wrapper compat); event glue: receiver → try_recv_initial (on
  `P::FRAMES_INITIAL_MESSAGES` && has-non-stream-members) → prelude via
  with_request_deserializer (initial payload as "body" on RPC) →
  set_<stream>(receiver) → build(). A2 quirk: event ops' error enums override
  content-type with `P::EVENT_STREAM_HTTP_CONTENT_TYPE`.
- http-0.x fork keeps pre-serialized ConstraintViolation
  (`serverValidationExceptionErrorSerializer` survives for 0.x ONLY); http-1.x
  error closure schema-gen runs on EVERY crate (flag-off included).
- Constrained-newtype serialize exclusions DELETED — ServerSchemaGenerator
  unwraps every newtype position (`.0` / `as_str()`);
  `unsafeForSchemaSerialization` is gone; predicate = flag + http1.x, no
  carve-outs.
- JSON codec is server-strict (comma discipline, trailing garbage, float
  strings NaN/Inf only, unknown union keys error except `__type`,
  `strict_timestamp_format` on SERVER codecs only — clients stay lenient).
- `RuntimeType.protocolTest` is http-version aware (misc regression).

## Housekeeping / cruft

- Projection build dirs accumulate ORPHAN files across regens — clean the dir
  before eyeballing generated output; `assumptions_a1_enum` build-dir orphan is
  expected (registered only under `-P includeFailingAssumptionTests=true`).
- When sweeping crates, check `cargo test` EXIT CODES — grepping for
  "test result: FAILED" misses compile failures (this bit me once).
- Client `SchemaDecorator.kt` pokemon allowlist edit still TEMPORARY; stale
  `D:\smithy-rs-example-sdk\` outside the repo awaits user deletion.
- `codegen-server-test-suite.{log,done}` in the repo root are throwaway
  (gitignored? NO — do not commit them; delete after reading).
- Don't touch core `SchemaGenerator.kt` (imported pristine from #4721).
- JAVA_HOME = `$HOME/scoop/apps/corretto21-jdk/current`; `./gradlew` via bash;
  `generateSmithyBuild` needs `--rerun-tasks` when `-P modules` changes.
