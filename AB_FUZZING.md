# A/B fuzzing generated Smithy servers

This runbook describes how to generate and run a differential, A/B-style fuzz
campaign for generated smithy-rs servers. The setup compares two generated
servers, usually a "before" revision and an "after" revision, by sending the
same fuzzed HTTP request to both and treating stable response differences as
fuzzer findings.

The flow uses two pieces of tooling:

- `fuzzgen`: the Smithy build plugin that generates a lexicon and a small
  `cdylib` fuzz target crate for each generated server.
- `aws-smithy-fuzz`: the runtime driver that loads those target crates,
  deserializes AFL inputs into HTTP requests, invokes each target, and compares
  responses.

Do not run `aws-smithy-fuzz setup-smithy` for this workflow. It clones repos and
can remove local Maven cache state. Use the Gradle-driven flow below instead.

## What A/B fuzzing checks

For each AFL input, `aws-smithy-fuzz`:

1. Deserializes bytes into an `HttpRequest`.
2. Invokes every configured target shared library with the same request.
3. Compares the HTTP response from each target against the first target.
4. Re-runs mismatches to filter nondeterminism.
5. Panics on stable response differences, which AFL records as a crash.

With one target, this is a panic/hang fuzz test. With two targets, it is a
behavioral equivalence test.

## Prerequisites

Install or verify:

```bash
cargo afl --version
cargo afl install --path rust-runtime/aws-smithy-fuzz --force
```

Use the repository-pinned Rust toolchain when building fuzz targets:

```bash
export RUSTUP_TOOLCHAIN=1.94.1
```

Set AFL environment variables used on hosts where CPU frequency checks or
core-pattern handling would otherwise stop AFL:

```bash
export AFL_SKIP_CPUFREQ=1
export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
export AFL_NO_UI=1
```

If `cargo afl` fails to link because it cannot find `ld.gold`, put the
binutils directory containing `ld.gold` on `PATH`:

```bash
export PATH="$(dirname "$(ls /nix/store/*-binutils-*/bin/ld.gold 2>/dev/null | head -1)"):$PATH"
```

## Step 1: choose revisions and workspace paths

Use `/tmp` for generated servers, harnesses, and AFL output:

```bash
export FUZZ_ROOT=/tmp/smithy-ab-fuzz
export BEFORE_REPO=/tmp/smithy-rs-before
export AFTER_REPO=$PWD
export BASE_COMMIT=<base-commit-sha>
```

Create a worktree for the before revision:

```bash
git worktree add "$BEFORE_REPO" "$BASE_COMMIT"
```

The after revision is normally the current working tree.

## Step 2: generate the before and after servers

Generate HTTP 1.x servers only. `aws-smithy-fuzz` uses the `http` 1.x /
`http-body` 1.0 stack, so HTTP 0.x generated servers are not compatible with
this harness.

The local generator test is gated so CI does not run it by default. Enable it
explicitly:

```bash
export RPCV2_FUZZ_GENERATE=true
```

If the model service lives in an external Smithy artifact, inject that artifact
into the `fuzzgen` test classpath with an init script instead of editing
tracked Gradle files:

```bash
cat > /tmp/fuzzgen-protocol-tests.gradle <<'EOF'
allprojects {
    if (name == "fuzzgen") {
        afterEvaluate {
            dependencies {
                testImplementation "software.amazon.smithy:smithy-protocol-tests:1.73.0"
            }
        }
    }
}
EOF
```

Generate the after server:

```bash
cd "$AFTER_REPO"
RPCV2_FUZZ_OUTPUT="$FUZZ_ROOT/after" \
RPCV2_FUZZ_GENERATE=true \
  ./gradlew -I /tmp/fuzzgen-protocol-tests.gradle \
  :fuzzgen:test --tests '*RpcV2CborFuzzHarnessGenerationTest*' \
  --no-configuration-cache
```

Generate the before server from the base worktree:

```bash
cd "$BEFORE_REPO"
RPCV2_FUZZ_OUTPUT="$FUZZ_ROOT/before" \
RPCV2_FUZZ_GENERATE=true \
  ./gradlew -I /tmp/fuzzgen-protocol-tests.gradle \
  :fuzzgen:test --tests '*RpcV2CborFuzzHarnessGenerationTest*' \
  --no-configuration-cache
```

Expected outputs:

```text
$FUZZ_ROOT/before/server-http-1x/
$FUZZ_ROOT/after/server-http-1x/
```

