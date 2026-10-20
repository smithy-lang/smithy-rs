# A/B fuzzing generated Smithy servers

Compare servers generated from two revisions using the same operations, handlers,
and fuzzed HTTP requests. The before side uses legacy, single-protocol codegen
from the explicitly selected `$BASELINE_REPO`. The after side normally uses this
working tree, including its uncommitted changes, with `schemaSerde: true`.
The default campaign isolates each protocol; combined protocol sets are explicit.

There are two execution modes:

| Mode | Execution | Current support |
| --- | --- | --- |
| In-process AFL | `aws-smithy-fuzz` loads separate `cdylib` targets and invokes their services directly | Implemented in `tools/multi-protocol-ab-fuzz/` |
| Live HTTP | Separate server processes listen on assigned ports; a network driver sends each request to both sides | `live.py` implements launch and comparison; see README.md and its module documentation for current limits |

The port allocation and `/__ab/build` endpoint below are the contract for the live
HTTP mode. The existing AFL scripts do not launch listeners or provide that
endpoint. Both modes need generation-time checks that verify the actual serde
implementation before accepting comparison results.

Do not run `aws-smithy-fuzz setup-smithy` for this workflow. It clones repositories
and can remove local Maven cache state. Use the Gradle-driven generation below.


## Start an automated AFL campaign

Run from this directory after selecting a clean baseline checkout:

```bash
export BASELINE_REPO=/path/to/clean/baseline/smithy-rs
bash fuzz.sh 900 2
```

`fuzz.sh` checks build prerequisites and the baseline, calls `generate.sh`,
then `init.sh`, runs AFL, and calls `replay.sh`. Generation or seed-comparison
failures stop the run before fuzzing. Existing AFL findings are preserved by
default. Each invocation regenerates SDKs and rebuilds targets so path dependencies
do not silently reuse stale sources. Duration applies to fuzzing, excluding setup.
Use `SUITES` and `CASES` to select a smaller run. `--prepared` skips generation
and initialization only when the caller has already validated those stages.

The detailed contracts below include verification and coverage work that the
launcher does not fully automate; see README.md for the current implementation.

## 1. Select and record the revisions

Run from the after checkout:

```bash
export AFTER_REPO="$(git rev-parse --show-toplevel)"
export BEFORE_REPO=/path/to/clean/baseline/smithy-rs
export COVERAGE_REPO=/path/to/optional/coverage/checkout
export FUZZ_ROOT="$AFTER_REPO/tools/multi-protocol-ab-fuzz/artifacts"
mkdir -p "$FUZZ_ROOT"

# Inspect local edits before updating the baseline.
git -C "$BEFORE_REPO" status --short
git -C "$BEFORE_REPO" pull --ff-only
```

Check the pull's exit status. A successful fetch does not mean the checkout was
updated. If local edits would be overwritten or the branch has diverged, stop
baseline generation; do not reset, discard, or silently stash those edits.
Preserve them and resolve the checkout separately, or create a clean temporary
worktree from the latest fetched upstream revision:

```bash
# Alternative when the normal baseline checkout cannot be updated safely.
# Use a new, unused path and record it as a temporary worktree.
git -C "$BEFORE_REPO" fetch origin
git -C "$BEFORE_REPO" worktree add --detach \
  "$FUZZ_ROOT/before-worktree" origin/main
export BEFORE_REPO="$FUZZ_ROOT/before-worktree"
```

The normal baseline must be clean and at the intended upstream commit. Using a
patched baseline is a separate, explicitly recorded experiment, not a comparison
against pristine upstream. Updating the baseline is an explicit operator action;
`fuzz.sh` does not fetch or pull it.

Do not patch the before generator or runtime to make comparisons agree. Handle
representation-only differences, such as JSON object order and equivalent CBOR
container encodings, in the fuzz comparison layer (`aws-smithy-fuzz`); use `fuzzgen`
for shared deterministic fixtures and handlers. Parsing, validation, timestamp,
and numeric behavior differences remain findings to investigate, not changes to
apply to the baseline or silently normalize away.

