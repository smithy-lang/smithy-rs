# Codegen Integration Test
This module defines an integration test of the code generation machinery. Models defined in `model` are built and generated into a Rust package. A `cargoCheck` Gradle task ensures that the generated Rust code compiles. This is added as a finalizer of the `test` task. 

## Usage
```
# Compile codegen, Regenerate Rust, compile:
# REPO_ROOT allows the runtime deps to be specified properly:
REPO_ROOT=../ ../gradlew test
```

The `smithy-build.json` configures the runtime dependencies to point directly to `../rust-runtime/*` via relative paths.