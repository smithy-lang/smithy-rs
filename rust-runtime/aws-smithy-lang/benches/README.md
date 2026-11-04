# Benchmarks

`model.rs` measures these operations on the SQS, S3, and EC2 models from
`aws/sdk/aws-models`:

- `parse`: `Model::from_json_slice` from in-memory bytes. This includes
  assembly, prelude injection, and `StructuralV1` validation, but not file I/O.
- `write`: compact, insertion-ordered `AstWriter::write` into a reused,
  pre-sized `Vec<u8>`.
- `service_closure`: a `Walker` over every shape and member reachable from the
  model's service, following all relationships.

`idl.rs` loads the same three models as Smithy IDL (see [Smithy IDL](#smithy-idl)).

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

## Smithy IDL

`idl.rs` renders each JSON model as equivalent IDL with the test-support converter
`tests/support/to_idl.rs`, and checks that every rendering assembles to a model equivalent to
the JSON one before measuring. The rendering uses `///` documentation, relative shape IDs,
omitted annotation-trait values, and `= value` assignments, so it is smaller than the JSON
(for S3, 1.8 MiB versus 2.8 MiB). `tests/idl_corpus.rs` applies the same check to every model
in `aws/sdk/aws-models`.

- `parse/json` and `parse/idl`: one input loaded and validated, as in `parse` above. Both run
  in the same binary, under the same counting allocator.
- `assemble/idl_64_files`: each namespace's shapes dealt round-robin into 64 IDL files, so
  most relative references resolve across files.
- `assemble/mixed`: shapes alternated between an IDL file and a JSON AST document.
- `strings`: a synthetic 0.3 MiB file of nested metadata objects, documentation comments,
  text blocks, and list traits.
- `syntax_error/early` and `syntax_error/late`: the S3 IDL with a syntax error inserted 1%
  and 99% of the way through.

```bash
taskset -c 2 cargo bench --bench idl -- --warm-up-time 1 --measurement-time 3 --noplot
```

Same machine, toolchain, and fixture commit as above; crate commit `5130265fc` plus the
uncommitted IDL changes. Times are criterion medians; throughput is per input byte, so it is
not directly comparable between representations.

| Model | `parse/json` | `parse/idl` | `assemble/idl_64_files` | `assemble/mixed` |
| ----- | -----------: | ----------: | ----------------------: | ---------------: |
| SQS   |      2.82 ms |     5.05 ms |                 5.41 ms |          4.24 ms |
| S3    |      29.1 ms |     54.9 ms |                 54.2 ms |          38.4 ms |
| EC2   |     115.3 ms |    173.6 ms |                167.5 ms |         145.7 ms |

| Benchmark            |    Time |
| -------------------- | ------: |
| `strings`            | 31.6 ms |
| `syntax_error/early` | 2.15 ms |
| `syntax_error/late`  | 28.0 ms |

Loading IDL takes 1.5–1.9× as long as loading the equivalent JSON. Spreading a model over
64 files costs nothing extra: the declaration index is built once, and lookups are hash
probes. A syntax error stops parsing, so an early error costs mostly UTF-8 validation and
line indexing.

The memory report (printed once by the benchmark) counts every allocation while loading
once, including dropping temporary data:

| Model | Input |   Bytes | Allocations | Allocated | Peak live | Retained |
| ----- | ----- | ------: | ----------: | --------: | --------: | -------: |
| SQS   | JSON  | 0.3 MiB |      12,279 |   2.6 MiB |   1.2 MiB |  1.0 MiB |
| SQS   | IDL   | 0.2 MiB |      24,540 |   4.3 MiB |   2.1 MiB |  1.0 MiB |
| S3    | JSON  | 2.8 MiB |     109,717 |  23.4 MiB |  12.6 MiB | 11.8 MiB |
| S3    | IDL   | 1.8 MiB |     252,326 |  40.8 MiB |  21.7 MiB | 11.4 MiB |
| EC2   | JSON  | 6.1 MiB |     459,098 |  84.9 MiB |  35.2 MiB | 29.4 MiB |
| EC2   | IDL   | 3.7 MiB |     820,887 | 128.8 MiB |  59.3 MiB | 29.3 MiB |

The retained model is the same size for both representations, so a loaded `Model` keeps
nothing IDL-specific. IDL peaks at about 1.7× the JSON peak because each file's syntax tree
is kept until the declaration index covering every input is built; JSON streams one shape
at a time. Its peak stays about twice the retained model, so the two-pass parser the design
describes as a fallback is not needed.

The following come from one-off profiling during development, not from the committed
benchmark. With a temporary timer around each phase, EC2 spent about 54 ms parsing and
79 ms lowering through the shared converter, both dominated by allocation (the syntax tree
owns its identifier and shape ID strings). An earlier version spent 52% of `parse/idl/s3`
counting characters for columns, because each lookup rescanned its line from the start,
which is quadratic for long lines such as S3's documentation. Columns on ASCII lines are now
byte differences, and other lines resume from the previous lookup; `parse/idl/s3` went from
177 ms to 55 ms in the same benchmark.

## Runtime schemas

Requires `--features runtime-schema`:

```bash
taskset -c 2 cargo bench --features runtime-schema --bench model -- '^schema/' --warm-up-time 1 --measurement-time 3 --noplot
```

- `schema/all`: `RuntimeSchemaSet::try_from(&model)`, every data shape and
  operation.
- `schema/service`: `RuntimeSchemaSet::builder(&model).for_service(..)`, with the
  model's knowledge index already built.
- `schema/service_cold_knowledge`: the same on a freshly parsed model, so it
  includes building the knowledge index that service selection uses.
- `schema/operation_lookup`: `RuntimeSchemaSet::operation` plus reading the input
  schema.

Same machine and fixture commit as above, crate commit `052d4dddc` plus the
uncommitted `runtime_schema` module:

| Model | `schema/all` | `schema/service` | `schema/service_cold_knowledge` | `schema/operation_lookup` |
| ----- | -----------: | ---------------: | ------------------------------: | ------------------------: |
| SQS   |      0.13 ms |          0.15 ms |                         0.41 ms |                     42 ns |
| S3    |      1.27 ms |          1.38 ms |                         2.51 ms |                     40 ns |
| EC2   |      8.73 ms |          9.21 ms |                        19.23 ms |                     44 ns |

Building schemas costs about 5–8% of parsing the same model. Service selection
first computed effective errors with `OperationKnowledge::errors_in`, which
walks the service's containment graph once per operation; EC2's 769 operations
made `schema/service` 27.3 ms. Deriving the same errors from the service's
common errors and each operation's own errors brought it to 9.2 ms.
