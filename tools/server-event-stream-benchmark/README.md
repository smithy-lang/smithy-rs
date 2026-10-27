# Generated server event-stream benchmark

The default comparison generates two servers from the **same source tree**, with
`schemaSerde: false` (generated serde) and `schemaSerde: true` (schema serde).
One generated Rust client per protocol is shared by both. Clients and servers
are separate processes exchanging unsigned event streams over loopback HTTP/2.
Existing executable/cache labels containing `legacy` mean generated serde,
independently of HTTP version.

For a historical release comparison, explicitly select `--comparison release`.
That comparison includes revision and dependency differences and cannot isolate
schema serialization. `--legacy-release` selects the pinned release tag
(default `release-2026-09-30.2`).
The current side exercises the [schema event-stream runtime](../../rust-runtime/aws-smithy-http-server/src/schema/event_stream.rs).
Generation validation checks the generated server's marshaller, unmarshaller,
and response-body calls, so an ignored setting cannot silently benchmark the
same implementation twice.

The checked-in model and programs are the benchmark. SDKs, Cargo lockfiles,
binaries, profiles and results are retained under ignored `artifacts/`.
No production runtime APIs are changed.

## Prepare on a benchmark host

For local use:

```sh
python3 tools/server-event-stream-benchmark/prepare.py
python3 tools/server-event-stream-benchmark/build.py
python3 tools/server-event-stream-benchmark/run.py --preset smoke
python3 tools/server-event-stream-benchmark/run.py --preset representative
```

For a remote benchmark host, first copy a new source snapshot, then run the same
commands on that host. `--host` is required; the destination must not exist.

```sh
python3 tools/server-event-stream-benchmark/remote.py \
  --host YOUR_BENCHMARK_HOST \
  --destination /home/$USER/event-stream-benchmark-SNAPSHOT
```

`prepare.py` accepts `--current` and `--cache`. It never updates Git. Both servers
and the common client come from `--current` by default. Set `BASELINE_CHECKOUT`
to the reference checkout location before comparing releases:

```sh
python3 tools/server-event-stream-benchmark/prepare.py \
  --comparison release --baseline "$BASELINE_CHECKOUT" \
  --legacy-release release-2026-09-30.2 --cache /tmp/event-stream-release-cache
```

Release mode verifies that the baseline is clean and at the selected tag.
`remote.py --update-baseline` fetches tags and checks out `--legacy-release` in
its `--baseline` checkout. Cache reuse rejects changes in comparison design or
release selection.
`build.py` never runs generation. `run.py` never pulls, generates, or compiles;
it rejects stale source/configuration/SDK/binary/lockfile/compiler fingerprints.
Use a new cache to preserve a prior comparison when sources change. The same
Rust toolchain from the repository's `rust-toolchain.toml` must build both sides.

The Gradle init script loads each checkout's real server/client codegen plugin.
Legacy settings disable `schemaSerde` when the checkout supports that flag;
older legacy codegen has no schema serialization path. Current settings enable it.
Both use HTTP 1.x **types** and always send CBOR initial-response metadata.
The transport itself is HTTP/2. One unsigned client SDK is generated per protocol and reused for both servers.
It comes from the current tree in same-revision mode, or the baseline in release mode.
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
  client produces and receives concurrently. Throughput runs do not collect
  event timestamps. Latency runs use a timestamp queue sized to the stream
  event count by default; `--duplex-window` permits an explicit sensitivity
  experiment. Preflight gates the producer after its
  first event until the first echo arrives, proving a response before request
  end of stream. Each stream has a fixed upper bound on message count.

Structured events have text, a nested object, a collection, and an event
header sequence. Binary events use an event header sequence and a raw blob
event payload. Non-stream members exercise REST headers and CBOR
initial-request/initial-response messages. Preflight also verifies a modeled
stream error following two valid messages. Any corruption, incomplete stream,
unexpected error, process failure, or timeout fails the run; such a run is not
reported as successful throughput. Preflight deeply validates every payload. Measurement checks event type,
sequence, length, count, metadata and end of stream; full byte scans and nested
value checks require `--deep-validation`. Validation is identical on both sides.
This flag permits a sensitivity experiment for validation overhead.

