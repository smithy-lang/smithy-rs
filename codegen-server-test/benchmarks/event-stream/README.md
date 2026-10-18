# Generated server event-stream Criterion benchmarks

Rust benchmarks compare generated legacy serde, generated schema serde, and a
schema server with REST JSON and RPC v2 CBOR enabled together. All SDKs come
from the same checkout and model. Every projection sets `http-1x: true`;
single-protocol projections differ only in `schemaSerde`.

`make codegen` uses the repository's Gradle server-codegen tasks, just like
`examples/multi-protocol`. Five generated SDKs are dependencies of this standalone
Cargo workspace. No Python, generated client, listening socket, or network
transport is involved in running the benchmark.

```sh
cd codegen-server-test/benchmarks/event-stream
make test
make bench                       # small: legacy versus schema
make bench PRESET=medium         # add the multiprotocol server
make bench PRESET=full           # all payload/count combinations
```

`make test` generates the SDKs, checks actual wire behavior, and runs all 144
Criterion cases once with `--test`. `make bench` regenerates the SDKs and defaults
to `small`. Presets change the matrix, retaining the same measurement duration:

| Preset | Implementations | Workloads | Cases | Approximate time |
| --- | --- | --- | ---: | ---: |
| `small` (default) | Legacy, single-protocol schema | 1 KiB × 64 events | 24 | 28 minutes |
| `medium` | Add multiprotocol schema | 1 KiB × 64 events | 36 | 42 minutes |
| `full` | All three | All four payload/count points | 144 | 2 hours 48 minutes |

All presets include both protocols, all three operations and both event types.
Times include ten-second warmup and sixty-second measurement per case, excluding
build and analysis. Criterion may extend measurement to obtain sufficient samples.
Pass a Criterion filter or timing options through `BENCH_ARGS`:

```sh
make bench PRESET=small BENCH_ARGS="'rest-json/upload' --measurement-time 60"
```

Set `EVENT_COUNT` to override the number of events per request:

```sh
make bench PRESET=small EVENT_COUNT=256 BENCH_ARGS=rpc-v2-cbor
```

This runs the 12 CBOR cases with 256 events of 1 KiB per request. The count
must be an integer from 1 to 2147483647; unset or empty keeps the preset defaults.
The override applies to every workload in the selected preset. For `full`, the
two 1 KiB workloads become one, giving 108 cases overall or 54 CBOR cases.
Benchmark names include the selected count, and throughput uses that count.
Measurement duration stays the same; increasing the count increases work per
request rather than the configured time per case.

After generation, use Cargo directly to avoid regenerating between measurements.
`BENCH_PRESET` selects the matrix and defaults to `small` when unset:

```sh
BENCH_PRESET=full cargo bench --quiet --bench event_streams -- \
  'rest-json/echo/structured/.*/1024B-64events'
```

For a custom count with Cargo, set `BENCH_EVENT_COUNT`:

```sh
BENCH_PRESET=small BENCH_EVENT_COUNT=256 cargo bench --quiet --bench event_streams -- \
  rpc-v2-cbor
```

Each benchmark defaults to ten seconds of warmup, sixty seconds of measurement,
and thirty samples. Criterion can extend a measurement to obtain sufficient
samples. There are 144 benchmarks in the full suite, so allow several hours.
CLI options can override these defaults for an explicitly labeled diagnostic run.

## What is measured

Each timed iteration sends one `http::Request` to the generated Tower service
using `oneshot`, then polls its response body through end of stream:

| Workload | Server work |
| --- | --- |
| Upload | Routing, initial input, decoding events, counting payload bytes, ordinary response serialization |
| Download | Routing, initial input, lazy modeled-event clones, event serialization and framing |
| Echo | Routing, initial input, decoding and re-encoding every event |

Request construction, immutable input-fixture cloning, service cloning, service
construction, and correctness checks are outside timing. Input fixtures are valid
CRC-protected event-stream frames encoded once in Rust and shared by all three
implementations. They are delivered as individual HTTP data chunks. CBOR requests
include their initial-request message; CBOR responses include initial-response
metadata. Structured events have text, a nested object, eight integers, and a
sequence event header. Binary events have a blob payload and sequence header.
Cloning each chunk's `Bytes` handle as the input body is polled is included.

Download modeled events are prepared outside timing, then cloned lazily inside
the handler. String/collection clones and consumed-event destruction are included;
`Blob` clones share their backing bytes. All implementations perform the same
application work. Echo forwards each decoded owned event without an extra clone.

Response draining is incremental and does not build a whole-response buffer or
decode the output. **Stopping at the response headers would miss lazy
serialization**, so draining every frame is inside timing. A common
`BoxCloneService` adapter, Tokio current-thread executor entry, handler assertions,
frame polling, and destruction are included. This is a complete in-process
server-stack comparison, not a serializer-only measurement.

Criterion reports time per complete request and events/second. Echo counts one
decoded-and-encoded event, not two. Compare `legacy` against `schema` to assess the
serde paths; compare `schema` against `multi-schema` to assess the enabled-protocol
configuration. Protocol routing construction and service initialization are not
included. This suite does not measure HTTP parsing, HTTP/2 flow control, sockets,
client work, network latency, concurrent requests, or real duplex backpressure.

The workload points are:

| Payload bytes | Events/request |
| ---: | ---: |
| 64 | 1 |
| 1,024 | 64 |
| 1,024 | 1,024 |
| 65,536 | 16 |

## Correctness and results

Before each benchmark, deep checks verify status, upload counts/bytes, output
frame CRCs, event order, event headers, exact payloads, metadata and stream end.
Integration tests also split input frames into seven-byte chunks and verify
modeled errors after successful events. Both implementations must satisfy the
same wire expectations before timing. A preflight failure stops the run.

Criterion retains machine-readable estimates, sample data and reports under the
Cargo target directory's `criterion/`. The repository's `.cargo/config.toml`
shares `<repository>/target/criterion/` across Cargo workspaces; `CARGO_TARGET_DIR` can
override it. Record host load, CPU affinity, compiler and source revision when
interpreting a run. Criterion intervals describe that run's samples; use repeated
runs with alternating implementation filters to assess host and order effects.

For a focused profile, Criterion provides a fixed-duration profiling mode:

```sh
BENCH_PRESET=small cargo bench --quiet --bench event_streams -- \
  'rpc-v2-cbor/download/structured/schema/1024B-64events' --profile-time 60
```

The bench profile retains debug information. Build with
`RUSTFLAGS="-C force-frame-pointers=yes"` for frame-pointer profiling. Attach
`perf` to the benchmark executable or use `cargo flamegraph` if installed.
Whole-process captures also contain setup and validation; use the generated
marshaller/unmarshaller and response-drain stacks to distinguish measured work.
Keep profiling passes separate from ordinary Criterion measurements. Criterion
time estimates are not CPU-counter, allocation, heap, or RSS measurements.
