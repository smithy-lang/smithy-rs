# Smithy Rust Server SDK examples

This folder contains an example services showcasing the service framework capabilities and to run benchmarks.

- `/pokemon-service`, a HTTP server implementation demonstrating [middleware](https://awslabs.github.io/smithy-rs/design/server/middleware.html)
and [extractors](https://awslabs.github.io/smithy-rs/design/server/from_parts.html).
- `/pokemon-service-tls`, a minimal HTTPS server implementation.
- `/pokemon-service-lambda`, a minimal Lambda deployment.

The `/{binary}/tests` folders are integration tests involving the generated clients.

## Build

Since this example requires both the server and client SDK to be code-generated
from their [model](/codegen-server-test/model/pokemon.smithy), a Makefile is
provided to build and run the service. Just run `make` to prepare the first
build.

Once the example has been built successfully the first time, idiomatic `cargo`
can be used directly.

`make distclean` can be used for a complete cleanup of all artefacts.

## Run

To run a binary use

```bash
cargo run -p $BINARY
```

CLI arguments can be passed to the servers, use

```bash
cargo run -p $BINARY -- --help
```

for more information.

## Test

`cargo test` can be used to spawn a service and run some simple integration
tests against it. Use `-p $BINARY` to filter by package.

More info can be found in the `tests` folder of each package.

## Benchmarks

Please see [BENCHMARKS.md](/examples/BENCHMARKS.md).