## Presets and measurements

The representative preset runs both protocols, all three directions, both event
types, 1 KiB payloads, concurrency 1/4/16, and 1,024 events per stream. Defaults
are 5 seconds warmup, 15 seconds measurement, and ten paired throughput
repetitions with alternating order. Allocation runs repeat three times per side,
also alternating order. Latency, Dial9 traces, Tokio runtime diagnostics and profiling are separate passes. This is a
multi-hour experiment; use explicit smaller settings for diagnostic runs.

`smoke` uses eight events per stream, concurrency 1, 0.2-second warmup,
0.5-second measurement, and one throughput/allocation pair. Its timings are
not statistically meaningful. `full` additionally sweeps 64-byte and 64-KiB
payloads. `--seconds`, `--pairs`, `--memory-pairs`, `--concurrency`, and
`--no-profiles` control the experiment. `--events-per-stream` overrides stream
length, making stream-start overhead and long-stream steady-state costs explicit.
`--warmup` overrides the warmup interval. `--diagnostics` selects the separate
passes (`memory latency runtime dial9` by default). For a shorter server-timing
experiment that retains ten throughput pairs:

```sh
python3 tools/server-event-stream-benchmark/build.py --modes cpu timing dial9
python3 tools/server-event-stream-benchmark/run.py --preset representative \
  --warmup 1 --seconds 2 --pairs 10 --concurrency 1 \
  --diagnostics latency dial9 --no-profiles
```

### Server timing and Dial9

Latency passes use a separate server binary with the same outer HTTP service
and response-body wrappers for generated serde and schema serde. The shared
client uses its ordinary binary. Server measurement starts when Hyper calls
the HTTP service, after parsing request headers and before routing and input
deserialization. Histograms retain sample count, p50/p95/p99 and maximum:

- `response_ready`: time until the service returns response headers and a body.
- `first_frame_ready`: time until the first serialized data frame is handed to Hyper.
- `body_complete`: time until response-body end of stream is handed to Hyper.
- `service_poll_active` and `body_poll_active`: summed wall time inside the
  response future and response-body polls. These exclude time between polls
  but include blocking or preemption during a poll; they are not CPU time.

Download and duplex serialization is lazy: returning the response does not end
the request timer. The body wrapper forwards frames, errors, trailers, size
hints and end-of-stream state. Known end-of-stream on a final frame completes
the timer even if Hyper skips another poll. Empty responses, errors and
abandoned responses are accounted for separately. Each request retains its
measurement window across a reset, and warmup is excluded from histograms.
Normal measured runs require exactly one complete set of server samples per
client-completed stream, zero active/error/aborted requests, and HTTP 200.
Correctness preflight also runs with both diagnostic wrappers enabled.

These boundaries measure serialized output availability, not socket flush or
peer receipt. Streaming wall latency includes client pacing and HTTP/2
backpressure. Client end-to-end latency and per-process CPU remain complementary
measurements.

Dial9 uses pinned `dial9` and `dial9-utils` 0.5.5 dependencies. Its separate server
binary builds with `--cfg tokio_unstable`, attaches the recorder to a Tokio
runtime with the requested worker count, and installs `Dial9TracingLayer`.
The layer captures only spans declaring the `dial9` marker; the benchmark's
`server_request` span covers service and body polling and records explicit
response/first-frame/completion elapsed nanoseconds and outcome. It is entered
per poll, never held across an await. Runtime traces can also include warmup;
benchmark request spans and timing histograms start at the measurement reset.
CPU and memory sampling are not enabled by this mode.

The runner sets `BENCH_DIAL9_TRACE` to each case's `server-trace-{side}.bin`
prefix. Dial9 seals numbered files such as `server-trace-legacy.0.bin`
(`schemaSerde: false`) or `server-trace-schema.0.bin` (`schemaSerde: true`).
All segments stay on the benchmark host. After the client exits, the controller
requests graceful server shutdown; the server drops Tokio workers and drains
the recorder before exiting. A missing trace or failed shutdown fails the case.
Artifact path, size and SHA-256 are saved in `result.json`. Diagnostic throughput
is excluded from the paired throughput comparison.