Record both commit IDs, working-tree status, the tracked diff, relevant untracked
source files, and their hashes. The after commit ID alone is insufficient because
the after revision normally includes local changes. Freeze or snapshot all source
and runtime inputs for a run: rebuilding against path dependencies after either
checkout changes creates a different experiment.

The existing scripts call these variables `CURRENT_REPO` and `BASELINE_REPO`:

```bash
export CURRENT_REPO="$AFTER_REPO"
export BASELINE_REPO="$BEFORE_REPO"
export AB_FUZZ_DIR="$AFTER_REPO/tools/multi-protocol-ab-fuzz"
```

## 2. Freeze a shared model corpus

Use models from these sources:

- This checkout's `codegen-core/common-test-models/`, including the multi-protocol
  and Pokémon services used by the existing generator.
- `$COVERAGE_REPO/coverage/models/`, which contains additional fixtures
  designed to exercise more codegen paths, including JSON, XML, CBOR, bindings,
  and event-stream cases.
- The imports and dependencies those fixtures require. Read
  `$COVERAGE_REPO/coverage/README.md` and the fixtures' `coverage-service`,
  `coverage-import`, `coverage-codegen`, and `coverage-transforms` directives.

Copy the selected model sources and imports into a run-specific input snapshot.
Generate both sides from that same snapshot. Record the selected service IDs,
source hashes, Smithy dependency versions, transformations, and exclusions.
Coverage models are input data; generate the before side with the before checkout,
not with the coverage checkout's generator or its existing generated SDKs.

Only run valid, executable fixtures as live or in-process server comparisons.
Expected-generation-failure fixtures and HTTP 0.x compatibility fixtures remain
separate codegen tests. More models improve coverage but do not establish that
all paths or combinations have executed; report measured coverage and gaps.

The before and after projected models intentionally differ in service protocol
annotations. Store both projected model hashes as well as the shared input-corpus
hash. Verify that their modeled operations, members, traits, constraints, and
handler behavior otherwise match, apart from documented shared adaptations.
Do not demand identical full projected-model hashes when protocol annotations
necessarily differ.

### Current corpus support and adaptations

`MultiProtocolAbFuzzHarnessTest` currently supports the `multiprotocol` and
`pokemon` suites. `generate.sh` sets `MP_FUZZ_MODELS` to this checkout's common test
models. It does not automatically discover the coverage repository or consume a
frozen external corpus. Adding those inputs requires extending the generator's
suite selection and model/import loading, and making the script accept that input
snapshot. Do not label a run as including coverage fixtures until that is done.

The current generator applies these shared adaptations to both sides:

- Excludes ordinary blob-streaming operations. Event-stream operations are included.
- Removes `Publish.topic` and `CapturePokemon.region` only for profiles serving
  AWS JSON; the capture URI becomes `/capture-pokemon-event`. REST-only and
  CBOR-only profiles retain these members.
- Preserves `Greet`'s explicit HTTP 201 status so status differences remain
  compatibility failures.

Record these limitations. Both targets must implement the same deterministic
handlers, including equivalent event-stream outputs and modeled errors. Unsupported
operations must be reported as excluded, not counted as equivalent behavior.

## 3. Generate legacy before servers and schema after servers

Generate HTTP 1.x SDKs only. The fuzz runtime uses `http` 1.x and `http-body` 1.0.

| Side | Codegen setting | Service protocol traits | Runtime dependencies |
| --- | --- | --- | --- |
| Before | `schemaSerde: false` | Exactly one selected protocol per generated server | Before checkout |
| After | `schemaSerde: true` | One protocol by default; explicit combined profiles supported | After checkout |

Put settings under `codegen`, not `codegenConfig`:

```json
{
  "codegen": {
    "http-1x": true,
    "schemaSerde": false
  }
}
```

Use the same configuration with `schemaSerde: true` for the after SDK. Each before
server supports only one protocol; do not apply multiple protocol traits and rely
on the old generator to select one. For an explicit combined profile, the after service declares its supported
protocols, for example:

