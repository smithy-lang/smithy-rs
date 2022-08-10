# Smithy Rust/Python Server SDK example

This folder contains an example service called Pokémon Service used to showcase
the service framework Python bindings capabilities and to run benchmarks.

The Python implementation of the service can be found inside
[pokemon_service.py](/rust-runtime/aws-smithy-http-python-server/examples/pokemon_service.py).

## Build

Since this example requires both the server and client SDK to be code-generated
from their [model](/codegen-server-test/model/pokemon.smithy), a Makefile is
provided to build and run the service. Just run `make build` to prepare the first
build.

Once the example has been built successfully the first time, idiomatic `cargo`
can be used directly.

`make distclean` can be used for a complete cleanup of all artefacts.

### Uvloop

The server can depend on [uvloop](https://pypi.org/project/uvloop/) for a
faster event loop implementation. Uvloop can be installed with your favourite
package manager or by using pip:

```sh
pip instal uvloop
```

and it will be automatically used instead of the standard library event loop if
it is found in the dependencies' closure.

### MacOs

To compile and test on MacOs, please follow the official PyO3 guidelines on how
to [configure your linker](https://pyo3.rs/latest/building_and_distribution.html?highlight=rustflags#macos).

Please note that the `.cargo/config.toml` with linkers override can be local to
your project.

## Run

`cargo run` can be used to start the Pokémon service on
`http://localhost:13734`.

## Test

`cargo test` can be used to spawn the Python service and run some simple integration
tests against it.

More info can be found in the `tests` folder of `pokemon-service-test` package.
