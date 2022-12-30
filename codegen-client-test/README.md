# Codegen Integration Tests

This module defines integration tests of the code generation machinery.
`./build.gradle.kts` will generate a `smithy-build.json` file as part of the
build. The `rust-client-codegen` Smithy build plugin then invokes our codegen
machinery and generates Rust crates, one for each of the integration test
services defined under `model/`.

## Usage

These commands are all meant to be run from the repository root.

To run all protocol tests of all the integration test services:

```sh
./gradlew codegen-client-test:build
```

To run only a _subset_ of the integration test services (refer to
`./build.gradle.kts` for a full list):

```sh
./gradlew codegen-client-test:build -P modules='simple,rest_json'
```

The Gradle task will run `cargo check`, `cargo test`, `cargo docs` and `cargo
clippy` by default on all the generated Rust crates. You can also specify a
subset of these commands. For instance, if you're working on documentation and
want to check that the crates also compile, you can run:

```sh
./gradlew codegen-client-test:build -P cargoCommands='check,docs'
```

For fast development iteration cycles on protocol tests, we recommend you write
a codegen _unit_ test with a minimal service definition and only run that unit
test.  Alternatively, you can write a minimal integration test service
definition in `model/simple.smithy` and run:

```sh
./gradlew codegen-client-test:build -P cargoCommands='test' -P modules='simple'
```
