# Previous Release Comparison Benchmark

Compares the **current locally-built SDK** against the **latest published release** on crates.io
using [Criterion](https://github.com/bheisler/criterion.rs) for statistically rigorous measurement.

This is useful for measuring the performance impact of local changes (on any branch) relative to
the last released version. The benchmark uses a mock HTTP client so results reflect pure SDK
overhead with no network variability.

## Running

First, build the SDK from your current branch and navigate to the benchmark directory:

```bash
./gradlew :aws:sdk:assemble
cd aws/sdk/benchmarks/previous-release-comparison
```

Then run benchmarks:

```bash
# Run all benchmarks (previous release vs current)
cargo bench

# Run only the current (local build) benchmark
cargo bench -- "compare/current"

# Run only the previous (published release) benchmark
cargo bench -- "compare/previous"
```

## Output

Criterion reports per-iteration timing with confidence intervals for both `previous` (published
release) and `current` (local build). On subsequent runs it also reports whether performance
changed relative to the last run.