```smithy
use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use aws.protocols#restJson1
use aws.protocols#restXml
use smithy.protocols#rpcv2Cbor

@awsJson1_0
@awsJson1_1
@restJson1
@restXml
@rpcv2Cbor
service Example {
    version: "1"
    operations: [MyOperation]
}
```

`MyOperation` represents an operation in the selected model, not an extra probe
operation. Generate each suite's after SDK once per supported protocol set and
reuse it across that suite's comparisons.

### Existing generation command

The current scripts generate both SDK sides and separate fuzz target packages:

```bash
cd "$AFTER_REPO"
export SUITES="multiprotocol pokemon"
export CASES="aws-json-10 aws-json-11 rest-json1 rest-xml rpcv2-cbor"
bash "$AB_FUZZ_DIR/generate.sh"
```

The script injects the generator test into the baseline's `fuzzgen` test source
set with `fuzzgen-extra-tests.init.gradle`; it does not need to edit baseline source
files. The opt-in gate is `MP_FUZZ_GENERATE=true`, with `MP_FUZZ_SIDE=single` or
`multi`. Both A/B harnesses are generated by the after checkout so they implement
the same handlers, while each links its respective SDK and runtime.

The generator explicitly sets `schemaSerde: false` for the baseline and `true`
for the after server. `generate.sh` rejects a dirty baseline before generating
anything. Inspect the effective generated configuration as well; do not assume
that a requested setting took effect. The script does not pull the baseline;
complete step 1 first.

Expected layout for each suite:

```text
$FUZZ_ROOT/gen/<suite>/<protocol>/schema-server/
# Additional servers for explicit combined variants:
$FUZZ_ROOT/gen/<suite>/multi-server/
$FUZZ_ROOT/gen/<suite>/multi-xml-server/
$FUZZ_ROOT/gen/<suite>/<protocol>/single-server/
$FUZZ_ROOT/gen/<suite>/<protocol>/ab-harness/single/
$FUZZ_ROOT/gen/<suite>/<protocol>/ab-harness/multi/
$FUZZ_ROOT/gen/<suite>/<protocol>/ab-harness/lexicon.json
$FUZZ_ROOT/gen/<suite>/rest-xml/ab-harness-xml/{single,multi,lexicon.json}
$FUZZ_ROOT/gen/revisions.txt
```

For combined generation (`MP_FUZZ_ISOLATE_PROTOCOLS=false`),
`multi-xml-server` omits restJson1. The `rest-xml-nojson` comparison uses it because
restJson1 can claim bodyless REST requests that the single-protocol XML server
would handle differently. Preserve both comparisons and their protocol sets in
reports. `CASES` selects initialization/runs; generation's protocol filter is
`MP_FUZZ_PROTOCOLS`, if a narrower generation run is needed.

## 4. Verify the generated serde implementation

Perform these checks on fresh generation outputs, before compiling or fuzzing:

1. Before: effective `schemaSerde` is false; the generated crate declares the
   legacy `protocol_serde` module and contains its operation serde implementation.
2. After: effective `schemaSerde` is true; generated operations use the schema
   serialization/deserialization path and declare the expected protocol set.
3. Validate the actual SDK source/module declarations and operation wiring against
   the settings. Folder existence alone is insufficient: a stale directory or
   unused module does not prove which implementation is compiled.
4. Verify both SDKs' `Cargo.toml` runtime paths, and check the resolved dependency
   graph with `cargo metadata --quiet`. Before must resolve its Smithy runtime
   crates from the before checkout; after must resolve them from the after checkout.
5. Reject missing, inconsistent, or stale metadata. Generate into a new output
   directory, or remove only the previous generated output before regeneration.

The scripts currently record revisions and status, but do not yet enforce this
complete build-verification contract. Add the checks to generation/build tooling;
until then, inspect and record the evidence manually before trusting results.

### Build manifest and live endpoint contract

Generate a `build-info.json` manifest from the verified source and effective
configuration, embed the same data into the harness binary at build time, and
retain it beside the artifact. Include at least:

