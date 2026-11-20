# Event-stream body limits benchmark

This standalone microbenchmark imports the production timeout and poll-budget
wrappers directly. It compares four configurations:

- `disabled`: no deadlines and budget zero; both wrappers return the original body.
- `budget_only`: budget 32, with deadlines disabled (the runtime defaults).
- `before_fix`: idle and completion deadlines enabled, wrapping budget 32.
- `after_fix`: budget 32 wrapping the same enabled deadlines.

The before/after comparison changes only wrapper composition. `disabled` is a
configuration baseline, not a checkout of the code before limits were introduced.
Deadlines are one hour, so none expire during measurement.

Run from the repository root:

```sh
cargo build --quiet --offline --locked --release --manifest-path tools/event-stream-limits-bench/Cargo.toml
target/release/event-stream-limits-bench 1048576 9 > tools/event-stream-limits-bench/results.csv
```

Each workload returns continuously ready HTTP data frames. `complete_1024` puts
one 1024-byte event frame in each HTTP frame; `fragmented_64` splits each event
frame into sixteen 64-byte HTTP frames. The frame tracker consumes the length
prefix; CRC validation and event decoding are excluded.

The harness uses a current-thread Tokio runtime, warms each configuration, and
rotates/reverses execution order across repetitions. Body construction and initial
timer allocation are excluded; frame polling, clock checks, timer resets,
cooperative yields, and EOF polling are included. Timing is reported per HTTP
frame, not per event. Tokio is pinned to 1.53.1, matching the runtime lockfile.

This measures steady-state wrapper overhead on a ready body. It does not measure
network throughput, generated deserialization, service handlers, concurrent
request fairness, timeout latency, memory overhead, buffered-body size limits,
or non-streaming read limits. It should not be used as an end-to-end service
performance claim. Raw repetitions are retained in `results.csv`.
