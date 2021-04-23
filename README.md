# Smithy Rust ![status](https://github.com/awslabs/smithy-rs/workflows/CI/badge.svg)
Smithy code generators for Rust

The nightly SDK build can be found under `Actions -> CI (take latest run) -> Artifacts`

**All internal and external interfaces are considered unstable and subject to change without notice.**

## Setup
1. `./gradlew` will setup gradle for you. JDK 11 is required.
2. Running tests requires a working Rust installation. See [Rust docs](https://www.rust-lang.org/learn/get-started) for
installation instructions on your platform. Minimum supported Rust version is the latest released Rust version, although older versions may work.

## Generate an AWS SDK
The generated SDK will be placed in `aws/sdk/build/aws-sdk`.
```
./gradlew :aws:sdk:assemble # Generate an SDK. Do not attempt to compile / run tests
./gradlew :aws:sdk:test # Run all the tests
./gradlew :aws:sdk:cargoCheck # only validate that it compiles
```
## Run tests
```./test.sh```

This will run all the unit tests, codegen example models & Dynamo DB, validate that the generated code compiles, and run any tests targeting the generated code.

## Development
For development, pre-commit hooks may be useful. Setup:
```
brew install pre-commit # (or appropriate for your platform: https://pre-commit.com/)
pre-commit install
```

### Project Layout
* `aws`: AWS specific codegen & Rust code (signing, endpoints, customizations, etc.)
* `codegen`: Whitelabel Smithy code generation
* `codegen-test`: Smithy protocol test generation & integration tests for Smithy whitelabel code
