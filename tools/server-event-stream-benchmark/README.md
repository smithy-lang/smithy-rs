# Generated server event-stream benchmark

Compare the legacy server from a clean, updated `~/smithy-rs-latest` with the
schema server from the current working tree. Both use the same model and the
same generated Rust client per protocol. Clients and servers are separate
processes exchanging real, unsigned event streams over loopback HTTP/2.
This compares two revisions, so a measured difference cannot be attributed
solely to schema serialization.

The current side exercises the [schema event-stream runtime](../../rust-runtime/aws-smithy-http-server/src/schema/event_stream.rs).
Generation validation checks the generated server's marshaller, unmarshaller,
and response-body calls, so an ignored setting cannot silently benchmark the
same implementation twice.

The checked-in model and programs are the benchmark. SDKs, Cargo lockfiles,
binaries, profiles and results are retained under ignored `artifacts/`.
No production runtime APIs are changed.

## Prepare on cloudl

From this checkout, export a new source snapshot including uncommitted
production sources. The destination must not exist. This copies files, not Git
objects or refs. `--update-baseline` requires the remote legacy checkout to be
clean and performs `git pull --ff-only`.

```sh
python3 tools/server-event-stream-benchmark/remote.py \
  --host cloudl \
  --destination /home/fahadzub/event-stream-benchmark-SNAPSHOT \
  --update-baseline
ssh cloudl
cd ~/event-stream-benchmark-SNAPSHOT
export PATH="$HOME/.cargo/bin:$PATH"
python3 tools/server-event-stream-benchmark/prepare.py
python3 tools/server-event-stream-benchmark/build.py
python3 tools/server-event-stream-benchmark/run.py --preset smoke
python3 tools/server-event-stream-benchmark/run.py --preset representative
```

`prepare.py` can also run directly on any Linux machine with the two checkouts.
Set `--baseline`, `--current`, and `--cache` as needed. It never updates Git.
`build.py` never runs generation. `run.py` never pulls, generates, or compiles;
it rejects stale source/configuration/SDK/binary/lockfile/compiler fingerprints.
Use a new cache to preserve a prior comparison when sources change. The same
Rust toolchain from the repository's `rust-toolchain.toml` must build both sides.

The Gradle init script loads each checkout's real server/client codegen plugin.
Legacy settings disable `schemaSerde` when the checkout supports that flag;
older legacy codegen has no schema serialization path. Current settings enable it.
Both use HTTP 1.x **types** and always send CBOR initial-response metadata.
The transport itself is HTTP/2. One unsigned client SDK is generated from the
legacy checkout per protocol and reused for both servers.
Its fixed Hyper 0.14 connector forces HTTP/2 through the public connector
builder; both servers use their HTTP 1.x transport implementations.
The scripts require Python 3.11 or newer.

## Workloads and correctness

REST JSON and RPC v2 CBOR each run:

- Upload: the generated client produces messages lazily; the server validates
  and drains them, returning a verified count and metadata.
- Download: the server produces messages lazily; the client validates every
  payload, event sequence/header, count, metadata, and end of stream.
- Duplex: the server echoes messages while the request is still open; the
  client produces and receives concurrently. A bounded 64-event timestamp
  queue limits outstanding messages. Preflight gates the producer after its
  first event until the first echo arrives, proving a response before request
  end of stream. Each stream has a fixed upper bound on message count.

Structured events have text, a nested object, a collection, and an event
header sequence. Binary events use an event header sequence and a raw blob
event payload. Non-stream members exercise REST headers and CBOR
initial-request/initial-response messages. Preflight also verifies a modeled
stream error following two valid messages. Any corruption, incomplete stream,
unexpected error, process failure, or timeout fails the run; such a run is not
reported as successful throughput. Measurement verifies payloads too, so
application validation cost is included identically on both sides.

## Presets and measurements

The default representative preset runs both protocols, all three directions,
both event types, 1 KiB payloads, concurrency 1/16, 1,024 events per stream,
2 seconds warmup, 10 seconds measurement, and three paired repetitions.
Legacy/schema order alternates by pair. Memory passes are separate; duplex
at concurrency 16 also has separate counter and sampling passes. Allow
roughly 35–45 minutes, plus generation/build time.

