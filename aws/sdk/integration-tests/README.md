# Handwritten Integration Test Root

This folder contains handwritten integration tests that are specific to
individual services. In order for your test to be merged into the final artifact:

- The crate name must match the generated crate name, e.g. `kms`, `dynamodb`
- Your test must be placed into the `tests` folder. **Everything else in your test crate is ignored.**

The contents of the `test` folder will be combined with code-generated integration
tests & inserted into the `tests` folder of the final generated service crate.

## Benchmarks

Some integration test roots have a `benches/` directory. In these, `cargo bench` can be
invoked to run the benchmarks against the current version of smithy-rs. To compare
across smithy-rs versions, you can `git checkout` the version to compare against,
run the benchmark, and then `git checkout` the other version and run it again:

```bash
# For example, this was the very first commit that had a benchmark
git checkout 1fd6e978ae43fb8139cc091997f0ab76ae9fdafa

# Re-generate the SDK before benchmarking to make sure you have the correct code
./gradlew :aws:sdk:assemble

# The DynamoDB integration tests have benchmarks. Let's run them
cd aws/sdk/integration-tests/dynamodb
cargo bench

# Record the results...

# Now, run against the latest
git checkout main

# Re-generate the SDK before benchmarking to make sure you have the correct code
cd ../../../..
./gradlew :aws:sdk:assemble

# Go run the same benchmarks with the latest version
cd aws/sdk/integration-tests/dynamodb
cargo bench

# Compare!
```

## Adding dependencies to tests

When adding new dependencies or adding new features to old dependencies, don't forget to update the
[`IntegrationTestDependencies` file][IntegrationTestDependencies]. Otherwise, after your tests have been copied into
their respective SDK crates may fail when run due to a dependency resolution error.

[IntegrationTestDependencies]: ../../codegen-aws-sdk/src/main/kotlin/software/amazon/smithy/rustsdk/IntegrationTestDependencies.kt