Each generated server should point at the runtime in the repo that generated it.
Check the generated `Cargo.toml` files before continuing.

## Step 3: generate a two-target A/B harness

Run the harness generator from the after repo and pass both generated servers:

```bash
cd "$AFTER_REPO"
RPCV2_FUZZ_OUTPUT="$FUZZ_ROOT/ab" \
RPCV2_FUZZ_BEFORE_SERVER="$FUZZ_ROOT/before/server-http-1x" \
RPCV2_FUZZ_AFTER_SERVER="$FUZZ_ROOT/after/server-http-1x" \
RPCV2_FUZZ_GENERATE=true \
  ./gradlew -I /tmp/fuzzgen-protocol-tests.gradle \
  :fuzzgen:test --tests '*RpcV2CborFuzzHarnessGenerationTest*' \
  --no-configuration-cache
```

Expected outputs:

```text
$FUZZ_ROOT/ab/harness/before/
$FUZZ_ROOT/ab/harness/after/
$FUZZ_ROOT/ab/harness/lexicon.json
```

The `before` and `after` directories are separate Cargo packages that build
separate shared libraries. This avoids linking both runtime revisions into one
binary.

## Pokemon multi-protocol per-protocol A/B harnesses

To compare each single-protocol Pokemon server against the generated
multi-protocol Pokemon server, first generate the single-protocol side from a
clean smithy-rs checkout, not from the multi-protocol branch. Put those outputs
under a baseline root with this layout:

```bash
export FUZZ_ROOT=/tmp/smithy-pokemon-mp-fuzz
export BASELINE_ROOT=/tmp/smithy-pokemon-clean-baseline
export CLEAN_REPO=~/smithy-rs-clean
```

```text
$BASELINE_ROOT/aws-json-10/server-http-1x/
$BASELINE_ROOT/aws-json-11/server-http-1x/
$BASELINE_ROOT/rest-json1/server-http-1x/
$BASELINE_ROOT/rest-xml/server-http-1x/
$BASELINE_ROOT/rpcv2-cbor/server-http-1x/
```

Then use the opt-in fuzzgen test from the multi-protocol branch to generate the
multi-protocol side and the two-target A/B harnesses:

```bash
POKEMON_MP_FUZZ_GENERATE=true \
POKEMON_MP_FUZZ_OUTPUT="$FUZZ_ROOT" \
POKEMON_MP_FUZZ_BASELINE_ROOT="$BASELINE_ROOT" \
  ./gradlew :fuzzgen:test \
  --tests 'software.amazon.smithy.rust.codegen.fuzz.FuzzHarnessBuildPluginTest.generate Pokemon single protocol versus multi protocol fuzz harnesses' \
  --no-configuration-cache
```

This generates one A/B pair for each selected protocol:

```text
$FUZZ_ROOT/aws-json-10/multi-server-http-1x/
$FUZZ_ROOT/aws-json-10/harness/single/
$FUZZ_ROOT/aws-json-10/harness/multi/
$FUZZ_ROOT/aws-json-10/harness/lexicon.json

$FUZZ_ROOT/aws-json-11/...
$FUZZ_ROOT/rest-json1/...
$FUZZ_ROOT/rest-xml/...
$FUZZ_ROOT/rpcv2-cbor/...
```

Each `single` target points at the clean single-protocol server under
`$BASELINE_ROOT`. Each `multi` target points at a server generated from the
multi-protocol branch with all selected protocol traits applied. The common
Pokemon Smithy models in the multi-protocol branch are read unchanged; protocol
traits are adjusted only in local test models.

Then initialize and run one protocol at a time:

```bash
export PROTOCOL=rest-json1
mkdir -p "$FUZZ_ROOT/$PROTOCOL/work"
cd "$FUZZ_ROOT/$PROTOCOL/work"

aws-smithy-fuzz initialize \
  --lexicon "$FUZZ_ROOT/$PROTOCOL/harness/lexicon.json" \
  --target-crate "$FUZZ_ROOT/$PROTOCOL/harness/single" \
  --target-crate "$FUZZ_ROOT/$PROTOCOL/harness/multi" \
  --release \
  --force-rebuild

aws-smithy-fuzz replay --corpus --json > "$FUZZ_ROOT/$PROTOCOL/replay-corpus.json"
test -n "$(find afl-input/corpus -maxdepth 1 -type f -print -quit)" || \
  printf 'seed' > afl-input/corpus/seed
timeout -s INT 900 aws-smithy-fuzz fuzz --num-fuzzers 8 \
  > "$FUZZ_ROOT/$PROTOCOL/fuzz-smoke.log" 2>&1
```

