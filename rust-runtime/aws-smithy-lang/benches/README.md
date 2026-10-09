# Benchmarks

`model.rs` measures three operations on the SQS, S3, and EC2 models from
`aws/sdk/aws-models`:

- `parse`: `Model::from_json_slice` from in-memory bytes. This includes prelude injection
  and `StructuralV1` validation, but not file I/O.
- `write`: compact `ModelWriter::write` into a reused, pre-sized `Vec<u8>`.
- `service_closure`: a `Walker` over every shape and member reachable from the model's
  service, following all relationships.

There is no pass/fail threshold. Compare new results against this baseline on the same
machine class and fixture commit.

## Running

From `rust-runtime/aws-smithy-lang`:

```bash
taskset -c 2 cargo bench --bench model -- --warm-up-time 2 --measurement-time 5 --noplot
```

## Baseline

- Machine: Intel Xeon Platinum 8259CL @ 2.50GHz, 16 vCPUs, pinned to one core with `taskset`.
- Toolchain: rustc 1.94.1, `bench` profile.
- Fixture commit: `f45327599b` (last change to the three model files).
- Crate commit: `a43c245d7`, plus the uncommitted benchmark.

Times are criterion medians.

| Model | Input size | `parse` | `write` | `service_closure` |
| ----- | ---------: | ------: | ------: | ----------------: |
| SQS   |   272 KB |   2.54 ms (102 MiB/s) |  0.35 ms | 0.13 ms |
| S3    | 2.98 MB  |  22.6 ms (126 MiB/s)  |  3.57 ms | 1.02 ms |
| EC2   | 6.35 MB  | 109.3 ms (55 MiB/s)   |  9.65 ms | 8.95 ms |

EC2 parses at roughly half the throughput of S3. The cause has not been profiled.
