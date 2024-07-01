# Smithy Rust Server SDK examples

This folder contains:
- example services showcasing Smithy Rust Server SDK, also known as the Rust service framework,
- benchmarking tooling

Three server implementations are available:

- `/pokemon-service`, a HTTP server demonstrating [middleware] and [extractors].
- `/pokemon-service-tls`, a HTTPS server. This server can do
   its own TLS negotiation, rather than relying on a load balancer.
- `/pokemon-service-lambda`, a server that can be deployed onto AWS Lambda.

These servers, and their clients, are generated using smithy-rs. You're invited
to benchmark the performance of these servers to see whether smithy-rs might be
a suitable choice for implementing your web service.

[middleware]: https://smithy-lang.github.io/smithy-rs/design/server/middleware.html
[extractors]: https://smithy-lang.github.io/smithy-rs/design/server/from_parts.html


## Pre-requisites

You will need install Java 17 to run the smithy-rs code generator and an
installation of Rust, including `cargo`, to compile the generated code.

(Optional) The [Cargo Lambda](https://cargo-lambda.info/) sub-command for
`cargo` is required to support the AWS Lambda integration.


## Building

Since these examples require both the server and client SDK to be code-generated
from their [model](/codegen-core/common-test-models/pokemon.smithy), a Makefile is
provided to build and run the service. Just run `make` to prepare the first
build.

Once the example has been built successfully the first time, idiomatic `cargo`
can be used directly.

### Make targets:

- `codegen`: generates the Pokémon service crates (default)
- `build`: compiles the generated client and server
- `clean`: deletes build artifacts
- `clippy`: lints the code
- `distclean`: delete generated code and build artifacts
- `doc-open`: builds and opens the rustdoc documentation
- `lambda_invoke`: invokes a running server
- `lambda_watch`: runs the service on an emulated AWS Lambda environment
- `run`: runs the Pokémon service
- `test`: runs integration and unit tests


## Running services

To run one of the three server implementations locally, provide the appropriate
service name to the `--bin` flag:

```bash
cargo run --bin pokemon-service[(-lambda|-tls)]
```

CLI arguments can be passed to the server binaries by adding them after `--`.
For example, to see a service's help information, use the following:

```bash
cargo run --bin <service> -- --help
```

## Testing

The `/pokemon-test*/tests` folders provide integration tests involving the
generated clients.

They can be invoked with `cargo test`. This will spawn each service in turn
and run some integration tests against it. Use `-p <package>` to filter by
package.

More info can be found in the `tests` folder of each package.


## Benchmarking

Servers running locally (see "Running services") can be benchmarked with any
load testing tool, such as Artillery or `wrk`.

Please see [BENCHMARKS.md](/examples/BENCHMARKS.md) for benchmarking results
produced by the smithy-rs team.
