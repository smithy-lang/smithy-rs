# How to benchmark server routing

Compare CPU and memory costs of the generated servers in `~/smithy-rs-latest` with the schema-based multi-protocol server in this checkout. Run the commands below from this repository's root.

## What is compared

| Side | Checkout | Generated servers |
| --- | --- | --- |
| Baseline | `~/smithy-rs-latest` | Five separate servers, each serving one protocol |
| Current | This checkout | One schema-based server serving all five protocols |

Both use [the same model](tools/server-routing-benchmark/model.smithy), with eight operations. Requests invoke `Echo`, whose handler returns the supplied string. Preparation changes the protocol traits in retained copies of the model; it does not edit existing models in either checkout.

The protocols are REST JSON, REST XML, AWS JSON 1.0, AWS JSON 1.1, and RPC v2 CBOR. Default string sizes are 64, 1,024, and 16,384 bytes, excluding wire-format envelopes.

**Existing local changes in both checkouts are included.** This compares working trees, not necessarily clean commits. Each result records revisions, checkout status, tracked diffs, runtime source hashes, and binary hashes.

## Quick start: run again

The generated servers and binaries already exist locally. To measure them again:

```bash
python3 tools/server-routing-benchmark/run.py
```

This performs **no code generation or compilation**. It writes a new timestamped results directory under:

```text
tools/server-routing-benchmark/artifacts/results/
```

After changing runtime code or the Rust benchmark harness, rebuild and run:

```bash
python3 tools/server-routing-benchmark/build.py
python3 tools/server-routing-benchmark/run.py
```

`build.py` reuses the generated servers and Cargo's build cache. `run.py` rejects binaries when its recorded runtime or harness hash is stale.

## First-time setup

Requirements: Linux, Python 3, Git, `/usr/bin/time`, the repositories' Rust toolchain, and a JDK compatible with their Gradle builds. The original measurements used Rust 1.94.1 and Java 17. Both checkouts must remain available at their recorded paths.

```bash
python3 tools/server-routing-benchmark/prepare.py
python3 tools/server-routing-benchmark/build.py
python3 tools/server-routing-benchmark/run.py
```

The scripts have separate responsibilities:

| Script | Action |
| --- | --- |
| `prepare.py` | Generate and retain servers, or reuse completed generation |
| `build.py` | Build release executables for CPU measurement and separate allocation measurement |
| `run.py` | Execute existing binaries and save measurements |

Both sides use HTTP 1.x codegen. Current additionally enables `schemaSerde`. Each side builds against its own checkout's runtime and Tower dependencies.

## Preserve and refresh generated servers

The default cache is `tools/server-routing-benchmark/artifacts/`. It contains:

```text
artifacts/
  baseline/
    generated/<protocol>/rust-server-codegen/
    generation.json
    generation.diff
    build.json
    runner/                 # Harness source, Cargo.toml, Cargo.lock
    target-cpu/
    target-memory/
  current/
    generated/multi/rust-server-codegen/
    ...
  results/<timestamp>/
```

These artifacts are ignored by Git but persist across runs. **Keep this directory to avoid generating the servers again.** Runtime dependencies refer to the original checkouts; the cache does not snapshot those runtime sources.

Repeated `prepare.py` invocations reuse completed generation. They check the checkout path, model hash, and presence of generated manifests. They do not regenerate merely because the checkout's codegen implementation changed.

When intentionally testing new codegen or a changed model, use a fresh cache directory. Pass the same cache to all three scripts:

```bash
python3 tools/server-routing-benchmark/prepare.py \
  --baseline "$HOME/smithy-rs-latest" \
  --current "$PWD" \
  --cache "$PWD/tools/server-routing-benchmark/artifacts/experiment-2"

python3 tools/server-routing-benchmark/build.py \
  --cache "$PWD/tools/server-routing-benchmark/artifacts/experiment-2"

python3 tools/server-routing-benchmark/run.py \
  --cache "$PWD/tools/server-routing-benchmark/artifacts/experiment-2"
```

This preserves the earlier generated servers and results. If generated Rust or build settings are edited manually, rebuild explicitly; do not rely on the runtime/harness freshness check to detect every possible input change.

## Select cases and repeat measurements

Protocol CLI names:

| Protocol | Argument |
| --- | --- |
| REST JSON | `rest_json` |
| REST XML | `rest_xml` |
| AWS JSON 1.0 | `aws_json_10` |
| AWS JSON 1.1 | `aws_json_11` |
| RPC v2 CBOR | `rpc_v2_cbor` |

For example, repeat the larger AWS JSON cases five times:

```bash
python3 tools/server-routing-benchmark/run.py \
  --protocols aws_json_10 aws_json_11 \
  --sizes 1024 16384 \
  --repeats 5
```