`smoke` uses eight events per stream, concurrency 1, 0.2-second warmup,
0.5-second measurement, and one pair. It exercises every workload and both
implementations; its timings are not statistically meaningful.
`full` additionally sweeps 64-byte and 64-KiB payloads.
`--seconds`, `--pairs`, and `--no-profiles` control a diagnostic run.

Default affinity uses server CPU 0 and client CPUs 1,2, with one server
worker and two client workers. Override `--server-cpus`, `--client-cpus`, and
`--server-workers`. CPU lists must contain distinct physical cores, not SMT
siblings, and fit the process's allowed affinity. Pinning reduces interference;
recorded machine load is evidence to assess, not a claim of isolation.

Warmup fully drains before the barrier. Only complete streams started during
the measurement window contribute; their final drain is included in elapsed
time and CPU accounting. Each task stops starting new streams at the deadline.
Client measurement CPU uses `getrusage` snapshots within the barrier; server
snapshots bracket client measurement, including a small control overhead.

Ordinary release binaries report:

- Events/s and application MiB/s. Bytes mean only text/blob content, excluding
  framing, event headers and fixed structured fields. Duplex counts one
  completed round trip, not two events or twice the application bytes.
- Per-process user/system CPU seconds, utilization as a percentage of one
  core, and CPU nanoseconds per event/round trip.
- Stream completion and download/duplex first-event p50/p95/p99 latency.
  Duplex round-trip latency starts before enqueueing the event for wire
  serialization and includes queueing, transport, server and client work.
  Histograms include sample counts and have three significant digits.
- Idle RSS and RSS sampled every 100 ms during measurement. Sampled peak
  can miss short spikes. Lifetime maximum RSS is reported separately by
  `getrusage` and includes startup/warmup.

Allocation-instrumented release binaries run separately and report allocations,
requested allocated bytes per event, requested live heap at the baseline and
end, and absolute peak requested live heap after reset at the barrier.
Successful `realloc` counts as an allocation of the new requested size.
Requested heap excludes allocator metadata and is distinct from RSS.
Instrumentation changes execution cost: memory-pass throughput is never
mixed into ordinary throughput ratios.

Profiling binaries preserve frame pointers and debug information. Separate
`perf stat` and `perf record` passes attach to both processes after warmup and
before measurement. Sampling uses 32-page ring buffers so simultaneous
collectors fit ordinary locked-memory limits. Raw counter CSV, `perf.data`, stack traces and hotspot
reports are retained. Flamegraphs are produced if `stackcollapse-perf.pl`
and `flamegraph.pl` are available. Unsupported counters or permission failures
remain explicit in perf exit codes/logs/CSV, never converted into zero or used
to change system settings.
Clients remain alive after reporting their measurements while the controller
stops and flushes both profiles. `stopped_by_controller` records intentional
SIGINT termination (which can produce exit code `-2`); counter/sample artifacts
and diagnostic logs remain available for inspection.

## Results and tests

Each timestamped result directory contains provenance, raw JSON, commands,
process stderr, listener/cleanup evidence, RSS samples, counter/profile files,
`summary.csv`, and `report.md`. `summary.json` includes server/client CPU cost.
Summaries show median throughput and paired schema/legacy ratios, with the
observed min/max range and pair count. No outliers are discarded and the range
is not a confidence interval. Keep the entire cache (including generated SDKs,
models, settings, logs, compiler information and locks) with results to rerun
the exact binaries.

```sh
python3 -m unittest discover -s tools/server-event-stream-benchmark -p 'test_*.py'
```

Accounting tests check interval deltas, pairing, profiling/allocation exclusion,
source/cache invalidation and SMT exclusion. A smoke run adds real generation,
compilation and live client/server preflight coverage. Servers bind only
`127.0.0.1` on ephemeral ports; each case records the actual listener and verifies
that the port is closed after cleanup.
