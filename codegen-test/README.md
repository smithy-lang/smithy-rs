# Codegen Integration Test
This module defines an integration test of the code generation machinery. `.build.gradle.kts` will generate a `smithy-build.json` file as part of the build. The Smithy build plugin then invokes our codegen machinery and generates Rust crates.

The `test` task will run `cargo check` and `cargo clippy` to validate that the generated Rust compiles and is idiomatic.
## Usage
```
../gradlew test
```