Use `--cpu N` to select an allowed logical CPU. By default, the runner picks the lowest CPU in its allowed affinity set. Use `--output PATH` for a named results directory; that directory must not already exist.

Run benchmarks without concurrent builds or other heavy workloads. The runner pins execution to one logical CPU but does not isolate the host or lock CPU frequency.

## What the measurements include

The harness measures an **in-process, sequential full request pipeline**:

1. Construct an HTTP request around prebuilt body bytes.
2. Clone the service and perform Tower's `oneshot` readiness/call sequence.
3. Route, read the body, deserialize, execute the handler, and serialize.
4. Collect the response body.

Service setup, payload construction, and Tokio runtime construction are outside measurement. Before measuring, the harness checks response status and the echoed value.

Requests use `http_body_util::Full<Bytes>`, so each implementation's body adaptation is included. There are no sockets, HTTP parsing, TLS, or added middleware. These results do not measure a running Hyper network server, concurrency scaling, or routing alone.

### CPU

- Release builds with the normal system allocator.
- One current-thread Tokio runtime.
- 300 ms warmup per process, followed by 40 batches calibrated to approximately 20 ms each.
- Process CPU time and elapsed wall time recorded separately, in nanoseconds per request.
- Three process runs per case by default, alternating which implementation runs first.
- Summary CPU time is the median of the process-run medians. Minimum and maximum run medians show variation; they are not confidence intervals.

### Memory

Allocation measurements use separately compiled executables so counting does not affect CPU timings. They measure 1,000 warmed requests once per side and case, regardless of `--repeats`.

| Metric | Meaning |
| --- | --- |
| `allocations_per_request` | Allocation and reallocation calls per request |
| `allocated_bytes_per_request` | Total requested allocation sizes per request, including reallocations |
| `peak_extra_live_heap_bytes` | Maximum tracked live heap increase during the measured batch, excluding preexisting state |
| `net_live_heap_bytes` | Tracked heap change remaining after the batch |
| `peak_rss_kib` | Whole-process peak resident memory from `/usr/bin/time` |

Allocation bytes measure churn, not simultaneous memory use. Heap tracking excludes allocator metadata and stack memory. RSS includes startup, warmup, executable pages, and shared libraries. The summary uses median peak RSS from the uninstrumented CPU processes; raw records also include the instrumented processes' RSS.

## Read the results

Each completed run writes:

| File | Contents |
| --- | --- |
| `summary.csv`, `summary.json` | Per-case CPU, wall time, allocation, heap, and RSS comparisons |
| `samples.jsonl` | Raw batches and memory observations |
| `metadata.json` | Machine, affinity, compiler, revisions, source/binary hashes, and configuration |
| `baseline.diff`, `current.diff` | Tracked changes relative to each checkout's HEAD |

`cpu_current_over_baseline` below 1 means current used less CPU. For example, 0.8 means 20% less CPU per request. `allocated_bytes_current_over_baseline` uses the same convention for allocation bytes.

Compare CPU, allocation count, allocated bytes, and peak live heap separately. Fewer allocations can still mean more total bytes or a higher peak. Do not interpret small RSS differences as per-request heap savings.

The initial comparison and follow-up are retained locally:

- [Combined report](tools/server-routing-benchmark/artifacts/results/comparison.txt)
- [Initial full matrix](tools/server-routing-benchmark/artifacts/results/20261001T003850Z/summary.csv)
- [AWS JSON follow-up](tools/server-routing-benchmark/artifacts/results/20261001T004126Z/summary.csv)

The combined report was assembled from those two runs; `run.py` creates individual run summaries, not an updated combined report.

## Limits and troubleshooting

- **Stale or missing binaries:** run `build.py`, then `run.py`.
- **Changed model, checkout path, or intended codegen revision:** use a new cache and run all three scripts.
- **Variable timings:** inspect the run ranges and raw samples, then repeat the affected cases. Large AWS JSON 1.0 timings varied in the initial measurements; its apparent CPU gain was inconclusive.
- **Model detail:** the input string is optional, though every benchmark request supplies it and the handler requires it. Baseline REST XML generation of a required input string failed to compile because `set_value` received `Option<String>`. Both sides use the same final optional-input model; neither generator was patched for this benchmark.
- **Attribution:** differences include runtime, serialization, body adaptation, service cloning, and dependency versions. They cannot be attributed solely to multi-protocol routing or `Arc` cloning.
- **Reproducibility:** generated sources and lockfiles are retained, but checkouts remain mutable. Preserve the recorded source state when reproducing an old result; a revision plus tracked diff alone does not archive untracked source files.
