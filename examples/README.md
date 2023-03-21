# Smithy Rust Server SDK example

This folder contains an example service called Pokémon Service used to showcase
the service framework capabilities and to run benchmarks.

## Build

Since this example requires both the server and client SDK to be code-generated
from their [model](/codegen-server-test/model/pokemon.smithy), a Makefile is
provided to build and run the service. Just run `make` to prepare the first
build.

Once the example has been built successfully the first time, idiomatic `cargo`
can be used directly.

`make distclean` can be used for a complete cleanup of all artefacts.

## Run

`cargo run` can be used to start the Pokémon service on
`http://localhost:13734`.

## Test

`cargo test` can be used to spawn the service and run some simple integration
tests against it.

More info can be found in the `tests` folder of `pokemon-service` package.

## Benchmarks

Please see [BENCHMARKS.md](/rust-runtime/aws-smithy-http-server/examples/BENCHMARKS.md).