```json
{
  "side": "before",
  "suite": "pokemon",
  "service": "pokemon#PokemonService",
  "generator_revision": "<git-sha>",
  "generator_working_tree_dirty": false,
  "generator_source_sha256": "<source-snapshot-hash>",
  "runtime_source_sha256": "<runtime-snapshot-hash>",
  "schema_serde": false,
  "protocol_serde_module": true,
  "protocols": ["aws.protocols#restJson1"],
  "model_source_sha256": "<shared-input-corpus-hash>",
  "projected_model_sha256": "<projection-hash>",
  "generated_sdk_sha256": "<sdk-source-hash>",
  "build_id": "<unique-build-id>"
}
```

Use the actual service ID from the model. Record build commands, toolchain, lockfile,
and final artifact hash in the accompanying run manifest. Compute the final binary
hash externally; do not try to embed a binary's own hash into itself. Ensure rebuilds
refresh embedded metadata whenever generator, model, SDK, or runtime inputs change.

For live HTTP mode, expose **`GET /__ab/build`** in the harness wrapper before the
Smithy protocol router. It returns the embedded JSON with HTTP 200 and
`Content-Type: application/json`, without protocol-specific headers. Do not add it
to the Smithy service: the probe should work identically across all protocols and
must not change the model or lexicon being compared. Exclude the management path
from differential fuzz inputs.

A running Rust binary cannot discover whether its original source tree contained
a folder. The endpoint reports facts established during generation/build, not a
runtime filesystem scan or a manually supplied `schema_serde` flag. Keeping those
facts in the binary also avoids reading metadata belonging to a newer build.

The launcher must query every process, match its build ID to the launch manifest,
validate its side, suite, model inputs, protocol set, and serde mode, and stop if
any check fails. Save the responses with run results. Probe again after a restart
or rebuild. In-process AFL should validate the same embedded identity through a
harness metadata interface; that interface also remains to be implemented. There
is no HTTP endpoint to query in the current shared-library workflow.

## 5. Live HTTP port allocation

For each suite, run one before process per protocol and one after process serving
all selected protocols. Increment before ports by **10** for each next protocol:

| Comparison | Before address | After address |
| --- | --- | --- |
| awsJson1_0 | `127.0.0.1:18000` | `127.0.0.1:18100` |
| awsJson1_1 | `127.0.0.1:18010` | `127.0.0.1:18100` |
| restJson1 | `127.0.0.1:18020` | `127.0.0.1:18100` |
| restXml | `127.0.0.1:18030` | `127.0.0.1:18100` |
| rpcv2Cbor | `127.0.0.1:18040` | `127.0.0.1:18100` |
| rest-xml-nojson | `127.0.0.1:18030` | `127.0.0.1:18110` |

The optional `18110` process serves the after protocol set without restJson1. Assign
another non-overlapping block to the next concurrent suite, for example add 1000
to every port. Record exact ports, PIDs, protocol sets, artifact paths, and build IDs
in the launch manifest. A port collision or a response from a previous run is an
error; do not silently compare against whichever process is already listening.

For running live wrappers, build identity can be inspected with:

```bash
curl --fail --silent --show-error http://127.0.0.1:18020/__ab/build
curl --fail --silent --show-error http://127.0.0.1:18100/__ab/build
```

The live driver must send the same method, request target, headers, and body to
each selected pair, allowing only documented transport differences such as the
connection destination. It must handle timeouts, crashes, streaming, repeatability,
and response normalization. Ports do not apply to in-process AFL, and the existing
`aws-smithy-fuzz` target-crate arguments cannot be replaced with HTTP URLs.

## 6. Initialize and replay the in-process AFL corpus

Verify prerequisites with `cargo afl --version`. Match cargo-afl to the `afl`
version in `rust-runtime/aws-smithy-fuzz/Cargo.lock`. `generate.sh` builds AFL++ under
`$FUZZ_ROOT/xdg` and installs the driver under `$FUZZ_ROOT/driver`, leaving the global
driver installation alone. `env.sh` selects the after checkout's pinned Rust
toolchain and configures the local AFL environment.

```bash
source "$AB_FUZZ_DIR/env.sh"
bash "$AB_FUZZ_DIR/init.sh"
```

