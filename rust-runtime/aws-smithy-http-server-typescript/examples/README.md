# Smithy Rust/Typescript Server SDK example

This folder contains an example service called Pokémon Service used to showcase
the service framework Typescript bindings capabilities and to run benchmarks.

The Python implementation of the service can be found inside
[pokemon_service.ts](/rust-runtime/aws-smithy-http-typescript-server/examples/pokemon_service.ts).

## Depedencies

TODO: Add NAPI installation instructions

## Build

Since this example requires both the server and client SDK to be code-generated
from their [model](/codegen-server-test/model/pokemon.smithy), a Makefile is
provided to build and run the service. Just run `make build` to prepare the first
build.

Once the example has been built successfully the first time, idiomatic `cargo`
can be used directly.

`make distclean` can be used for a complete cleanup of all artefacts.

## Test

`cargo test` can be used to spawn the Python service and run some simple integration
tests against it.

More info can be found in the `tests` folder of `pokemon-service-test` package.