#### Generating and reading traces

Run these commands from the repository root on your benchmark host:

```sh
python3 tools/server-event-stream-benchmark/prepare.py
python3 tools/server-event-stream-benchmark/build.py --modes cpu timing dial9
python3 tools/server-event-stream-benchmark/run.py \
  --preset smoke --events-per-stream 1024 \
  --diagnostics latency dial9 --no-profiles
```

The runner configures tracing and flushes it automatically on shutdown. A
legacy Dial9 case produces:

```text
tools/server-event-stream-benchmark/artifacts/results/<run>/<case>/
├── result.json
└── server-trace-legacy.0.bin
```

Schema cases use `server-trace-schema.0.bin`. Additional numbered segments,
when present, belong to the same case. Case numbers depend on the run; use
`result.json` to identify its protocol, direction, event kind and variant.

For latency distributions, read the run's `report.md`, or inspect an individual
case. Replace `<run>` and `<case>` with the directory names from your run:

```sh
benchmark_case='tools/server-event-stream-benchmark/artifacts/results/<run>/<case>'
jq '{side, protocol, direction, kind, mode, dial9}' "$benchmark_case/result.json"
jq '.server_timing' "$benchmark_case/result.json"
```

To decode traces and analyze Tokio tasks and scheduling offline, install the
pinned viewer and extract its analysis toolkit. The toolkit requires Node.js:

```sh
cargo install --quiet --locked dial9-viewer \
  --version 0.5.5 --no-default-features
dial9-viewer agents toolkit /tmp/dial9-toolkit
node /tmp/dial9-toolkit/analyze.js "$benchmark_case"
```

The analyzer accepts either a case directory containing all its segments or
an individual trace file:

```sh
node /tmp/dial9-toolkit/analyze.js "$benchmark_case/server-trace-legacy.0.bin"
```

Use `server-trace-schema.0.bin` for a schema case. The `server_request` span
records these explicit wall-time fields:

- `response_ready_ns`: response headers and body object are ready.
- `first_frame_ready_ns`: the first serialized data frame is ready for Hyper.
- `body_complete_ns`: the complete response body has been handed to Hyper.

These fields measure elapsed time since service entry. A span's active
segments cover only time inside service and body polls.

### Load models and why this is not stock wrk2