`init.sh` force-rebuilds both target libraries, writes seeds, and compares the seed
corpus. Any initialization or seed-comparison failure produces a nonzero exit
status, including failures in background jobs. Inspect each case's
`initialize.log` and `corpus-diff.log` for the recorded evidence. Workspaces
are under `$FUZZ_ROOT/work-isolated-v1/<suite>/<case>/`.

The current comparison defaults are:

- `SMITHY_FUZZ_SEMANTIC_COMPARE=1`: compare deserialized input and observable
  responses, normalizing content length, JSON member order, CBOR encoding choices,
  and the order of differently named XML siblings. Event-stream frame order,
  typed headers, event/error names, and raw payload bytes remain significant.
  Recognized F1 corrupt event-stream rejection and F3 XML modeled event-error
  differences are diagnostic classifications only. They fail comparison just
  like other differences. F2 numeric spellings and lost XML handler dispatch
  remain regression cases. Shared fixtures in `known-divergences.json`,
  `xml-claim-divergences.json`, and `xml-event-error-divergences.json` are retained.
- Routing differences always fail, including rejected requests before a handler
  runs. `SMITHY_FUZZ_IGNORE_UNROUTED` has been retired.
- `AFL_IGNORE_SEED_PROBLEMS` is unset so failing seeds cannot be silently skipped.

Set `SMITHY_FUZZ_SEMANTIC_COMPARE` to the empty string for byte comparisons.
Record the chosen policy; normalized equality is not byte-for-byte equality.
Resolve reproducible seed mismatches before a
long campaign. Do not silently broaden normalization to hide a regression.

For a manual single-case replay after initialization:

```bash
cd "$FUZZ_ROOT/work-isolated-v1/pokemon/rest-json1"
aws-smithy-fuzz replay --corpus --json > replay-corpus.json
```

Use new workspaces for different model snapshots. After codegen changes regenerate
SDKs first; after runtime changes rebuild both targets. `init.sh` already uses
`--force-rebuild`. Its optional `FRESH=1` deletes existing corpus/output state, so
archive findings first if a fresh campaign is intended.

## 7. Run, replay, and report

Start with a bounded smoke campaign:

```bash
bash "$AB_FUZZ_DIR/fuzz.sh" --prepared 900 2
```

That is 15 minutes with two fuzzers **per suite/case**, not two fuzzers total. Keep
fuzzers multiplied by active cases below the available CPU budget to avoid false
hangs. For an eight-hour run of one case:

```bash
SUITES=pokemon CASES=rest-json1 bash "$AB_FUZZ_DIR/fuzz.sh" 28800 2
```

Scripts preserve AFL output for resuming. Recheck artifact identity and model
compatibility before resuming a campaign. Replay both crashes and hangs: repeated
mismatch checks can exceed AFL's timeout and be recorded as hangs.

```bash
bash "$AB_FUZZ_DIR/replay.sh"
python3 "$AB_FUZZ_DIR/abfuzz.py" cluster pokemon rest-json1 -v 3
```

A stable difference can be a panic, a hang, an input-deserialization mismatch, or
an observable response difference. Preserve the input and repeat it against the
recorded artifact pair before classifying the finding.

For each suite/protocol pair, collect:

- Generator/runtime revisions, dirty-state snapshots, model/configuration hashes,
  protocol sets, serde verification evidence, and artifact/build identities.
- Execution mode; for live HTTP, port/PID mapping and saved `/__ab/build` responses.
- Seed replay outcome, comparison/normalization policy, and explicit exclusions.
- Duration, fuzzer count, execution counts, corpus growth, crashes, hangs, and
  measured coverage. AFL bitmap coverage is not proof of full source-path coverage.
- Reproducible requests, both responses, and triage results for each distinct finding.

The AFL scripts write `fuzz.log`, `summary.log`, `replay.log`, and `replay.jsonl`
under each case's work directory. Preserve these alongside manifests and inputs.

## CI and remaining implementation work

CI may generate and compile small harnesses and run deterministic replay tests.
Long AFL campaigns remain explicit local work, gated by `MP_FUZZ_GENERATE=true`.

Before claiming the full workflow described here is automated, implement:

- Source/configuration
  verification that rejects the wrong serde implementation.
