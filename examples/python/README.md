# Smithy Rust/Python Server SDK example

This folder contains an example service called Pokémon Service used to showcase
the service framework Python bindings capabilities and to run benchmarks.

The Python implementation of the service can be found inside
[pokemon_service.py](./pokemon_service.py).

* [Build](#build)
    * [Build dependencies](#build-dependencies)
    * [Makefile](#makefile)
    * [Python stub generation](#python-stub-generation)
* [Run](#run)
* [Test](#test)
* [MacOs](#macos)
* [Running servers on AWS Lambda](#running-servers-on-aws-lambda)

## Build

Since this example requires both the server and client SDK to be code-generated
from their [model](/codegen-server-test/model/pokemon.smithy), a Makefile is
provided to build and run the service. Just run `make build` to prepare the first
build.

### Build dependencies

Ensure these dependencies are installed.

```bash
pip install maturin uvloop aiohttp mypy
```

The server can depend on [uvloop](https://pypi.org/project/uvloop/) for a faster
event loop implementation and it will be automatically used instead of the standard
library event loop if it is found in the dependencies' closure.

### Makefile

The build logic is drive by the Makefile:

* `make codegen`: run the codegenerator.
* `make build`: build the Maturin package in debug mode (includes type stubs
  generation).
* `make release`: build the Maturin package in release mode (includes type stubs
  generation).
* `make install-wheel`: install the latest release or debug wheel using `pip`.
* `make run`: run the example server.
* `make test`: run the end-to-end integration test.
* `make clean`: clean the Cargo artefacts.
* `make dist-clean`: clean the Cargo artefacts and generated folders.

### Python stub generation

We support the generation of mypy python stubs and every SDK crate ships with
a script called `stubgen.sh`. **Note that the script is not called
automatically as part of the build**. We suggest users to call it after code generation.
It will do first compilation of the crate, generate the types and exit.

The script takes some command line arguments:

```bash
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

## MacOs

To compile and test on MacOs, please follow the official PyO3 guidelines on how
to [configure your linker](https://pyo3.rs/latest/building_and_distribution.html?highlight=rustflags#macos).

Please note that the `.cargo/config.toml` with linkers override can be local to
your project.

## Running servers on AWS Lambda

`aws-smithy-http-server-python` supports running your services on [AWS Lambda](https://aws.amazon.com/lambda/).

You need to use `run_lambda` method instead of `run` method to start
the [custom runtime](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-custom.html)
instead of the [Hyper](https://hyper.rs/) HTTP server.

In your `app.py`:

```diff
from pokemon_service_server_sdk import App
from pokemon_service_server_sdk.error import ResourceNotFoundException

# ...

# Get the number of requests served by this server.
@app.get_server_statistics
def get_server_statistics(
    _: GetServerStatisticsInput, context: Context
) -> GetServerStatisticsOutput:
    calls_count = context.get_calls_count()
    logging.debug("The service handled %d requests", calls_count)
    return GetServerStatisticsOutput(calls_count=calls_count)

# ...

-app.run()
+app.run_lambda()
```

`aws-smithy-http-server-python` comes with a
[custom runtime](https://docs.aws.amazon.com/lambda/latest/dg/runtimes-custom.html)
so you should run your service without any provided runtimes.
You can achieve that with a `Dockerfile` similar to this:

```dockerfile
# You can use any image that has your desired Python version
FROM public.ecr.aws/lambda/python:3.8-x86_64

# Copy your application code to `LAMBDA_TASK_ROOT`
COPY app.py ${LAMBDA_TASK_ROOT}

# When you build your Server SDK for your service, you will get a Python wheel.
# You just need to copy that wheel and install it via `pip` inside your image.
# Note that you need to build your library for Linux, and Python version used to
# build your SDK should match with your image's Python version.
# For cross compiling, you can consult to:
# https://pyo3.rs/latest/building_and_distribution.html#cross-compiling
COPY wheels/ ${LAMBDA_TASK_ROOT}/wheels
RUN pip3 install ${LAMBDA_TASK_ROOT}/wheels/*.whl

# You can install your application's other dependencies listed in `requirements.txt`.
COPY requirements.txt .
RUN pip3 install -r requirements.txt --target "${LAMBDA_TASK_ROOT}"

# Create a symlink for your application's entrypoint,
# so we can use `/app.py` to refer it
RUN ln -s ${LAMBDA_TASK_ROOT}/app.py /app.py

# By default `public.ecr.aws/lambda/python` images comes with Python runtime,
# we need to override `ENTRYPOINT` and `CMD` to not call that runtime and
# instead run directly your service and it will start our custom runtime.
ENTRYPOINT [ "/var/lang/bin/python3.8" ]
CMD [ "/app.py" ]
```

See [https://docs.aws.amazon.com/lambda/latest/dg/images-create.html#images-create-from-base](https://docs.aws.amazon.com/lambda/latest/dg/images-create.html#images-create-from-base)
for more details on building your custom image.
