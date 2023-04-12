# Smithy Rust/Python Server SDK example

This folder contains an example service called Pokémon Service used to showcase
the service framework Python bindings capabilities and to run benchmarks.

The Python implementation of the service can be found inside
[pokemon_service.py](/rust-runtime/aws-smithy-http-python-server/examples/pokemon_service.py).

* [Build](#build)
    * [Build dependencies](#build-dependencies)
    * [Makefile](#makefile)
    * [Python stub generation](#python-stub-generation)
* [Run](#run)
* [Test](#test)
* [Uvloop](#uvloop)
* [MacOs](#macos)

## Build

Since this example requires both the server and client SDK to be code-generated
from their [model](/codegen-server-test/model/pokemon.smithy), a Makefile is
provided to build and run the service. Just run `make build` to prepare the first
build.

### Build dependencies

Ensure these dependencies are installed.

```
pip install maturin uvloop aiohttp mypy
```

### Makefile

The build logic is drive by the Makefile:

* `make codegen`: run the codegenerator.
* `make build`: build the Maturin package in debug mode (includes type stubs
  generation).
* `make release`: build the Maturin package in release mode (includes type stub
  generation).
* `make install`: install the latest release or debug build.
* `make run`: run the example server.
* `make test`: run the end-to-end integration test.

### Python stub generation

We support the generation of mypy python stubs and every SDK crate ships with
a script called `stubgen.sh`. **Note that the script is not called
automatically as part of the build**. We suggest users to call it after code generation.
It will do first compilation of the crate, generate the types and exit.

The script takes some command line arguments:

```
./stubgen.sh module_name manifest_path output_directory
```

* module_name: name of the Python module to generate stubs for, IE `pokemon_service_server_sdk`.
* manifest_path: path for the crate manifest used to build the types.
* output_directory: directory where to generate the stub hierarchy. **This
  directory should be a folder `python/$module_name` in the root of the Maturin package.**

## Run

`make run` can be used to start the Pokémon service on `http://localhost:13734`.

## Test

`make test` can be used to spawn the Python service and run some simple integration
tests against it.

More info can be found in the `tests` folder of `pokemon-service-test` package.

## Uvloop

The server can depend on [uvloop](https://pypi.org/project/uvloop/) for a
faster event loop implementation. Uvloop can be installed with your favourite
package manager or by using pip:

```sh
pip instal uvloop
```

and it will be automatically used instead of the standard library event loop if
it is found in the dependencies' closure.

## MacOs

To compile and test on MacOs, please follow the official PyO3 guidelines on how
to [configure your linker](https://pyo3.rs/latest/building_and_distribution.html?highlight=rustflags#macos).

Please note that the `.cargo/config.toml` with linkers override can be local to
your project.