- Frozen corpus loading, coverage-fixture selection/imports, and shared adaptations.
- Build manifests and embedded identity for the in-process targets and live wrappers.
- Full live streaming and strict rejection comparison. `live.py` currently
  implements launch, readiness/identity checks, and seeded HTTP comparison,
  but labels differing 4xx responses `BOTH_REJECTED` without failing the run.

The commands above use the in-process tooling. Launch automation does not establish
that the complete verification and coverage contract has been implemented.

## Cleanup

Stop only the processes recorded for this run, save results, and remove temporary
worktrees before deleting their parent scratch directory. If step 1 created the
fallback worktree, remove only that worktree after its runtime paths are no longer
needed:

```bash
git -C "$BASELINE_REPO" worktree remove "$FUZZ_ROOT/before-worktree"
```

Never remove the baseline, after, or coverage checkout as
run cleanup. Delete generated artifacts and AFL state only after archiving the
findings you need. Do not commit generated SDKs, corpora, build caches, or run
output unless intentionally preserving a minimal regression fixture.

### Fuzzer compatibility contract

`aws-smithy-fuzz/src/semantic.rs` and the Python triage comparator own the
compatibility rules. Shared cases in `aws-smithy-fuzz/tests/compatibility.json`
exercise both implementations: JSON member order, CBOR map order, widths and
container lengths, XML structure member order, case-insensitive HTTP header
names, and valid Content-Length omission are accepted in semantic mode.
A supplied Content-Length must match its own body, even when encodings differ.
List order, JSON boolean/number types, float precision, timestamps, status,
handler invocation and parsed input differences remain failures. Recognized
regressions, routing differences, and nondeterministic mismatches cannot suppress
a failing comparison.
Do not make baseline serializers or parsers match the after server. Shared
fixtures and handlers belong in FuzzGen; comparison allowances belong here.

Run the shared contract and event-stream regression tests with:

```bash
cargo test --quiet --manifest-path rust-runtime/aws-smithy-fuzz/Cargo.toml
python3 -m unittest discover -s tools/multi-protocol-ab-fuzz -p 'test_*.py'
```


## Protocol-isolated campaigns

Each A/B case pins its own protocol identity before invoking either target. The driver
replaces Content-Type with the case's media type, removes foreign smithy-protocol and
Content-Encoding headers, and removes X-Amz-Target for non-AWS-JSON cases. Header
matching is case-insensitive and duplicate variants are removed. Body bytes, operation
targets for AWS JSON, paths, methods, Accept, and other headers remain fuzzable.
AWS JSON event streams use the versioned JSON Content-Type; CBOR event streams retain
the event-stream Content-Type and pin smithy-protocol to rpc-v2-cbor.

The fixed identity is recorded as `protocol` in smithy-fuzz-config.json. Native replay
inherits it; the Python replay tool explicitly selects the case's identity too. Saved
AFL files contain raw mutations: wire headers are normalized on invocation, so raw
request dumps may show a different Content-Type from the effective request.

New campaigns use work-isolated-v1/<suite>/<case>, with separate corpus, dictionaries,
AFL output, and workers for every case. Existing work/ campaigns are preserved but
must not be copied or synchronized into these directories. Default `fuzz.sh`
regenerates and initializes before starting the campaign.

Protocol-selection and malformed/missing selection-header cases belong in dedicated
routing tests as well as unmodified-request differential replay. Routing differences
remain failures; the retired `SMITHY_FUZZ_IGNORE_UNROUTED` flag cannot suppress them.


### Single-protocol targets on both sides

The isolated campaign generator now sets MP_FUZZ_ISOLATE_PROTOCOLS=true. Each case
compares a legacy single-protocol server with a schemaSerde single-protocol server.
The after server declares only that case's protocol, so an operation with no
content-type restriction cannot fall through to a different protocol. Per-protocol
after SDKs are generated under <suite>/<protocol>/schema-server. The historical
multi-protocol generator branch remains available for routing investigations, but
is not used by generate.sh. The default case list contains the five protocols;
the historical rest-xml-nojson comparison is no longer needed.
