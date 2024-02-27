# S3 Express Benchmark

This directory contains S3 Express One Zone benchmarks that measure end-to-end throughput when using the AWS Rust SDK to put, get, and delete objects to/from S3 Express One Zone buckets. We use [`Criterion`](https://github.com/bheisler/criterion.rs) for benchmarks.

Performance numbers will vary depending on the benchmarking environment, but relative performance should still be accurate (i.e. regular S3 bucket vs. S3 Express bucket or comparing with a previous release of the Rust SDK).

## Benchmark targets
- `put_get_delete`: `PutObject`, `GetObject`, and `DeleteObject` using sequential invocations (20 by default) of operations across different buckets, switching buckets on every request and using both 64KB and 1MB objects.
- `concurrent_put_get`: Schedule the equal number of async tasks of `PutObject` (20 by default) to different buckets, wait for completion, then schedule the equal number of async tasks of `GetObject` to different buckets, and wait for completion, using the 64KB objects.

## Running benchmarks
Example of running the `put_get_delete` benchmark in local dev environment:

```bash
BUCKETS=test0--usw2-az1--x-s3,test1--usw2-az1--x-s3 cargo bench --bench put_get_delete
```
To configure how the benchmark is run, set the following environment variables:
#### required
- `BUCKETS`: a list of comma separated bucket names

#### optional
- `CONFIDENCE_LEVEL`: the confidence level for benchmarks in a group (0.99 by default)
- `NUMBER_OF_ITERATIONS`: the number of times a set of operations runs for measurement (20 by default)
- `PROF`: whether it uses the default profiler or [`PProfProfiler`](https://github.com/tikv/pprof-rs); specify `pprof` for the latter (use the default, otherwise)
- `PROF_FREQUENCY`: how often `pprof` collects samples per second (10 by default)
- `SAMPLE_SIZE`: the size of the sample for benchmarks in a group (10 by default)

### Flamegraph generation
When `PROF=pprof` is set, `criterion` uses a `PProfProfiler` and can generate a flamegraph:
```bash
PROF=pprof BUCKETS=test0--usw2-az1--x-s3,test1--usw2-az1--x-s3 cargo bench --bench put_get_delete -- --profile-time 300
```
It is important to specify `--profile-time`, otherwise no flamegraph is generated ([issue]( https://github.com/tikv/pprof-rs/issues/127)).
Make sure to specify a value large enough for benchmarks to finish running within (if the default profiler is used, we can see how long it takes for each benchmark to run).

Finally, it is important to set `PROF_FREQUENCY` to a lower value if `criterion` did not exit normally ([issue](https://github.com/tikv/pprof-rs/issues/237)):
```bash
error: bench failed, to rerun pass `--bench put_get_delete`

Caused by:
  process didn't exit successfully: `src/smithy-rs/target/release/deps/put_get_delete-a80309fea01754e4 --profile-time 300 --bench` (signal: 5, SIGTRAP: trace/breakpoint trap)
```

The resulting flamegraph is stored at `target/criterion/<name-of-benchmark>/profile/flamegraph.svg`.


## Limitation
Benchmarks currently measure end-to-end throughput of operations, including both the Rust SDK latency and the server side latency. To detect regressions in the Rust SDK reliably, we should only capture the time taken before sending a request and after receiving a response.
