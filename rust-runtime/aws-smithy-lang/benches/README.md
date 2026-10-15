# Benchmarks

`model.rs` measures these operations on the SQS, S3, and EC2 models from
`aws/sdk/aws-models`:

- `parse`: `Model::from_json_slice` from in-memory bytes. This includes
  assembly, prelude injection, and `StructuralV1` validation, but not file I/O.
- `write`: compact, insertion-ordered `AstWriter::write` into a reused,
  pre-sized `Vec<u8>`.
- `service_closure`: a `Walker` over every shape and member reachable from the
  model's service, following all relationships.

There is no pass/fail threshold. Compare new results against this baseline on
the same machine class and fixture commit.

## Running

From `rust-runtime/aws-smithy-lang`:

```bash
taskset -c 2 cargo bench --bench model -- --warm-up-time 2 --measurement-time 5 --noplot
```

## Baseline

- Machine: Intel Xeon Platinum 8259CL @ 2.50GHz, 16 vCPUs, pinned to one core
  with `taskset`.
- Toolchain: rustc 1.94.1, `bench` profile.
- Fixture commit: `f45327599b` (last change to the three model files).
- Crate commit: `a43c245d7`, plus the uncommitted benchmark.

Times are criterion medians.

| Model | Input size |             `parse` | `write` | `service_closure` |
| ----- | ---------: | ------------------: | ------: | ----------------: |
| SQS   |     272 KB | 2.54 ms (102 MiB/s) | 0.35 ms |           0.13 ms |
| S3    |    2.98 MB | 22.6 ms (126 MiB/s) | 3.57 ms |           1.02 ms |
| EC2   |    6.35 MB | 109.3 ms (55 MiB/s) | 9.65 ms |           8.95 ms |

Note: EC2 parses at roughly half the throughput (MiB/s) of S3 because it is
denser, not because of an EC2-specific bottleneck. Profiling found no hotspot
unique to EC2 and nothing that grows faster than the model:

| Model | Shapes + members per MB | Traits per MB | Whitespace | Documentation text |
| ----- | ----------------------: | ------------: | ---------: | -----------------: |
| S3    |                     924 |         1,457 |        43% |                40% |
| EC2   |                   2,729 |         4,736 |        29% |                34% |

Most of S3's bytes are whitespace and documentation strings, which serde_json
scans at about 1 GB/s. EC2's bytes are mostly structure, and each shape, member,
and trait costs several allocations. Per shape or member, EC2 is cheaper than S3
(5.9 µs vs 8.1 µs, 41 vs 54 allocations), but it makes 113 allocations per KB
versus S3's 50. Plain `serde_json::Value` parsing shows the same input effect
(322 MiB/s for S3, 223 MiB/s for EC2). Allocation and deallocation are 29% (S3)
to 35% (EC2) of parse samples, and `parse` includes dropping the model (3.2 ms
for S3, 16.6 ms for EC2). Reducing per-member and per-trait allocations would
improve throughput for every model.

## Knowledge-index baseline

Same machine, toolchain, and fixture commit as above; crate commit `1cb221fdd`
plus the uncommitted knowledge indexes. Measured with
`--warm-up-time 1 --measurement-time 3`.

| Model | `relationship_index_build` | `contained_operations` | `incoming_relationships` | `input_bindings` | `parent_chain` |
| ----- | -------------------------: | ---------------------: | -----------------------: | ---------------: | -------------: |
| SQS   |                    0.26 ms |                 3.2 µs |                  0.66 µs |          0.10 µs |         1.7 µs |
| S3    |                    1.31 ms |                15.1 µs |                  0.68 µs |          0.11 µs |         7.8 µs |
| EC2   |                   11.96 ms |               108.3 µs |                  0.68 µs |          0.11 µs |        37.1 µs |

`service_closure` was re-measured in the same run at 0.13 ms, 1.02 ms, and 10.06
ms.

## PR review baseline

Same machine and fixture commit; crate commit `ec0520942` plus the uncommitted
owned shape model, `ModelAssembler`, per-node source locations, and
insertion-ordered writer. Measured with `--warm-up-time 1 --measurement-time 3`.

| Model |  `parse` | `write` | `service_closure` | `relationship_index_build` |
| ----- | -------: | ------: | ----------------: | -------------------------: |
| SQS   |  2.64 ms | 0.26 ms |           0.13 ms |                    0.27 ms |
| S3    |  24.9 ms | 2.56 ms |           1.04 ms |                    1.35 ms |
| EC2   | 116.2 ms | 8.94 ms |           9.34 ms |                   11.57 ms |

Recording a source location for every nested trait and metadata value initially
cost up to 25% of S3's parse time. Each node tree now shares one table of JSON
Pointers, and scalars need no table, which brings `parse` back within a few
percent of the previous run on this machine (2.61 ms, 23.8 ms, and 116 ms
measured before the change). `write` is faster because the default output no
longer sorts.

`shape_id` and `trait_lookup/s3` measure the representation questions raised in
review:

| Benchmark                                                             |   Time |
| --------------------------------------------------------------------- | -----: |
| `shape_id/parse` (`ShapeId::new`)                                     | 127 ns |
| `shape_id/from_parts`                                                 | 154 ns |
| `shape_id/with_member`                                                |  63 ns |
| `shape_id/namespace`                                                  | 2.0 ns |
| `trait_lookup/s3/shape_id` (`has_trait` on every S3 shape and member) | 158 µs |
| `trait_lookup/s3/str`                                                 | 155 µs |

Building an ID from parts costs one allocation, about 20% more than parsing
because the absolute string is assembled first. Looking a trait up by `ShapeId`
or by `&str` costs the same: both hash the absolute ID.

## Line and column positions

Same machine and fixture commit. "Before" is the PR review baseline code above,
re-measured immediately before adding line and column positions; "after" adds
them. Measured with `--warm-up-time 2 --measurement-time 5`.

| Model | `parse` before | `parse` after | Change | Allocations before → after | Retained before → after |
| ----- | -------------: | ------------: | -----: | -------------------------: | ----------------------: |
| SQS   |        2.64 ms |       2.71 ms |    +3% |            17,921 → 12,278 |       0.9 MiB → 1.0 MiB |
| S3    |        25.2 ms |       26.1 ms |    +4% |          149,021 → 109,716 |     11.4 MiB → 11.8 MiB |
| EC2   |       114.4 ms |      106.1 ms |    −7% |          716,455 → 459,097 |     24.9 MiB → 29.4 MiB |

Every value in a shape, including its members, references, and trait values,
now shares one location table per shape instead of one allocation per member
and one table per trait. That removes about a third of EC2's allocations, which
outweighs the cost of computing positions. The table also records structural
values such as targets, so retained memory grows. `write`, `service_closure`,
and `relationship_index_build` are unchanged.