Change `PROTOCOL` to `aws-json-10`, `aws-json-11`, `rest-json1`, `rest-xml`, or
`rpcv2-cbor` to run the corresponding single-vs-multi comparison.

## Step 4: initialize the fuzz workspace

Initialize from a clean work directory:

```bash
mkdir -p "$FUZZ_ROOT/work"
cd "$FUZZ_ROOT/work"

aws-smithy-fuzz initialize \
  --lexicon "$FUZZ_ROOT/ab/harness/lexicon.json" \
  --target-crate "$FUZZ_ROOT/ab/harness/before" \
  --target-crate "$FUZZ_ROOT/ab/harness/after" \
  --release \
  --force-rebuild
```

`initialize` writes:

```text
smithy-fuzz-config.json
afl-input/
target/fuzz-target-target/
```

Use `--force-rebuild` whenever either target server or runtime changed.
Otherwise an old shared library may be reused.

## Step 5: sanity-check with replay

Before running AFL, replay the generated corpus:

```bash
aws-smithy-fuzz replay --corpus --json > "$FUZZ_ROOT/replay-corpus.json"
```

For an A/B run, corpus replay should not produce stable response differences.
If it does, debug the generated servers before starting a long fuzz campaign.

## Step 6: run the A/B fuzz campaign

Run a time-bounded campaign. `timeout -s INT` lets AFL save state on exit.

```bash
cd "$FUZZ_ROOT/work"
timeout -s INT 28800 aws-smithy-fuzz fuzz --num-fuzzers 8 \
  > "$FUZZ_ROOT/fuzz-8h.log" 2>&1
```

That runs for 8 hours. Use a shorter timeout for smoke tests:

```bash
timeout -s INT 900 aws-smithy-fuzz fuzz --num-fuzzers 8
```

If you resume an existing AFL output directory, add:

```bash
export AFL_AUTORESUME=1
```

or AFL will refuse to overwrite old results.

## Step 7: collect results

Count saved findings:

```bash
find "$FUZZ_ROOT/work/afl-output" -path '*/crashes/id:*' -type f | wc -l
find "$FUZZ_ROOT/work/afl-output" -path '*/hangs/id:*' -type f | wc -l
```

Collect AFL stats:

```bash
find "$FUZZ_ROOT/work/afl-output" -name fuzzer_stats -print
for f in "$FUZZ_ROOT"/work/afl-output/fuzzer*/fuzzer_stats; do
    printf '%s ' "$(basename "$(dirname "$f")")"
    awk -F': ' '/run_time|execs_done|saved_crashes|saved_hangs|corpus_count|bitmap_cvg/ {
        printf "%s=%s ", $1, $2
    } END { print "" }' "$f"
done
```

For PR reporting, include:

- elapsed time
- number of fuzzers
- total or per-fuzzer `execs_done`
- `saved_crashes`
- `saved_hangs`
- corpus growth
- whether corpus replay passed

## Step 8: replay and triage crashes

Replay every saved crash:

```bash
cd "$FUZZ_ROOT/work"
aws-smithy-fuzz replay --json > "$FUZZ_ROOT/replay-crashes.json"
```

Replay a specific crash:

```bash
aws-smithy-fuzz replay --invoke-only afl-output/fuzzer0/crashes/id:000000,...
```

A crash in a two-target run means one of:

- one target panicked
- one target hung
- the before and after targets returned stable, different responses

Stable response divergence is usually a real compatibility finding, even for
unusual paths, malformed headers, or malformed bodies.

## CI expectations

CI should not run AFL campaigns. It may run `fuzzgen` smoke tests that generate
and `cargo check` small harness crates. Long-running fuzzing should stay local
and explicit.

For local-only harness generator tests, keep an explicit environment gate such
as:

```kotlin
@EnabledIfEnvironmentVariable(named = "RPCV2_FUZZ_GENERATE", matches = "true")
```

This prevents CI from requiring external protocol-test artifacts or starting
local fuzz generation paths accidentally.

## Cleanup

Remove generated scratch state when done:

```bash
rm -rf "$FUZZ_ROOT"
git worktree remove "$BEFORE_REPO"
```

Do not commit generated AFL work directories, corpus growth, `target/`, or
temporary harness output unless the team explicitly decides to check in a
minimal corpus.
