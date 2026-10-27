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

The [`multi-protocol`](multi-protocol/README.md) client showcase demonstrates
selecting all five built-in protocols at runtime with one generated SDK.

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

The examples use `pokemon-service-server-schema-sdk`, generated with
`schemaSerde: true`, aliased as `pokemon-service-server-sdk` in their Cargo
manifests so the Rust imports stay the same. The separate
`pokemon-service-server-sdk` projection uses `schemaSerde: false` and the original
REST JSON model for comparison with mainline-generated code.

The schema Pokémon server SDK also imports
[`pokemon-multi-protocol.smithy`](../codegen-server-test/custom-test-models/pokemon-multi-protocol.smithy).
This overlay applies AWS JSON 1.0, AWS JSON 1.1, REST XML, and RPC v2 CBOR to the
existing Pokémon service, which already declares REST JSON. The shared Pokémon
models remain unchanged, and the REST JSON client continues to work.

REST requests identify their protocol with the operation's derived `Content-Type`.
Pokémon lookups use `application/json` for REST JSON and `application/xml` for
REST XML, even though their input is bound entirely to the URI.

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
