# Event-stream limits overhead results

Measured on 2026-10-26 on an AMD EPYC 9R14 Linux devbox with Rust 1.94.1 and Tokio 1.53.1. Release build, current-thread Tokio runtime, nine repetitions per configuration, 1,048,576 HTTP data frames per repetition. The CPU was not reserved or pinned. These are local measurements of the production body wrappers; no network, event decoder, or service handler is involved.

Both enabled deadline configurations use one-hour idle and completion windows and poll budget 32. The only difference between before and after is the wrapper order. Body construction and initial timer allocation are outside the timed interval.

Values are median nanoseconds per HTTP frame, followed by minimum–maximum across repetitions.

| Workload | Limits disabled | Budget only | Before fix | After fix |
|---|---:|---:|---:|---:|
| complete_1024 | 36.58 (36.31–36.91) | 54.44 (54.22–54.53) | 151.94 (151.65–152.13) | 151.14 (150.82–151.29) |
| fragmented_64 | 36.44 (36.37–36.60) | 54.41 (54.31–54.62) | 116.33 (115.79–116.54) | 114.73 (113.88–114.85) |

`complete_1024` returns a complete 1024-byte event frame per HTTP frame. `fragmented_64` splits each event into sixteen 64-byte HTTP frames. The payload is not decoded or CRC-validated; the timeout tracker reads the length prefix.

The ordering fix showed no overhead increase in this run. Small before/after differences should not be treated as a general performance improvement. Enabled deadlines add measurable clock/timer/tracking cost; the disabled configuration is a configuration baseline, not a historical pre-feature checkout.

This benchmark does not cover concurrent request fairness, timeout latency, sockets, buffered-body size limits, non-streaming read limits, or end-to-end server throughput. See README.md for the command and full scope; results.csv retains every repetition.
