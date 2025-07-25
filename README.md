Smithy Rust [![CI on Branch `main`](https://github.com/smithy-lang/smithy-rs/actions/workflows/ci-main.yml/badge.svg)](https://github.com/smithy-lang/smithy-rs/actions/workflows/ci-main.yml)
==================================================================================

Smithy code generators for Rust that generate clients, servers, and the entire AWS SDK.
The latest unreleased SDK build can be found in [aws-sdk-rust/next](https://github.com/awslabs/aws-sdk-rust/tree/next).

[Design documentation](https://smithy-lang.github.io/smithy-rs/design/)

**All internal and external interfaces are considered unstable and subject to change without notice.**

Setup
-----

1. `./gradlew` will setup gradle for you. JDK 17 is required.
2. Running tests requires a working Rust installation. See [Rust docs](https://www.rust-lang.org/learn/get-started) for
installation instructions on your platform. The MSRV (**M**inimum **S**upported **R**ust **V**ersion) for the crates in this project is `stable-2`, i.e. the current `stable` Rust version and the prior two versions. Older versions may work.

Development
-----------

For development, pre-commit hooks make it easier to pass automated linting when opening a pull request. Setup:
```bash
brew install pre-commit # (or appropriate for your platform: https://pre-commit.com/)
pre-commit install
```

Project Layout
--------------

* `aws`: AWS specific codegen & Rust code (signing, endpoints, customizations, etc.)
  Common commands:
  * `./gradlew :aws:sdk:assemble`: Generate (but do not test / compile etc.) a fresh SDK into `sdk/build/aws-sdk`
  * `./gradlew :aws:sdk:sdkTest`: Generate & run all tests for a fresh SDK. (Note that these tests require Go to be
  installed for FIP support to compile properly)
  * `./gradlew :aws:sdk:{cargoCheck, cargoTest, cargoDocs, cargoClippy}`: Generate & run specified cargo command.
* `codegen-core`: Common code generation logic useful for clients and servers
* `codegen-client`: Smithy client code generation
* `codegen-client-test`: Smithy protocol test generation & integration tests for Smithy client whitelabel code
* [`design`](design): Design documentation. See the [design/README.md](design/README.md) for details about building / viewing.
* `codegen-server`: Smithy server code generation
* `codegen-server-test`: Smithy protocol test generation & integration tests for Smithy server whitelabel code
* `examples`: A collection of server implementation examples

Testing
-------

Running all of smithy-rs's tests can take a very long time, so it's better to know which parts
to test based on the changes being made, and allow continuous integration to find other issues
when posting a pull request.

In general, the components of smithy-rs affect each other in the following order (with earlier affecting later):

1. `rust-runtime`
2. `codegen` and `codegen-server`
3. `aws/rust-runtime`
4. `aws/codegen-aws-sdk`

Some components, such as `codegen-client-test` and `codegen-server-test`, are purely for testing other components.

### Testing `rust-runtime` and `aws/rust-runtime`

To test the `rust-runtime` crates:

```bash
# Run all Rust tests for `rust-runtime/` (from repo root):
cargo test --manifest-path=rust-runtime/Cargo.toml
# Run clippy for `rust-runtime/` (from repo root):
cargo clippy --manifest-path=rust-runtime/Cargo.toml

# Or
cd rust-runtime
cargo test
cargo clippy
```

To test the `aws/rust-runtime` crates:

```bash
# Run all Rust tests for `aws/rust-runtime/` (from repo root):
cargo test --manifest-path=aws/rust-runtime/Cargo.toml
# Run clippy for `aws/rust-runtime/` (from repo root):
cargo clippy --manifest-path=aws/rust-runtime/Cargo.toml

# Or
cd aws/rust-runtime
cargo test
cargo clippy
```

Some runtime crates have a `additional-ci` script that can also be run. These scripts often require
[`cargo-hack`](https://github.com/taiki-e/cargo-hack) and [`cargo-udeps`](https://github.com/est31/cargo-udeps)
to be installed.

### Testing Client/Server Codegen

To test the code generation, the following can be used:

```bash
# Run Kotlin codegen unit tests
./gradlew codegen-core:check
./gradlew codegen-client:check
./gradlew codegen-server:check
# Run client codegen tests
./gradlew codegen-client-test:check
# Run server codegen tests
./gradlew codegen-server-test:check
```

Several Kotlin unit tests generate Rust projects and compile them. When these fail, they typically
output links to the location of the generated code so that it can be inspected.

To look at generated code when the codegen tests fail, check these paths depending on the test suite that's failing:
- For codegen-client-test: `codegen-client-test/build/smithyprojections/codegen-client-test`
- For codegen-server-test: `codegen-server-test/build/smithyprojections/codegen-server-test`

### Testing SDK Codegen

See the readme in `aws/sdk/` for more information about these targets as they can be configured
to generate more or less AWS service clients.

```bash
# Run Kotlin codegen unit tests
./gradlew aws:codegen-aws-sdk:check
# Generate an SDK, but do not attempt to compile / run tests. Useful for inspecting generated code
./gradlew :aws:sdk:assemble
# Run all the tests
./gradlew :aws:sdk:sdkTest
# Validate that the generated code compiles
./gradlew :aws:sdk:cargoCheck
# Validate that the generated code passes Clippy
./gradlew :aws:sdk:cargoClippy
# Validate the generated docs
./gradlew :aws:sdk:cargoDoc
```

The generated SDK will be placed in `aws/sdk/build/aws-sdk`.
