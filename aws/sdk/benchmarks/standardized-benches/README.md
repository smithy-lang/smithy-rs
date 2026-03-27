Standardized Benchmarks
=======================

Cross-SDK standardized benchmarks for the AWS Rust SDK, covering two categories:

- **E2E (end-to-end)**: Measures API call latency (DynamoDB) and throughput (S3) against live AWS services.
- **Endpoint**: Measures endpoint resolution performance for S3 and Lambda.

## Why a separate crate from sdk-perf?

`sdk-perf` is designed for RoadRunner. The standardized benchmark spec does not prescribe the use of RoadRunner and proposes using different benchmark frameworks. For now, it is cleaner to keep a separate crate without polluting `sdk-perf`.

## Why not criterion?

Plain binaries with simple stats are useful for generating flamegraphs. Additionally, extracting results in a custom output format is not straightforward with criterion, which matters when we want to migrate to RoadRunner since it defines its own output format.

## Prerequisites

- AWS credentials configured (via environment, profile, etc.)
- The SDK must be built first: `./gradlew :aws:sdk:assemble` from the smithy-rs root

## E2E Benchmarks

E2E benchmarks run against live AWS services. Each benchmark is driven by a JSON config file.

### S3

```bash
cargo run --bin s3_e2e -- --config-path e2e-configs/s3-upload-256KiB-throughput-benchmark.json
cargo run --bin s3_e2e -- --config-path e2e-configs/s3-download-256KiB-throughput-benchmark.json
```

Example output (S3 upload, `cpuStats` in %, `memoryStats` in MB):

```json
{
  "name": "s3-upload-256KiB-throughput-benchmark",
  "iterations": [
    { "totalTimeSeconds": 4.93, "throughputGbps": 4.25 },
    { "totalTimeSeconds": 4.80, "throughputGbps": 4.37 },
    { "totalTimeSeconds": 5.19, "throughputGbps": 4.04 }
  ],
  "cpuStats": {
    "mean": 79.72,
    "max": 108.64
  },
  "memoryStats": {
    "mean": 2943.24,
    "max": 2944.67
  }
}
```

### DynamoDB

```bash
cargo run --bin ddb_e2e -- --config-path e2e-configs/ddb-putitem-1KiB-latency-benchmark.json
cargo run --bin ddb_e2e -- --config-path e2e-configs/ddb-getitem-1KiB-latency-benchmark.json
```

Example output (DynamoDB, `cpuStats` in %, `memoryStats` in MB):

```json
{
  "name": "ddb-putitem-1KiB-latency-benchmark",
  "latencyStats": {
    "meanMs": 66.51297124400017,
    "p50Ms": 65.959283,
    "p90Ms": 67.485355,
    "p99Ms": 77.957288,
    "stdDevMs": 2.224000635462994
  },
  "cpuStats": {
    "mean": 0.2734170206661882,
    "max": 9.876543045043945
  },
  "memoryStats": {
    "mean": 13.51953125,
    "max": 13.51953125
  }
}
```

### Config Format

Config files control e2e benchmark behavior. See the files in `e2e-configs/` for more examples.

For S3, `batch.concurrency` can be specified to control how many tasks are in flight concurrently.

```json
{
  "version": 1,
  "name": "s3-upload-256KiB-throughput-benchmark",
  "description": "S3 PutObject throughput benchmark",
  "service": "s3",
  "action": "upload",
  "actionConfig": {
    ...
  },
  "batch": {
    "numberOfActions": 10000,
    "sequentialExecution": false,
    "concurrency": 100
  },
  ...
}
```

## Endpoint Benchmarks

Endpoint benchmarks measure endpoint resolution performance without making network calls.

```bash
cargo run --bin s3_endpoint
cargo run --bin lambda_endpoint
```

Example output (S3 endpoint):

```json
{
  "product_id": "aws-sdk-rust",
  "results": [
    {
      "description": "S3 outposts vanilla test",
      "id": "s3_outposts_endpoint_resolution",
      "mean_ns": 2132.6549815498156,
      "median_ns": 2123.0,
      "n": 9214,
      "outliers_removed": 786,
      "p90_ns": 2318,
      "p99_ns": 3439,
      "std_dev_ns": 73.8085510800762
    },
    ...
  ]
}
```