Stock [wrk2](https://github.com/giltene/wrk2) is an HTTP/1.x request/response
load generator. This tool uses HTTP/2 and must generate, decode and validate
Smithy frames, including a bidirectional stream whose response starts before
request completion. Stock wrk2 cannot exercise that contract. We adopt its
intended-arrival-time measurement principle in the generated client.

The default **closed-loop** run measures completed event throughput at a fixed
number of concurrent streams. It cannot establish production tail latency at a
fixed external arrival rate. For a scheduled stream-arrival experiment:

```sh
python3 tools/server-event-stream-benchmark/run.py --preset representative \
  --stream-rate 20 --concurrency 1 4 16 --no-profiles
```

`--stream-rate` is streams per second, not events per second. Arrivals follow an
absolute schedule independent of completion, with the concurrency setting
limiting active streams. Delayed arrivals retain their intended timestamps;
stream completion and first-event latency include dispatch backlog. All scheduled
streams are drained, and dispatch delay and drain duration are reported. Large
backlogs can hit the run timeout and fail the experiment. This models queued
stream arrivals with a concurrency cap; it does not independently pace events
inside each stream. Duplex event RTT starts at event production, separately from
stream-arrival latency. At a fixed offered rate, throughput ratios describe
achieved completion rates, not maximum server capacity.

Use a rate sweep below and above observed capacity, examine client CPU cost and
dispatch lag, and repeat with more client resources before attributing a plateau
to the server. Verify stable results with longer warmup/measurement, repeated
runs, and explicit CPU placements; affinity alone is not machine isolation.

### Socket-free variants

The existing [Criterion benchmark](../../codegen-server-test/benchmarks/event-stream/README.md)
provides two same-revision modes using pre-encoded request fixtures:

```sh
cd codegen-server-test/benchmarks/event-stream
make bench TRANSPORT=tower
make bench TRANSPORT=hyper-memory
```

`tower` calls the generated service with `oneshot` and drains the response body.
`hyper-memory` runs Hyper HTTP/2 client/server connections over a bounded
in-memory byte stream. It includes HTTP/2 framing, flow control and scheduling,
with no sockets or TCP stack. Neither includes the generated client or the same
payload-generation workload as this network tool; compare each mode on its own.
The difference between modes is not an exact TCP or HTTP/2 cost subtraction.

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
Separate latency runs report:

- Stream completion and download/duplex first-event p50/p95/p99 latency.
  Duplex round-trip latency starts before enqueueing the event for wire
  serialization and includes queueing, transport, server and client work.
  Histograms are reused per worker, include sample counts, and have three
  significant digits. Throughput/allocation runs disable latency histograms
  and per-event timestamp queueing.
All measured modes report:

- Idle RSS and RSS sampled every 100 ms during measurement. Sampled peak
  can miss short spikes. Lifetime maximum RSS is reported separately by
  `getrusage` and includes startup/warmup.

Each workload also runs a separate Tokio diagnostic pass for both server
implementations and the shared client. These binaries enable `tokio_unstable`
and Tokio's `schedule-latency` feature (Tokio 1.54 or newer); ordinary throughput,
allocation and perf binaries retain their existing compiler settings.
Tokio's own `RuntimeMetrics` supplies:

- Per-worker busy duration, poll count, parks, local schedules, queue overflows
  and stolen tasks, as counter deltas after warmup.
- Global and worker-local queue depth, alive tasks, blocking queue depth,
  blocking thread count and worker mean poll time (Tokio's EWMA), sampled
  every 100 ms by an independent OS thread, with initial and final observations.
- Task poll duration and scheduler delay histograms. Scheduler delay measures
  time from scheduling a task until polling it, including tasks polled from
  Tokio's LIFO slot. Logarithmic buckets cover short polls through long stalls;
  actual bucket boundaries and per-worker count deltas are retained.

Both monitors start at the measurement barrier and include final stream drain.
The server window includes the controller margin; each worker's busy percentage
uses its own monitor's elapsed duration. Raw before/after counters and timestamped
gauge samples remain in `result.json`. Reports show per-process worker busy
percentage, queue/task peaks and p99 histogram **bucket bounds**, not exact
quantiles. Busy duration is wall time processing work, including blocking inside
a task, rather than process CPU time. Tokio publishes some counters in batches,
so interval-edge attribution is approximate. Sampled queue peaks can miss
transients; the main `Runtime::block_on` future and OS threads are outside worker
metrics. There is no custom task timing hook or heartbeat.

These observations help distinguish a starved client scheduler from server
cost: examine client scheduler delay and queued work alongside process CPU,
stream dispatch delay and a repeat with more client resources. A high busy
percentage alone does not establish starvation. Histogram instrumentation adds
clock calls and changes execution cost, so diagnostic-pass throughput never
contributes to the paired throughput comparison.

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
Summaries show median throughput and paired ratios, with observed min/max and
pair count. They also show the geometric mean paired ratio and a deterministic
95% percentile bootstrap interval (5,000 resamples, seed 0), withheld below ten
pairs. Ten pairs is a reporting threshold, not a precision guarantee. The interval
assumes independent repetitions and does not correct thermal drift or correlated
noise. No outliers are discarded, and the observed range is not a confidence
interval. Latency runs are exploratory observations, not confidence estimates. Keep the entire cache (including generated SDKs,
models, settings, logs, compiler information and locks) with results to rerun
the exact binaries.

```sh
python3 -m unittest discover -s tools/server-event-stream-benchmark -p 'test_*.py'
```

Accounting tests check interval deltas, pairing, instrumented-pass exclusion,
source/cache invalidation and SMT exclusion. A smoke run adds real generation,
compilation and live client/server preflight coverage. Servers bind only
`127.0.0.1` on ephemeral ports; each case records the actual listener and verifies
that the port is closed after cleanup.
During build, Rust calibration tests verify the allocator and deliberately block
a Tokio worker with queued tasks, checking visible backlog, busy time, and
scheduler-delay histogram observations in both process roles.
